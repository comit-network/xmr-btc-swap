use crate::network::quote::BidQuote;
use crate::network::{encrypted_signature, quote, redial, spot_price, transfer_proof};
use crate::protocol::bob::{execution_setup, State2};
use anyhow::{anyhow, Error, Result};
use libp2p::core::Multiaddr;
use libp2p::request_response::{RequestId, ResponseChannel};
use libp2p::{NetworkBehaviour, PeerId};
use std::time::Duration;

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
