use crate::network::swap_setup::{
    protocol, read_cbor_message, write_cbor_message, BlockchainNetwork, SpotPriceError,
    SpotPriceRequest, SpotPriceResponse,
};
use crate::protocol::bob::{State0, State2};
use crate::protocol::{Message1, Message3};
use crate::{bitcoin, cli, env, monero};
use anyhow::Result;
use futures::future::{BoxFuture, OptionFuture};
use futures::{AsyncWriteExt, FutureExt};
use libp2p::core::connection::ConnectionId;
use libp2p::core::upgrade;
use libp2p::swarm::{
    KeepAlive, NegotiatedSubstream, NetworkBehaviour, NetworkBehaviourAction, NotifyHandler,
    PollParameters, ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr,
    SubstreamProtocol,
};
use libp2p::{Multiaddr, PeerId};
use std::collections::VecDeque;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use uuid::Uuid;
use void::Void;

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
    type ProtocolsHandler = Handler;
    type OutEvent = Completed;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        Handler::new(self.env_config, self.bitcoin_wallet.clone())
    }

    fn addresses_of_peer(&mut self, _: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
    }

    fn inject_connected(&mut self, _: &PeerId) {}

    fn inject_disconnected(&mut self, _: &PeerId) {}

    fn inject_event(&mut self, peer: PeerId, _: ConnectionId, completed: Completed) {
        self.completed_swaps.push_back((peer, completed));
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        _params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ProtocolsHandler>> {
        if let Some((_, event)) = self.completed_swaps.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }

        if let Some((peer, event)) = self.new_swaps.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::NotifyHandler {
                peer_id: peer,
                handler: NotifyHandler::Any,
                event,
            });
        }

        Poll::Pending
    }
}

type OutboundStream = BoxFuture<'static, Result<State2>>;

pub struct Handler {
    outbound_stream: OptionFuture<OutboundStream>,
    env_config: env::Config,
    timeout: Duration,
    new_swaps: VecDeque<NewSwap>,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    keep_alive: KeepAlive,
}

impl Handler {
    fn new(env_config: env::Config, bitcoin_wallet: Arc<bitcoin::Wallet>) -> Self {
        Self {
            env_config,
            outbound_stream: OptionFuture::from(None),
            timeout: Duration::from_secs(120),
            new_swaps: VecDeque::default(),
            bitcoin_wallet,
            keep_alive: KeepAlive::Yes,
        }
    }
}

#[derive(Debug)]
pub struct NewSwap {
    pub swap_id: Uuid,
    pub btc: bitcoin::Amount,
    pub tx_refund_fee: bitcoin::Amount,
    pub tx_cancel_fee: bitcoin::Amount,
    pub bitcoin_refund_address: bitcoin::Address,
}

#[derive(Debug)]
pub struct Completed(Result<State2>);

impl ProtocolsHandler for Handler {
    type InEvent = NewSwap;
    type OutEvent = Completed;
    type Error = Void;
    type InboundProtocol = upgrade::DeniedUpgrade;
    type OutboundProtocol = protocol::SwapSetup;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = NewSwap;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(upgrade::DeniedUpgrade, ())
    }

    fn inject_fully_negotiated_inbound(&mut self, _: Void, _: Self::InboundOpenInfo) {
        unreachable!("Bob does not support inbound substreams")
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        mut substream: NegotiatedSubstream,
        info: Self::OutboundOpenInfo,
    ) {
        let bitcoin_wallet = self.bitcoin_wallet.clone();
        let env_config = self.env_config;

        let protocol = tokio::time::timeout(self.timeout, async move {
            write_cbor_message(
                &mut substream,
                SpotPriceRequest {
                    btc: info.btc,
                    blockchain_network: BlockchainNetwork {
                        bitcoin: env_config.bitcoin_network,
                        monero: env_config.monero_network,
                    },
                },
            )
            .await?;

            let xmr = Result::from(read_cbor_message::<SpotPriceResponse>(&mut substream).await?)?;

            let state0 = State0::new(
                info.swap_id,
                &mut rand::thread_rng(),
                info.btc,
                xmr,
                env_config.bitcoin_cancel_timelock,
                env_config.bitcoin_punish_timelock,
                info.bitcoin_refund_address,
                env_config.monero_finality_confirmations,
                info.tx_refund_fee,
                info.tx_cancel_fee,
            );

            write_cbor_message(&mut substream, state0.next_message()).await?;
            let message1 = read_cbor_message::<Message1>(&mut substream).await?;
            let state1 = state0.receive(bitcoin_wallet.as_ref(), message1).await?;

            write_cbor_message(&mut substream, state1.next_message()).await?;
            let message3 = read_cbor_message::<Message3>(&mut substream).await?;
            let state2 = state1.receive(message3)?;

            write_cbor_message(&mut substream, state2.next_message()).await?;

            substream.flush().await?;
            substream.close().await?;

            Ok(state2)
        });

        let max_seconds = self.timeout.as_secs();
        self.outbound_stream = OptionFuture::from(Some(
            async move {
                protocol.await.map_err(|_| Error::Timeout {
                    seconds: max_seconds,
                })?
            }
            .boxed(),
        ));
    }

    fn inject_event(&mut self, new_swap: Self::InEvent) {
        self.new_swaps.push_back(new_swap);
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        _: ProtocolsHandlerUpgrErr<Void>,
    ) {
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    #[allow(clippy::type_complexity)]
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
        if let Some(new_swap) = self.new_swaps.pop_front() {
            self.keep_alive = KeepAlive::Yes;
            return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(protocol::new(), new_swap),
            });
        }

        if let Some(result) = futures::ready!(self.outbound_stream.poll_unpin(cx)) {
            self.outbound_stream = OptionFuture::from(None);
            return Poll::Ready(ProtocolsHandlerEvent::Custom(Completed(result)));
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
