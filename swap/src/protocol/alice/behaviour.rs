use crate::monero;
use crate::network::quote::BidQuote;
use crate::network::{encrypted_signature, quote, transfer_proof};
use crate::protocol::alice::event_loop::LatestRate;
use crate::protocol::alice::{execution_setup, spot_price, State3};
use anyhow::{anyhow, Error};
use libp2p::request_response::{RequestId, ResponseChannel};
use libp2p::{NetworkBehaviour, PeerId};
use std::fmt::Debug;
use uuid::Uuid;

#[derive(Debug)]
pub enum OutEvent {
    ExecutionSetupStart {
        peer: PeerId,
        btc: bitcoin::Amount,
        xmr: monero::Amount,
    },
    QuoteRequested {
        channel: ResponseChannel<BidQuote>,
        peer: PeerId,
    },
    ExecutionSetupDone {
        bob_peer_id: PeerId,
        swap_id: Uuid,
        state3: Box<State3>,
    },
    TransferProofAcknowledged {
        peer: PeerId,
        id: RequestId,
    },
    EncryptedSignatureReceived {
        msg: Box<encrypted_signature::Request>,
        channel: ResponseChannel<()>,
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

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour<LR>
where
    LR: LatestRate + Send + 'static + Debug,
{
    pub quote: quote::Behaviour,
    pub spot_price: spot_price::Behaviour<LR>,
    pub execution_setup: execution_setup::Behaviour,
    pub transfer_proof: transfer_proof::Behaviour,
    pub encrypted_signature: encrypted_signature::Behaviour,
}

impl<LR> Behaviour<LR>
where
    LR: LatestRate + Send + 'static + Debug,
{
    pub fn new(
        balance: monero::Amount,
        lock_fee: monero::Amount,
        max_buy: bitcoin::Amount,
        latest_rate: LR,
        resume_only: bool,
    ) -> Self {
        Self {
            quote: quote::alice(),
            spot_price: spot_price::Behaviour::new(
                balance,
                lock_fee,
                max_buy,
                latest_rate,
                resume_only,
            ),
            execution_setup: Default::default(),
            transfer_proof: transfer_proof::alice(),
            encrypted_signature: encrypted_signature::alice(),
        }
    }
}
