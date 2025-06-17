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
use futures::AsyncWriteExt;
use futures::FutureExt;
use libp2p::core::upgrade;
use libp2p::swarm::handler::ConnectionEvent;
use libp2p::swarm::{ConnectionHandler, ConnectionId};
use libp2p::swarm::{ConnectionHandlerEvent, NetworkBehaviour, SubstreamProtocol, ToSwarm};
use libp2p::{Multiaddr, PeerId};
use std::collections::VecDeque;
use std::fmt::Debug;
use std::task::Poll;
use std::time::{Duration, Instant};
use uuid::Uuid;

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
    unlocked_balance: monero::Amount,
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
        monero_wallet: &monero::Wallets,
        external_redeem_address: &Option<bitcoin::Address>,
        transfer_amount: bitcoin::Amount,
    ) -> Result<Self> {
        let unlocked_balance = monero_wallet.main_wallet().await.unlocked_balance().await;
        let total_balance = monero_wallet.main_wallet().await.total_balance().await;

        tracing::info!(%unlocked_balance, %total_balance, "Capturing monero wallet snapshot");

        let redeem_address = external_redeem_address
            .clone()
            .unwrap_or(bitcoin_wallet.new_address().await?);
        let punish_address = external_redeem_address
            .clone()
            .unwrap_or(bitcoin_wallet.new_address().await?);

        let redeem_fee = bitcoin_wallet
            .estimate_fee(bitcoin::TxRedeem::weight(), Some(transfer_amount))
            .await?;
        let punish_fee = bitcoin_wallet
            .estimate_fee(bitcoin::TxPunish::weight(), Some(transfer_amount))
            .await?;

        Ok(Self {
            unlocked_balance: unlocked_balance.into(),
            lock_fee: monero::CONSERVATIVE_MONERO_FEE,
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
                state3,
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
    type ConnectionHandler = Handler<LR>;
    type ToSwarm = OutEvent;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> std::result::Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        // A new inbound connection has been established by Bob
        // He wants to negotiate a swap setup with us
        // We create a new Handler to handle the negotiation
        let handler = Handler::new(
            self.min_buy,
            self.max_buy,
            self.env_config,
            self.latest_rate.clone(),
            self.resume_only,
        );

        Ok(handler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _peer: PeerId,
        _addr: &Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> std::result::Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        // A new outbound connection has been established (probably to a rendezvous node because we dont dial Bob)
        // We still return a handler, because we dont want to close the connection
        let handler = Handler::new(
            self.min_buy,
            self.max_buy,
            self.env_config,
            self.latest_rate.clone(),
            self.resume_only,
        );

        Ok(handler)
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _: ConnectionId,
        event: HandlerOutEvent,
    ) {
        // Here we receive events from the Handler, add some context and forward them to the swarm
        // This is done by pushing the event to the [`events`] queue
        // The queue is then polled in the [`poll`] function, and the events are sent to the swarm
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

    fn poll(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, ()>> {
        // Poll events from the queue and send them to the swarm
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(event));
        }

        Poll::Pending
    }

    fn on_swarm_event(&mut self, _event: libp2p::swarm::FromSwarm<'_>) {
        // We do not need to handle any swarm events here
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

    // This is the timeout for the negotiation phase where Alice and Bob exchange messages
    negotiation_timeout: Duration,

    // If set to None, we will keep the connection alive indefinitely
    // If set to Some, we will keep the connection alive until the given instant
    keep_alive_until: Option<Instant>,
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
            negotiation_timeout: Duration::from_secs(120),
            keep_alive_until: Some(Instant::now() + Duration::from_secs(30)),
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum HandlerOutEvent {
    Initiated(bmrng::RequestReceiver<bitcoin::Amount, WalletSnapshot>),
    Completed(Result<(Uuid, State3)>),
}

impl<LR> ConnectionHandler for Handler<LR>
where
    LR: LatestRate + Send + 'static,
{
    type FromBehaviour = ();
    type ToBehaviour = HandlerOutEvent;
    type InboundProtocol = protocol::SwapSetup;
    type OutboundProtocol = upgrade::DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(protocol::new(), ())
    }

    fn on_connection_event(
        &mut self,
        event: libp2p::swarm::handler::ConnectionEvent<
            '_,
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(substream) => {
                self.keep_alive_until = None;

                let mut substream = substream.protocol;

                let (sender, receiver) = bmrng::channel_with_timeout::<
                    bitcoin::Amount,
                    WalletSnapshot,
                >(1, Duration::from_secs(60));

                let resume_only = self.resume_only;
                let min_buy = self.min_buy;
                let max_buy = self.max_buy;
                let latest_rate = self.latest_rate.latest_rate();
                let env_config = self.env_config;

                // We wrap the entire handshake in a timeout future
                let protocol = tokio::time::timeout(self.negotiation_timeout, async move {
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

                        let rate =
                            latest_rate.map_err(|e| Error::LatestRateFetchFailed(Box::new(e)))?;
                        let xmr = rate
                            .sell_quote(btc)
                            .map_err(Error::SellQuoteCalculationFailed)?;

                        let unlocked = wallet_snapshot.unlocked_balance;

                        let needed_balance = xmr + wallet_snapshot.lock_fee;
                        if unlocked.as_piconero() < needed_balance.as_piconero() {
                            tracing::warn!(
                                unlocked_balance = %unlocked,
                                needed_balance = %needed_balance,
                                "Rejecting swap, unlocked balance too low"
                            );
                            return Err(Error::BalanceTooLow {
                                balance: wallet_snapshot.unlocked_balance,
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

                let max_seconds = self.negotiation_timeout.as_secs();
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
            ConnectionEvent::DialUpgradeError(..) => {
                unreachable!("Alice does not dial")
            }
            ConnectionEvent::FullyNegotiatedOutbound(..) => {
                unreachable!("Alice does not support outbound connections")
            }
            _ => {}
        }
    }

    fn on_behaviour_event(&mut self, _event: Self::FromBehaviour) {
        unreachable!("Alice does not receive events from the Behaviour in the handler")
    }

    fn connection_keep_alive(&self) -> bool {
        // If keep_alive_until is None, we keep the connection alive indefinitely
        // If keep_alive_until is Some, we keep the connection alive until the given instant
        match self.keep_alive_until {
            None => true,
            Some(keep_alive_until) => Instant::now() < keep_alive_until,
        }
    }

    #[allow(clippy::type_complexity)]
    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        // Send events in the queue to the behaviour
        // This is currently only used to notify the behaviour that the negotiation phase has been initiated
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
        }

        if let Some(result) = futures::ready!(self.inbound_stream.poll_unpin(cx)) {
            self.inbound_stream = OptionFuture::from(None);

            // Notify the behaviour that the negotiation phase has been completed
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                HandlerOutEvent::Completed(result),
            ));
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
    #[error("Unlocked balance ({balance}) too low to fulfill swapping {buy}")]
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
