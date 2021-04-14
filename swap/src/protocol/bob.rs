use crate::database::Database;
use crate::env::Config;
use crate::network::quote::BidQuote;
use crate::network::{encrypted_signature, quote, redial, spot_price, transfer_proof};
use crate::protocol::bob;
use crate::{bitcoin, monero};
use anyhow::{anyhow, Error, Result};
use libp2p::core::Multiaddr;
use libp2p::request_response::{
    RequestId, RequestResponseEvent, RequestResponseMessage, ResponseChannel,
};
use libp2p::{NetworkBehaviour, PeerId};
use std::sync::Arc;
use uuid::Uuid;

pub use self::cancel::cancel;
pub use self::event_loop::{EventLoop, EventLoopHandle};
pub use self::refund::refund;
pub use self::state::*;
pub use self::swap::{run, run_until};
use std::time::Duration;

pub mod cancel;
pub mod event_loop;
mod execution_setup;
pub mod refund;
pub mod state;
pub mod swap;

pub struct Swap {
    pub state: BobState,
    pub event_loop_handle: bob::EventLoopHandle,
    pub db: Database,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub env_config: Config,
    pub swap_id: Uuid,
    pub receive_monero_address: ::monero::Address,
}

pub struct Builder {
    swap_id: Uuid,
    db: Database,

    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,

    init_params: InitParams,
    env_config: Config,

    event_loop_handle: EventLoopHandle,

    receive_monero_address: ::monero::Address,
}

enum InitParams {
    None,
    New { btc_amount: bitcoin::Amount },
}

impl Builder {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Database,
        swap_id: Uuid,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        env_config: Config,
        event_loop_handle: EventLoopHandle,
        receive_monero_address: ::monero::Address,
    ) -> Self {
        Self {
            swap_id,
            db,
            bitcoin_wallet,
            monero_wallet,
            init_params: InitParams::None,
            env_config,
            event_loop_handle,
            receive_monero_address,
        }
    }

    pub fn with_init_params(self, btc_amount: bitcoin::Amount) -> Self {
        Self {
            init_params: InitParams::New { btc_amount },
            ..self
        }
    }

    pub fn build(self) -> Result<bob::Swap> {
        let state = match self.init_params {
            InitParams::New { btc_amount } => BobState::Started { btc_amount },
            InitParams::None => self.db.get_state(self.swap_id)?.try_into_bob()?.into(),
        };

        Ok(Swap {
            state,
            event_loop_handle: self.event_loop_handle,
            db: self.db,
            bitcoin_wallet: self.bitcoin_wallet.clone(),
            monero_wallet: self.monero_wallet.clone(),
            swap_id: self.swap_id,
            env_config: self.env_config,
            receive_monero_address: self.receive_monero_address,
        })
    }
}

#[derive(Debug)]
pub enum OutEvent {
    QuoteReceived {
        id: RequestId,
        response: BidQuote,
    },
    SpotPriceReceived {
        id: RequestId,
        response: spot_price::Response,
    },
    ExecutionSetupDone(Box<Result<State2>>),
    TransferProofReceived {
        msg: Box<transfer_proof::Request>,
        channel: ResponseChannel<()>,
    },
    EncryptedSignatureAcknowledged {
        id: RequestId,
    },
    AllRedialAttemptsExhausted {
        peer: PeerId,
    },
    ResponseSent, // Same variant is used for all messages as no processing is done
    CommunicationError(Error),
}

impl OutEvent {
    fn unexpected_request() -> OutEvent {
        OutEvent::CommunicationError(anyhow!("Unexpected request received"))
    }

    fn unexpected_response() -> OutEvent {
        OutEvent::CommunicationError(anyhow!("Unexpected response received"))
    }
}

impl From<quote::Message> for OutEvent {
    fn from(message: quote::Message) -> Self {
        match message {
            quote::Message::Request { .. } => OutEvent::unexpected_request(),
            quote::Message::Response {
                response,
                request_id,
            } => OutEvent::QuoteReceived {
                id: request_id,
                response,
            },
        }
    }
}

