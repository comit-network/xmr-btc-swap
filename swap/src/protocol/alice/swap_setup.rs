use crate::protocol::alice::event_loop::LatestRate;
use crate::protocol::alice::{State0, State3};
use crate::protocol::{alice, Message0, Message2, Message4};
use crate::{bitcoin, env, monero};
use anyhow::{anyhow, Context as _, Result};
use futures::future::{BoxFuture, OptionFuture};
use futures::FutureExt;
use libp2p::core::connection::ConnectionId;
use libp2p::core::upgrade::{from_fn, FromFnUpgrade};
use libp2p::core::{upgrade, Endpoint};
use libp2p::swarm::{
    KeepAlive, NegotiatedSubstream, NetworkBehaviour, NetworkBehaviourAction, PollParameters,
    ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use libp2p::{Multiaddr, PeerId};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt::Debug;
use std::future;
use std::task::{Context, Poll};
use uuid::Uuid;
use void::Void;

#[derive(Debug)]
pub enum OutEvent {
    Initiated {
        send_wallet_snapshot: bmrng::RequestReceiver<bitcoin::Amount, WalletSnapshot>,
    },
    Completed {
        peer_id: PeerId,
        swap_id: Uuid,
        state3: State3,
    },
    Error {
        peer_id: PeerId,
        error: Error
    }
}

#[derive(Debug)]
pub struct WalletSnapshot {
    balance: monero::Amount,
    lock_fee: monero::Amount,

    // TODO: Consider using the same address for punish and redeem (they are mutually exclusive, so
    // effectively the address will only be used once)
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,

    redeem_fee: bitcoin::Amount,
    punish_fee: bitcoin::Amount,
}

impl WalletSnapshot {
    pub async fn capture(
        bitcoin_wallet: &bitcoin::Wallet,
        monero_wallet: &monero::Wallet,
        transfer_amount: bitcoin::Amount,
    ) -> Result<Self> {
        let balance = monero_wallet.get_balance().await?;
        let redeem_address = bitcoin_wallet.new_address().await?;
        let punish_address = bitcoin_wallet.new_address().await?;
        let redeem_fee = bitcoin_wallet
            .estimate_fee(bitcoin::TxRedeem::weight(), transfer_amount)
            .await?;
        let punish_fee = bitcoin_wallet
            .estimate_fee(bitcoin::TxPunish::weight(), transfer_amount)
            .await?;

        Ok(Self {
            balance,
            lock_fee: monero::MONERO_FEE,
            redeem_address,
            punish_address,
            redeem_fee,
            punish_fee,
        })
    }
}

impl From<OutEvent> for alice::OutEvent {
    fn from(event: OutEvent) -> Self {
        match event {
            OutEvent::Initiated {
                send_wallet_snapshot,
            } => alice::OutEvent::SwapSetupInitiated {
                send_wallet_snapshot,
            },
            OutEvent::Completed {
                peer_id: bob_peer_id,
                swap_id,
                state3,
            } => alice::OutEvent::SwapSetupCompleted {
                peer_id: bob_peer_id,
                swap_id,
                state3: Box::new(state3),
            },
            OutEvent::Error { peer_id, error} => alice::OutEvent::Failure {
                peer: peer_id,
                error: anyhow!(error),
            },
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct BlockchainNetwork {
    #[serde(with = "crate::bitcoin::network")]
    pub bitcoin: bitcoin::Network,
    #[serde(with = "crate::monero::network")]
    pub monero: monero::Network,
}

#[allow(missing_debug_implementations)]
pub struct Behaviour<LR> {
    events: VecDeque<OutEvent>,
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    env_config: env::Config,

    latest_rate: LR,
    resume_only: bool,
}

impl<LR> Behaviour<LR> {
    pub fn new(
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
        env_config: env::Config,
        latest_rate: LR,
        resume_only: bool,
    ) -> Self {
        Self {
            events: Default::default(),
            min_buy,
            max_buy,
            env_config,
            latest_rate,
            resume_only,
        }
    }
}

impl<LR> NetworkBehaviour for Behaviour<LR>
where
    LR: LatestRate + Send + 'static + Clone,
{
    type ProtocolsHandler = Handler<LR>;
    type OutEvent = OutEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        Handler::new(
            self.min_buy,
            self.max_buy,
            self.env_config,
            self.latest_rate.clone(),
            self.resume_only,
        )
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        todo!()
    }

    fn inject_connected(&mut self, peer_id: &PeerId) {
        todo!()
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        todo!()
    }

    fn inject_event(&mut self, peer_id: PeerId, connection: ConnectionId, event: HandlerOutEvent) {
        match event {
            HandlerOutEvent::Initiated(send_wallet_snapshot) => {
                self.events.push_back(OutEvent::Initiated { send_wallet_snapshot })
            }
            HandlerOutEvent::Completed(swap_setup_result) => {
                match swap_setup_result {
                    Ok((swap_id, state3)) => {
                        self.events.push_back(OutEvent::Completed {
                            peer_id,
                            swap_id,
                            state3
                        })
                    }
                    Err(error) => {
                        self.events.push_back(OutEvent::Error {
                            peer_id,
                            error
                        })
                    }
                }

            }
        }
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        _params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<HandlerInEvent, Self::OutEvent>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }

        Poll::Pending
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpotPriceRequest {
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc: bitcoin::Amount,
    pub blockchain_network: BlockchainNetwork,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SpotPriceResponse {
    Xmr(monero::Amount),
    Error(SpotPriceError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SpotPriceError {
    NoSwapsAccepted,
    AmountBelowMinimum {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        min: bitcoin::Amount,
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },
    AmountAboveMaximum {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        max: bitcoin::Amount,
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },
    BalanceTooLow {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },
    BlockchainNetworkMismatch {
        cli: BlockchainNetwork,
        asb: BlockchainNetwork,
    },
    /// To be used for errors that cannot be explained on the CLI side (e.g.
    /// rate update problems on the seller side)
    Other,
}

// TODO: This is bob only.
// enum OutboundState {
//     PendingOpen(
//         // TODO: put data in here we pass in when we want to kick of swap
// setup, just bitcoin amount?     ),
//     PendingNegotiate,
//     Executing(BoxFuture<'static, anyhow::Result<(Uuid, bob::State3)>>)
// }

type InboundStream = BoxFuture<'static, anyhow::Result<(Uuid, alice::State3), Error>>;

pub struct Handler<LR> {
    inbound_stream: OptionFuture<InboundStream>,
    events: VecDeque<HandlerOutEvent>,

    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    env_config: env::Config,

    latest_rate: LR,
    resume_only: bool,
}

impl<LR> Handler<LR> {
    fn new(
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
        env_config: env::Config,
        latest_rate: LR,
        resume_only: bool,
    ) -> Self {
        Self {
            inbound_stream: OptionFuture::from(None),
            events: Default::default(),
            min_buy,
            max_buy,
            env_config,
            latest_rate,
            resume_only,
        }
    }
}

pub enum HandlerOutEvent {
    Initiated(bmrng::RequestReceiver<bitcoin::Amount, WalletSnapshot>),
    Completed(anyhow::Result<(Uuid, alice::State3), Error>),
}

pub enum HandlerInEvent {}

pub const BUF_SIZE: usize = 1024 * 1024;

impl<LR> ProtocolsHandler for Handler<LR>
where
    LR: LatestRate + Send + 'static,
{
    type InEvent = HandlerInEvent;
    type OutEvent = HandlerOutEvent;
    type Error = Error;
    type InboundProtocol = protocol::SwapSetup;
    type OutboundProtocol = upgrade::DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(protocol::new(), todo!("pass data down to handler"))
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        mut substream: NegotiatedSubstream,
        _: Self::InboundOpenInfo,
    ) {
        let (sender, receiver) = bmrng::channel_with_timeout::<bitcoin::Amount, WalletSnapshot>(
            1,
            todo!("decide on timeout"),
        );
        let resume_only = self.resume_only;
        let min_buy = self.min_buy;
        let max_buy = self.max_buy;
        let latest_rate = self.latest_rate.latest_rate();
        let env_config = self.env_config;

        // TODO: Put a timeout on the whole future
        self.inbound_stream = OptionFuture::from(Some(
            async move {
                let request = read_cbor_message::<SpotPriceRequest>(&mut substream).await.map_err(|e| Error::Io(e))?;
                let wallet_snapshot = sender.send_receive(request.btc).await.map_err(|e| Error::WalletSnapshotFailed(anyhow!(e)))?;

                // wrap all of these into another future so we can `return` from all the
                // different blocks
                let validate = async {
                    if resume_only {
                        return Err(Error::ResumeOnlyMode);
                    };

                    let blockchain_network = BlockchainNetwork {
                        bitcoin: env_config.bitcoin_network,
                        monero: env_config.monero_network,
                    };

                    if request.blockchain_network != blockchain_network {
                        return Err(Error::BlockchainNetworkMismatch {
                            cli: request.blockchain_network,
                            asb: blockchain_network,
                        });
                    }

                    let btc = request.btc;

                    if btc < min_buy {
                        return Err(Error::AmountBelowMinimum {
                            min: min_buy,
                            buy: btc,
                        });
                    }

                    if btc > max_buy {
                        return Err(Error::AmountAboveMaximum {
                            max: max_buy,
                            buy: btc,
                        });
                    }

                    let rate =
                        latest_rate.map_err(|e| Error::LatestRateFetchFailed(Box::new(e)))?;
                    let xmr = rate
                        .sell_quote(btc)
                        .map_err(|e| Error::SellQuoteCalculationFailed(e))?;

                    if wallet_snapshot.balance < xmr + wallet_snapshot.lock_fee {
                        return Err(Error::BalanceTooLow {
                            balance: wallet_snapshot.balance,
                            buy: btc,
                        });
                    }

                    Ok(xmr)
                };

                let xmr = match validate.await {
                    Ok(xmr) => {
                        write_cbor_message(&mut substream, SpotPriceResponse::Xmr(xmr)).await.map_err(|e| Error::Io(e))?;

                        xmr
                    }
                    Err(e) => {
                        write_cbor_message(
                            &mut substream,
                            SpotPriceResponse::Error(e.to_error_response()),
                        )
                        .await
                            .map_err(|e| Error::Io(e))?;
                        return Err(e.into());
                    }
                };

                let state0 = State0::new(
                    request.btc,
                    xmr,
                    env_config,
                    wallet_snapshot.redeem_address,
                    wallet_snapshot.punish_address,
                    wallet_snapshot.redeem_fee,
                    wallet_snapshot.punish_fee,
                    &mut rand::thread_rng(),
                );

                let message0 = read_cbor_message::<Message0>(&mut substream)
                    .await
                    .context("Failed to deserialize message0")
                    .map_err(|e| Error::Io(e))?;
                let (swap_id, state1) = state0.receive(message0).map_err(|e| Error::Io(e))?;

                write_cbor_message(&mut substream, state1.next_message()).await.map_err(|e| Error::Io(e))?;

                let message2 = read_cbor_message::<Message2>(&mut substream)
                    .await
                    .context("Failed to deserialize message2")
                    .map_err(|e| Error::Io(e))?;
                let state2 = state1
                    .receive(message2)
                    .context("Failed to receive Message2")
                    .map_err(|e| Error::Io(e))?;

                write_cbor_message(&mut substream, state2.next_message()).await.map_err(|e| Error::Io(e))?;

                let message4 = read_cbor_message::<Message4>(&mut substream)
                    .await
                    .context("Failed to deserialize message4")
                    .map_err(|e| Error::Io(e))?;
                let state3 = state2
                    .receive(message4)
                    .context("Failed to receive Message4")
                    .map_err(|e| Error::Io(e))?;

                Ok((swap_id, state3))
            }
            .boxed(),
        ));

        self.events.push_back(HandlerOutEvent::Initiated(receiver));
    }

    fn inject_fully_negotiated_outbound(&mut self, protocol: Void, info: Self::OutboundOpenInfo) {
        unreachable!("we don't support outbound")
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        todo!()
    }

    fn inject_dial_upgrade_error(
        &mut self,
        info: Self::OutboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<Void>,
    ) {
        todo!()
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        todo!()
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ProtocolsHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        if let Some(result) = futures::ready!(self.inbound_stream.poll_unpin(cx)) {
            return Poll::Ready(ProtocolsHandlerEvent::Custom(HandlerOutEvent::Completed(
                result,
            )));
        }

        Poll::Pending
    }
}

async fn read_cbor_message<T>(substream: &mut NegotiatedSubstream) -> Result<T>
where
    T: DeserializeOwned,
{
    let bytes = upgrade::read_one(substream, BUF_SIZE).await?;
    let mut de = serde_cbor::Deserializer::from_slice(&bytes);
    let message = T::deserialize(&mut de)?;

    Ok(message)
}

async fn write_cbor_message<T>(substream: &mut NegotiatedSubstream, message: T) -> Result<()>
where
    T: Serialize,
{
    let bytes = serde_cbor::to_vec(&message)?;
    upgrade::write_one(substream, &bytes).await?;

    Ok(())
}

mod protocol {
    use super::*;

    pub fn new() -> SwapSetup {
        from_fn(
            b"/comit/xmr/btc/swap_setup/1.0.0",
            Box::new(|socket, endpoint| {
                future::ready(match endpoint {
                    Endpoint::Listener => Ok(socket),
                    Endpoint::Dialer => todo!("return error"),
                })
            }),
        )
    }

    pub type SwapSetup = FromFnUpgrade<
        &'static [u8],
        Box<
            dyn Fn(
                    NegotiatedSubstream,
                    Endpoint,
                ) -> future::Ready<Result<NegotiatedSubstream, Void>>
                + Send
                + 'static,
        >,
    >;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ASB is running in resume-only mode")]
    ResumeOnlyMode,
    #[error("Amount {buy} below minimum {min}")]
    AmountBelowMinimum {
        min: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    #[error("Amount {buy} above maximum {max}")]
    AmountAboveMaximum {
        max: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    #[error("Balance {balance} too low to fulfill swapping {buy}")]
    BalanceTooLow {
        balance: monero::Amount,
        buy: bitcoin::Amount,
    },
    #[error("Failed to fetch latest rate")]
    LatestRateFetchFailed(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
    #[error("Failed to calculate quote: {0}")]
    SellQuoteCalculationFailed(#[source] anyhow::Error),
    #[error("Blockchain networks did not match, we are on {asb:?}, but request from {cli:?}")]
    BlockchainNetworkMismatch {
        cli: BlockchainNetwork,
        asb: BlockchainNetwork,
    },
    #[error("Io Error: {0}")]
    Io(anyhow::Error),
    #[error("Failed to request wallet snapshot: {0}")]
    WalletSnapshotFailed(anyhow::Error)
}

impl Error {
    pub fn to_error_response(&self) -> SpotPriceError {
        match self {
            Error::ResumeOnlyMode => SpotPriceError::NoSwapsAccepted,
            Error::AmountBelowMinimum { min, buy } => SpotPriceError::AmountBelowMinimum {
                min: *min,
                buy: *buy,
            },
            Error::AmountAboveMaximum { max, buy } => SpotPriceError::AmountAboveMaximum {
                max: *max,
                buy: *buy,
            },
            Error::BalanceTooLow { buy, .. } => SpotPriceError::BalanceTooLow { buy: *buy },
            Error::BlockchainNetworkMismatch { cli, asb } => {
                SpotPriceError::BlockchainNetworkMismatch {
                    cli: *cli,
                    asb: *asb,
                }
            }
            Error::LatestRateFetchFailed(_)
            | Error::SellQuoteCalculationFailed(_)
            | Error::WalletSnapshotFailed(_)
            | Error::Io(_) => {
                SpotPriceError::Other
            }
        }
    }
}
