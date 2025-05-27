use crate::network::swap_setup::{protocol, BlockchainNetwork, SpotPriceError, SpotPriceResponse};
use crate::protocol::bob::{State0, State2};
use crate::protocol::{Message1, Message3};
use crate::{bitcoin, cli, env, monero};
use anyhow::{Context, Result};
use futures::future::{BoxFuture, OptionFuture};
use futures::AsyncWriteExt;
use futures::FutureExt;
use libp2p::core::upgrade;
use libp2p::swarm::{
    ConnectionDenied, ConnectionHandler, ConnectionHandlerEvent, ConnectionId, FromSwarm,
    NetworkBehaviour, SubstreamProtocol, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use libp2p::{Multiaddr, PeerId};
use std::collections::VecDeque;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use uuid::Uuid;

use super::{read_cbor_message, write_cbor_message, SpotPriceRequest};

#[allow(missing_debug_implementations)]
pub struct Behaviour {
    env_config: env::Config,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    new_swaps: VecDeque<(PeerId, NewSwap)>,
    completed_swaps: VecDeque<(PeerId, Completed)>,
}

impl Behaviour {
    pub fn new(env_config: env::Config, bitcoin_wallet: Arc<bitcoin::Wallet>) -> Self {
        Self {
            env_config,
            bitcoin_wallet,
            new_swaps: VecDeque::default(),
            completed_swaps: VecDeque::default(),
        }
    }

    pub async fn start(&mut self, alice: PeerId, swap: NewSwap) {
        self.new_swaps.push_back((alice, swap))
    }
}

impl From<Completed> for cli::OutEvent {
    fn from(completed: Completed) -> Self {
        cli::OutEvent::SwapSetupCompleted(Box::new(completed.0))
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type ToSwarm = Completed;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(self.env_config, self.bitcoin_wallet.clone()))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _addr: &Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(self.env_config, self.bitcoin_wallet.clone()))
    }

    fn on_swarm_event(&mut self, _event: FromSwarm<'_>) {
        // We do not need to handle swarm events
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _connection_id: libp2p::swarm::ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.completed_swaps.push_back((peer_id, event));
    }

    fn poll(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        // Forward completed swaps from the connection handler to the swarm
        if let Some((_peer, completed)) = self.completed_swaps.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(completed));
        }

        // If there is a new swap to be started, send it to the connection handler
        if let Some((peer, event)) = self.new_swaps.pop_front() {
            return Poll::Ready(ToSwarm::NotifyHandler {
                peer_id: peer,
                handler: libp2p::swarm::NotifyHandler::Any,
                event,
            });
        }

        Poll::Pending
    }
}

type OutboundStream = BoxFuture<'static, Result<State2, Error>>;

pub struct Handler {
    outbound_stream: OptionFuture<OutboundStream>,
    env_config: env::Config,
    timeout: Duration,
    new_swaps: VecDeque<NewSwap>,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    keep_alive: bool,
}

