use crate::asb::LatestRate;
use crate::network::swap_setup;
use crate::network::swap_setup::{
    protocol, BlockchainNetwork, SpotPriceError, SpotPriceRequest, SpotPriceResponse,
};
use crate::protocol::alice::{State0, State3};
use crate::protocol::{Message0, Message2, Message4};
use crate::{asb, bitcoin, env, monero};
use anyhow::{anyhow, Context, Result};
use futures::future::{BoxFuture, OptionFuture};
use futures::{AsyncWriteExt, FutureExt};
use libp2p::core::connection::ConnectionId;
use libp2p::core::upgrade;
use libp2p::swarm::{
    KeepAlive, NegotiatedSubstream, NetworkBehaviour, NetworkBehaviourAction, PollParameters,
    ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use libp2p::{Multiaddr, PeerId};
use std::collections::VecDeque;
use std::fmt::Debug;
use std::task::Poll;
use std::time::{Duration, Instant};
use uuid::Uuid;
use void::Void;

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
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
        error: anyhow::Error,
    },
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

impl From<OutEvent> for asb::OutEvent {
    fn from(event: OutEvent) -> Self {
        match event {
            OutEvent::Initiated {
                send_wallet_snapshot,
            } => asb::OutEvent::SwapSetupInitiated {
                send_wallet_snapshot,
            },
            OutEvent::Completed {
                peer_id: bob_peer_id,
                swap_id,
                state3,
            } => asb::OutEvent::SwapSetupCompleted {
                peer_id: bob_peer_id,
                swap_id,
                state3: Box::new(state3),
            },
            OutEvent::Error { peer_id, error } => asb::OutEvent::Failure {
                peer: peer_id,
                error: anyhow!(error),
            },
        }
    }
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

    fn addresses_of_peer(&mut self, _: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
    }

    fn inject_connected(&mut self, _: &PeerId) {}

    fn inject_disconnected(&mut self, _: &PeerId) {}

    fn inject_event(&mut self, peer_id: PeerId, _: ConnectionId, event: HandlerOutEvent) {
        match event {
            HandlerOutEvent::Initiated(send_wallet_snapshot) => {
                self.events.push_back(OutEvent::Initiated {
                    send_wallet_snapshot,
                })
            }
            HandlerOutEvent::Completed(Ok((swap_id, state3))) => {
                self.events.push_back(OutEvent::Completed {
                    peer_id,
                    swap_id,
                    state3,
                })
            }
            HandlerOutEvent::Completed(Err(error)) => {
                self.events.push_back(OutEvent::Error { peer_id, error })
            }
        }
    }

    fn poll(
        &mut self,
        _cx: &mut std::task::Context<'_>,
        _params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<(), Self::OutEvent>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }

        Poll::Pending
    }
}

type InboundStream = BoxFuture<'static, Result<(Uuid, State3)>>;

pub struct Handler<LR> {
    inbound_stream: OptionFuture<InboundStream>,
    events: VecDeque<HandlerOutEvent>,

    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    env_config: env::Config,

    latest_rate: LR,
    resume_only: bool,

    timeout: Duration,
    keep_alive: KeepAlive,
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
            timeout: Duration::from_secs(120),
            keep_alive: KeepAlive::Until(Instant::now() + Duration::from_secs(5)),
        }
    }
}

#[allow(clippy::large_enum_variant)]
pub enum HandlerOutEvent {
    Initiated(bmrng::RequestReceiver<bitcoin::Amount, WalletSnapshot>),
    Completed(Result<(Uuid, State3)>),
}