impl From<spot_price::Message> for OutEvent {
    fn from(message: spot_price::Message) -> Self {
        match message {
            spot_price::Message::Request { .. } => OutEvent::unexpected_request(),
            spot_price::Message::Response {
                response,
                request_id,
            } => OutEvent::SpotPriceReceived {
                id: request_id,
                response,
            },
        }
    }
}

impl From<transfer_proof::Message> for OutEvent {
    fn from(message: transfer_proof::Message) -> Self {
        match message {
            transfer_proof::Message::Request {
                request, channel, ..
            } => OutEvent::TransferProofReceived {
                msg: Box::new(request),
                channel,
            },
            transfer_proof::Message::Response { .. } => OutEvent::unexpected_response(),
        }
    }
}

impl From<encrypted_signature::Message> for OutEvent {
    fn from(message: encrypted_signature::Message) -> Self {
        match message {
            encrypted_signature::Message::Request { .. } => OutEvent::unexpected_request(),
            encrypted_signature::Message::Response { request_id, .. } => {
                OutEvent::EncryptedSignatureAcknowledged { id: request_id }
            }
        }
    }
}

impl From<spot_price::OutEvent> for OutEvent {
    fn from(event: spot_price::OutEvent) -> Self {
        map_rr_event_to_outevent(event)
    }
}

impl From<quote::OutEvent> for OutEvent {
    fn from(event: quote::OutEvent) -> Self {
        map_rr_event_to_outevent(event)
    }
}

impl From<transfer_proof::OutEvent> for OutEvent {
    fn from(event: transfer_proof::OutEvent) -> Self {
        map_rr_event_to_outevent(event)
    }
}

impl From<encrypted_signature::OutEvent> for OutEvent {
    fn from(event: encrypted_signature::OutEvent) -> Self {
        map_rr_event_to_outevent(event)
    }
}

impl From<redial::OutEvent> for OutEvent {
    fn from(event: redial::OutEvent) -> Self {
        match event {
            redial::OutEvent::AllAttemptsExhausted { peer } => {
                OutEvent::AllRedialAttemptsExhausted { peer }
            }
        }
    }
}

fn map_rr_event_to_outevent<I, O>(event: RequestResponseEvent<I, O>) -> OutEvent
where
    OutEvent: From<RequestResponseMessage<I, O>>,
{
    use RequestResponseEvent::*;

    match event {
        Message { message, .. } => OutEvent::from(message),
        ResponseSent { .. } => OutEvent::ResponseSent,
        InboundFailure { peer, error, .. } => OutEvent::CommunicationError(anyhow!(
            "protocol with peer {} failed due to {:?}",
            peer,
            error
        )),
        OutboundFailure { peer, error, .. } => OutEvent::CommunicationError(anyhow!(
            "protocol with peer {} failed due to {:?}",
            peer,
            error
        )),
    }
}

impl From<execution_setup::OutEvent> for OutEvent {
    fn from(event: execution_setup::OutEvent) -> Self {
        match event {
            execution_setup::OutEvent::Done(res) => OutEvent::ExecutionSetupDone(Box::new(res)),
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Bob.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pub quote: quote::Behaviour,
    pub spot_price: spot_price::Behaviour,
    pub execution_setup: execution_setup::Behaviour,
    pub transfer_proof: transfer_proof::Behaviour,
    pub encrypted_signature: encrypted_signature::Behaviour,
    pub redial: redial::Behaviour,
}

impl Behaviour {
    pub fn new(alice: PeerId) -> Self {
        Self {
            quote: quote::bob(),
            spot_price: spot_price::bob(),
            execution_setup: Default::default(),
            transfer_proof: transfer_proof::bob(),
            encrypted_signature: encrypted_signature::bob(),
            redial: redial::Behaviour::new(alice, Duration::from_secs(2)),
        }
    }

    /// Add a known address for the given peer
    pub fn add_address(&mut self, peer_id: PeerId, address: Multiaddr) {
        self.quote.add_address(&peer_id, address.clone());
        self.spot_price.add_address(&peer_id, address.clone());
        self.transfer_proof.add_address(&peer_id, address.clone());
        self.encrypted_signature.add_address(&peer_id, address);
    }
}
