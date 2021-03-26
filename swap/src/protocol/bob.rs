use crate::database::Database;
use crate::env::Config;
use crate::network::quote::BidQuote;
use crate::network::{encrypted_signature, quote, redial, spot_price, transfer_proof};
use crate::protocol::bob;
use crate::{bitcoin, monero};
use anyhow::{anyhow, Error, Result};
use libp2p::core::Multiaddr;
use libp2p::request_response::{RequestId, ResponseChannel};
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
    Failure {
        peer: PeerId,
        error: Error,
    },
    /// "Fallback" variant that allows the event mapping code to swallow certain
    /// events that we don't want the caller to deal with.
    Other,
}

impl OutEvent {
    pub fn unexpected_request(peer: PeerId) -> OutEvent {
        OutEvent::Failure {
            peer,
            error: anyhow!("Unexpected request received"),
        }
    }

    pub fn unexpected_response(peer: PeerId) -> OutEvent {
        OutEvent::Failure {
            peer,
            error: anyhow!("Unexpected response received"),
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
