use crate::database::Database;
use crate::execution_params::ExecutionParams;
use crate::network::{peer_tracker, spot_price};
use crate::protocol::alice::TransferProof;
use crate::protocol::bob;
use crate::{bitcoin, monero};
use anyhow::{anyhow, Error, Result};
pub use execution_setup::{Message0, Message2, Message4};
use libp2p::core::Multiaddr;
use libp2p::request_response::{RequestResponseMessage, ResponseChannel};
use libp2p::{NetworkBehaviour, PeerId};
use std::sync::Arc;
use tracing::debug;
use uuid::Uuid;

pub use self::cancel::cancel;
pub use self::encrypted_signature::EncryptedSignature;
pub use self::event_loop::{EventLoop, EventLoopHandle};
pub use self::refund::refund;
pub use self::state::*;
pub use self::swap::{run, run_until};
use crate::network::quote;
use crate::network::quote::BidQuote;

pub mod cancel;
mod encrypted_signature;
pub mod event_loop;
mod execution_setup;
pub mod refund;
pub mod state;
pub mod swap;
mod transfer_proof;

pub struct Swap {
    pub state: BobState,
    pub event_loop_handle: bob::EventLoopHandle,
    pub db: Database,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub execution_params: ExecutionParams,
    pub swap_id: Uuid,
    pub receive_monero_address: ::monero::Address,
}

pub struct Builder {
    swap_id: Uuid,
    db: Database,

    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,

    init_params: InitParams,
    execution_params: ExecutionParams,

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
        execution_params: ExecutionParams,
        event_loop_handle: EventLoopHandle,
        receive_monero_address: ::monero::Address,
    ) -> Self {
        Self {
            swap_id,
            db,
            bitcoin_wallet,
            monero_wallet,
            init_params: InitParams::None,
            execution_params,
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
            execution_params: self.execution_params,
            receive_monero_address: self.receive_monero_address,
        })
    }
}

#[derive(Debug)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
    QuoteReceived(BidQuote),
    SpotPriceReceived(spot_price::Response),
    ExecutionSetupDone(Result<Box<State2>>),
    TransferProof {
        msg: Box<TransferProof>,
        channel: ResponseChannel<()>,
    },
    EncryptedSignatureAcknowledged,
    ResponseSent, // Same variant is used for all messages as no processing is done
    CommunicationError(Error),
}

impl From<peer_tracker::OutEvent> for OutEvent {
    fn from(event: peer_tracker::OutEvent) -> Self {
        match event {
            peer_tracker::OutEvent::ConnectionEstablished(id) => {
                OutEvent::ConnectionEstablished(id)
            }
        }
    }
}

impl From<spot_price::OutEvent> for OutEvent {
    fn from(event: spot_price::OutEvent) -> Self {
        match event {
            spot_price::OutEvent::Message {
                message: RequestResponseMessage::Response { response, .. },
                ..
            } => OutEvent::SpotPriceReceived(response),
            spot_price::OutEvent::Message {
                message: RequestResponseMessage::Request { .. },
                ..
            } => OutEvent::CommunicationError(anyhow!(
                "Bob is only meant to receive spot prices, not hand them out"
            )),
            spot_price::OutEvent::ResponseSent { .. } => OutEvent::ResponseSent,
            spot_price::OutEvent::InboundFailure { peer, error, .. } => {
                OutEvent::CommunicationError(anyhow!(
                    "spot_price protocol with peer {} failed due to {:?}",
                    peer,
                    error
                ))
            }
            spot_price::OutEvent::OutboundFailure { peer, error, .. } => {
                OutEvent::CommunicationError(anyhow!(
                    "spot_price protocol with peer {} failed due to {:?}",
                    peer,
                    error
                ))
            }
        }
    }
}

impl From<quote::OutEvent> for OutEvent {
    fn from(event: quote::OutEvent) -> Self {
        match event {
            quote::OutEvent::Message {
                message: RequestResponseMessage::Response { response, .. },
                ..
            } => OutEvent::QuoteReceived(response),
            quote::OutEvent::Message {
                message: RequestResponseMessage::Request { .. },
                ..
            } => OutEvent::CommunicationError(anyhow!(
                "Bob is only meant to receive quotes, not hand them out"
            )),
            quote::OutEvent::ResponseSent { .. } => OutEvent::ResponseSent,
            quote::OutEvent::InboundFailure { peer, error, .. } => {
                OutEvent::CommunicationError(anyhow!(
                    "quote protocol with peer {} failed due to {:?}",
                    peer,
                    error
                ))
            }
            quote::OutEvent::OutboundFailure { peer, error, .. } => {
                OutEvent::CommunicationError(anyhow!(
                    "quote protocol with peer {} failed due to {:?}",
                    peer,
                    error
                ))
            }
        }
    }
}

impl From<execution_setup::OutEvent> for OutEvent {
    fn from(event: execution_setup::OutEvent) -> Self {
        match event {
            execution_setup::OutEvent::Done(res) => OutEvent::ExecutionSetupDone(res.map(Box::new)),
        }
    }
}

impl From<transfer_proof::OutEvent> for OutEvent {
    fn from(event: transfer_proof::OutEvent) -> Self {
        use transfer_proof::OutEvent::*;
        match event {
            MsgReceived { msg, channel } => OutEvent::TransferProof {
                msg: Box::new(msg),
                channel,
            },
            AckSent => OutEvent::ResponseSent,
            Failure(err) => {
                OutEvent::CommunicationError(err.context("Failure with Transfer Proof"))
            }
        }
    }
}

impl From<encrypted_signature::OutEvent> for OutEvent {
    fn from(event: encrypted_signature::OutEvent) -> Self {
        use encrypted_signature::OutEvent::*;
        match event {
            Acknowledged => OutEvent::EncryptedSignatureAcknowledged,
            Failure(err) => {
                OutEvent::CommunicationError(err.context("Failure with Encrypted Signature"))
            }
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Bob.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pt: peer_tracker::Behaviour,
    quote: quote::Behaviour,
    spot_price: spot_price::Behaviour,
    execution_setup: execution_setup::Behaviour,
    transfer_proof: transfer_proof::Behaviour,
    encrypted_signature: encrypted_signature::Behaviour,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            pt: Default::default(),
            quote: quote::bob(),
            spot_price: spot_price::bob(),
            execution_setup: Default::default(),
            transfer_proof: Default::default(),
            encrypted_signature: Default::default(),
        }
    }
}

impl Behaviour {
    pub fn request_quote(&mut self, alice: PeerId) {
        let _ = self.quote.send_request(&alice, ());
    }

    pub fn request_spot_price(&mut self, alice: PeerId, request: spot_price::Request) {
        let _ = self.spot_price.send_request(&alice, request);
    }

    pub fn start_execution_setup(
        &mut self,
        alice_peer_id: PeerId,
        state0: State0,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
    ) {
        self.execution_setup
            .run(alice_peer_id, state0, bitcoin_wallet);
    }

    pub fn send_encrypted_signature(
        &mut self,
        alice: PeerId,
        tx_redeem_encsig: bitcoin::EncryptedSignature,
    ) {
        let msg = EncryptedSignature { tx_redeem_encsig };
        self.encrypted_signature.send(alice, msg);
        debug!("Encrypted signature sent");
    }

    /// Add a known address for the given peer
    pub fn add_address(&mut self, peer_id: PeerId, address: Multiaddr) {
        self.pt.add_address(peer_id, address)
    }
}