impl Handler {
    fn new(env_config: env::Config, bitcoin_wallet: Arc<bitcoin::Wallet>) -> Self {
        Self {
            env_config,
            outbound_stream: OptionFuture::from(None),
            timeout: Duration::from_secs(120),
            new_swaps: VecDeque::default(),
            bitcoin_wallet,
            keep_alive: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewSwap {
    pub swap_id: Uuid,
    pub btc: bitcoin::Amount,
    pub tx_lock_fee: bitcoin::Amount,
    pub tx_refund_fee: bitcoin::Amount,
    pub tx_cancel_fee: bitcoin::Amount,
    pub bitcoin_refund_address: bitcoin::Address,
}

#[derive(Debug)]
pub struct Completed(Result<State2>);

impl ConnectionHandler for Handler {
    type FromBehaviour = NewSwap;
    type ToBehaviour = Completed;
    type InboundProtocol = upgrade::DeniedUpgrade;
    type OutboundProtocol = protocol::SwapSetup;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = NewSwap;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        // Bob does not support inbound substreams
        SubstreamProtocol::new(upgrade::DeniedUpgrade, ())
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
            libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedInbound(_) => {
                unreachable!("Bob does not support inbound substreams")
            }
            libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedOutbound(outbound) => {
                let mut substream = outbound.protocol;
                let new_swap_request = outbound.info;

                let bitcoin_wallet = self.bitcoin_wallet.clone();
                let env_config = self.env_config;

                let protocol = tokio::time::timeout(self.timeout, async move {
                    let result = async {
                        // Here we request the spot price from Alice
                        write_cbor_message(
                            &mut substream,
                            SpotPriceRequest {
                                btc: new_swap_request.btc,
                                blockchain_network: BlockchainNetwork {
                                    bitcoin: env_config.bitcoin_network,
                                    monero: env_config.monero_network,
                                },
                            },
                        )
                        .await
                        .context("Failed to send spot price request to Alice")?;

                        // Here we read the spot price response from Alice
                        // The outer ? checks if Alice responded with an error (SpotPriceError)
                        let xmr = Result::from(
                            // The inner ? is for the read_cbor_message function
                            // It will return an error if the deserialization fails
                            read_cbor_message::<SpotPriceResponse>(&mut substream)
                                .await
                                .context("Failed to read spot price response from Alice")?,
                        )?;

                        let state0 = State0::new(
                            new_swap_request.swap_id,
                            &mut rand::thread_rng(),
                            new_swap_request.btc,
                            xmr,
                            env_config.bitcoin_cancel_timelock,
                            env_config.bitcoin_punish_timelock,
                            new_swap_request.bitcoin_refund_address.clone(),
                            env_config.monero_finality_confirmations,
                            new_swap_request.tx_refund_fee,
                            new_swap_request.tx_cancel_fee,
                            new_swap_request.tx_lock_fee,
                        );

                        write_cbor_message(&mut substream, state0.next_message())
                            .await
                            .context("Failed to send state0 message to Alice")?;
                        let message1 = read_cbor_message::<Message1>(&mut substream)
                            .await
                            .context("Failed to read message1 from Alice")?;
                        let state1 = state0
                            .receive(bitcoin_wallet.as_ref(), message1)
                            .await
                            .context("Failed to receive state1")?;
                        write_cbor_message(&mut substream, state1.next_message())
                            .await
                            .context("Failed to send state1 message")?;
                        let message3 = read_cbor_message::<Message3>(&mut substream)
                            .await
                            .context("Failed to read message3 from Alice")?;
                        let state2 = state1
                            .receive(message3)
                            .context("Failed to receive state2")?;

                        write_cbor_message(&mut substream, state2.next_message())
                            .await
                            .context("Failed to send state2 message")?;

                        substream
                            .flush()
                            .await
                            .context("Failed to flush substream")?;
                        substream
                            .close()
                            .await
                            .context("Failed to close substream")?;

                        Ok(state2)
                    }
                    .await;

                    result.map_err(|e: anyhow::Error| {
                        tracing::error!("Error occurred during swap setup protocol: {:#}", e);
                        Error::Other
                    })
                });

                let max_seconds = self.timeout.as_secs();

                self.outbound_stream = OptionFuture::from(Some(Box::pin(async move {
                    protocol.await.map_err(|_| Error::Timeout {
                        seconds: max_seconds,
                    })?
                })
                    as OutboundStream));

                // Once the outbound stream is created, we keep the connection alive
                self.keep_alive = true;
            }
            _ => {}
        }
    }

    fn on_behaviour_event(&mut self, new_swap: Self::FromBehaviour) {
        self.new_swaps.push_back(new_swap);
    }

    fn connection_keep_alive(&self) -> bool {
        self.keep_alive
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        // Check if there is a new swap to be started
        if let Some(new_swap) = self.new_swaps.pop_front() {
            self.keep_alive = true;

            // We instruct the swarm to start a new outbound substream
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(protocol::new(), new_swap),
            });
        }

        // Check if the outbound stream has completed
        if let Poll::Ready(Some(result)) = self.outbound_stream.poll_unpin(cx) {
            self.outbound_stream = None.into();

            // Once the outbound stream is completed, we no longer keep the connection alive
            self.keep_alive = false;

            // We notify the swarm that the swap setup is completed / failed
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Completed(
                result.map_err(anyhow::Error::from),
            )));
        }

        Poll::Pending
    }
}

impl From<SpotPriceResponse> for Result<monero::Amount, Error> {
    fn from(response: SpotPriceResponse) -> Self {
        match response {
            SpotPriceResponse::Xmr(amount) => Ok(amount),
            SpotPriceResponse::Error(e) => Err(e.into()),
        }
    }
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("Seller currently does not accept incoming swap requests, please try again later")]
    NoSwapsAccepted,
    #[error("Seller refused to buy {buy} because the minimum configured buy limit is {min}")]
    AmountBelowMinimum {
        min: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    #[error("Seller refused to buy {buy} because the maximum configured buy limit is {max}")]
    AmountAboveMaximum {
        max: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    #[error("Seller's XMR balance is currently too low to fulfill the swap request to buy {buy}, please try again later")]
    BalanceTooLow { buy: bitcoin::Amount },

    #[error("Seller blockchain network {asb:?} setup did not match your blockchain network setup {cli:?}")]
    BlockchainNetworkMismatch {
        cli: BlockchainNetwork,
        asb: BlockchainNetwork,
    },

    #[error("Failed to complete swap setup within {seconds}s")]
    Timeout { seconds: u64 },

    /// To be used for errors that cannot be explained on the CLI side (e.g.
    /// rate update problems on the seller side)
    #[error("Seller encountered a problem, please try again later.")]
    Other,
}

impl From<SpotPriceError> for Error {
    fn from(error: SpotPriceError) -> Self {
        match error {
            SpotPriceError::NoSwapsAccepted => Error::NoSwapsAccepted,
            SpotPriceError::AmountBelowMinimum { min, buy } => {
                Error::AmountBelowMinimum { min, buy }
            }
            SpotPriceError::AmountAboveMaximum { max, buy } => {
                Error::AmountAboveMaximum { max, buy }
            }
            SpotPriceError::BalanceTooLow { buy } => Error::BalanceTooLow { buy },
            SpotPriceError::BlockchainNetworkMismatch { cli, asb } => {
                Error::BlockchainNetworkMismatch { cli, asb }
            }
            SpotPriceError::Other => Error::Other,
        }
    }
}