impl<LR> ProtocolsHandler for Handler<LR>
where
    LR: LatestRate + Send + 'static,
{
    type InEvent = ();
    type OutEvent = HandlerOutEvent;
    type Error = Error;
    type InboundProtocol = protocol::SwapSetup;
    type OutboundProtocol = upgrade::DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(protocol::new(), ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        mut substream: NegotiatedSubstream,
        _: Self::InboundOpenInfo,
    ) {
        self.keep_alive = KeepAlive::Yes;

        let (sender, receiver) = bmrng::channel_with_timeout::<bitcoin::Amount, WalletSnapshot>(
            1,
            Duration::from_secs(5),
        );
        let resume_only = self.resume_only;
        let min_buy = self.min_buy;
        let max_buy = self.max_buy;
        let latest_rate = self.latest_rate.latest_rate();
        let env_config = self.env_config;

        let protocol = tokio::time::timeout(self.timeout, async move {
            let request = swap_setup::read_cbor_message::<SpotPriceRequest>(&mut substream)
                .await
                .context("Failed to read spot price request")?;

            let wallet_snapshot = sender
                .send_receive(request.btc)
                .await
                .context("Failed to receive wallet snapshot")?;

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

                let rate = latest_rate.map_err(|e| Error::LatestRateFetchFailed(Box::new(e)))?;
                let xmr = rate
                    .sell_quote(btc)
                    .map_err(Error::SellQuoteCalculationFailed)?;

                if wallet_snapshot.balance < xmr + wallet_snapshot.lock_fee {
                    return Err(Error::BalanceTooLow {
                        balance: wallet_snapshot.balance,
                        buy: btc,
                    });
                }

                Ok(xmr)
            };

            let result = validate.await;

            swap_setup::write_cbor_message(
                &mut substream,
                SpotPriceResponse::from_result_ref(&result),
            )
            .await
            .context("Failed to write spot price response")?;

            let xmr = result?;

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

            let message0 = swap_setup::read_cbor_message::<Message0>(&mut substream)
                .await
                .context("Failed to read message0")?;
            let (swap_id, state1) = state0
                .receive(message0)
                .context("Failed to transition state0 -> state1 using message0")?;

            swap_setup::write_cbor_message(&mut substream, state1.next_message())
                .await
                .context("Failed to send message1")?;

            let message2 = swap_setup::read_cbor_message::<Message2>(&mut substream)
                .await
                .context("Failed to read message2")?;
            let state2 = state1
                .receive(message2)
                .context("Failed to transition state1 -> state2 using message2")?;

            swap_setup::write_cbor_message(&mut substream, state2.next_message())
                .await
                .context("Failed to send message3")?;

            let message4 = swap_setup::read_cbor_message::<Message4>(&mut substream)
                .await
                .context("Failed to read message4")?;
            let state3 = state2
                .receive(message4)
                .context("Failed to transition state2 -> state3 using message4")?;

            substream
                .flush()
                .await
                .context("Failed to flush substream after all messages were sent")?;
            substream
                .close()
                .await
                .context("Failed to close substream after all messages were sent")?;

            Ok((swap_id, state3))
        });

        let max_seconds = self.timeout.as_secs();
        self.inbound_stream = OptionFuture::from(Some(
            async move {
                protocol.await.with_context(|| {
                    format!("Failed to complete execution setup within {}s", max_seconds)
                })?
            }
            .boxed(),
        ));

        self.events.push_back(HandlerOutEvent::Initiated(receiver));
    }

    fn inject_fully_negotiated_outbound(&mut self, _: Void, _: Self::OutboundOpenInfo) {
        unreachable!("Alice does not support outbound in the handler")
    }

    fn inject_event(&mut self, _: Self::InEvent) {
        unreachable!("Alice does not receive events from the Behaviour in the handler")
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        _: ProtocolsHandlerUpgrErr<Void>,
    ) {
        unreachable!("Alice does not dial")
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    #[allow(clippy::type_complexity)]
    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<
        ProtocolsHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(ProtocolsHandlerEvent::Custom(event));
        }

        if let Some(result) = futures::ready!(self.inbound_stream.poll_unpin(cx)) {
            self.keep_alive = KeepAlive::No;
            self.inbound_stream = OptionFuture::from(None);
            return Poll::Ready(ProtocolsHandlerEvent::Custom(HandlerOutEvent::Completed(
                result,
            )));
        }

        Poll::Pending
    }
}

impl SpotPriceResponse {
    pub fn from_result_ref(result: &Result<monero::Amount, Error>) -> Self {
        match result {
            Ok(amount) => SpotPriceResponse::Xmr(*amount),
            Err(error) => SpotPriceResponse::Error(error.to_error_response()),
        }
    }
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
    #[error("Failed to calculate quote")]
    SellQuoteCalculationFailed(#[source] anyhow::Error),
    #[error("Blockchain networks did not match, we are on {asb:?}, but request from {cli:?}")]
    BlockchainNetworkMismatch {
        cli: BlockchainNetwork,
        asb: BlockchainNetwork,
    },
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
            Error::LatestRateFetchFailed(_) | Error::SellQuoteCalculationFailed(_) => {
                SpotPriceError::Other
            }
        }
    }
}
