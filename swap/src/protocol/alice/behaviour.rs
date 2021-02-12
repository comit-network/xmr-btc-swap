use crate::{
    network::{peer_tracker, peer_tracker::PeerTracker},
    protocol::{
        alice::{
            encrypted_signature, execution_setup, quote_response, transfer_proof, QuoteResponse,
            State0, State3, TransferProof,
        },
        bob::{EncryptedSignature, QuoteRequest},
    },
};
use anyhow::{Error, Result};
use libp2p::{request_response::ResponseChannel, NetworkBehaviour, PeerId};
use tracing::{debug, info};

#[derive(Debug)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
    QuoteRequest {
        msg: QuoteRequest,
        channel: ResponseChannel<QuoteResponse>,
        bob_peer_id: PeerId,
    },
    ExecutionSetupDone {
        bob_peer_id: PeerId,
        state3: Box<State3>,
    },
    TransferProofAcknowledged,
    EncryptedSignature {
        msg: Box<EncryptedSignature>,
        channel: ResponseChannel<()>,
    },
    ResponseSent, // Same variant is used for all messages as no processing is done
    Failure(Error),
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

impl From<quote_response::OutEvent> for OutEvent {
    fn from(event: quote_response::OutEvent) -> Self {
        use crate::protocol::alice::quote_response::OutEvent::*;
        match event {
            MsgReceived {
                msg,
                channel,
                bob_peer_id,
            } => OutEvent::QuoteRequest {
                msg,
                channel,
                bob_peer_id,
            },
            ResponseSent => OutEvent::ResponseSent,
            Failure(err) => OutEvent::Failure(err.context("Quote Request/Response failure")),
        }
    }
}

impl From<execution_setup::OutEvent> for OutEvent {
    fn from(event: execution_setup::OutEvent) -> Self {
        use crate::protocol::alice::execution_setup::OutEvent::*;
        match event {
            Done {
                bob_peer_id,
                state3,
            } => OutEvent::ExecutionSetupDone {
                bob_peer_id,
                state3: Box::new(state3),
            },
            Failure(err) => OutEvent::Failure(err),
        }
    }
}

impl From<transfer_proof::OutEvent> for OutEvent {
    fn from(event: transfer_proof::OutEvent) -> Self {
        use crate::protocol::alice::transfer_proof::OutEvent::*;
        match event {
            Acknowledged => OutEvent::TransferProofAcknowledged,
            Failure(err) => OutEvent::Failure(err.context("Failure with Transfer Proof")),
        }
    }
}

impl From<encrypted_signature::OutEvent> for OutEvent {
    fn from(event: encrypted_signature::OutEvent) -> Self {
        use crate::protocol::alice::encrypted_signature::OutEvent::*;
        match event {
            MsgReceived { msg, channel } => OutEvent::EncryptedSignature {
                msg: Box::new(msg),
                channel,
            },
            AckSent => OutEvent::ResponseSent,
            Failure(err) => OutEvent::Failure(err.context("Failure with Encrypted Signature")),
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
#[derive(NetworkBehaviour, Default)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pt: PeerTracker,
    quote_response: quote_response::Behaviour,
    execution_setup: execution_setup::Behaviour,
    transfer_proof: transfer_proof::Behaviour,
    encrypted_signature: encrypted_signature::Behaviour,
}

impl Behaviour {
    pub fn send_quote_response(
        &mut self,
        channel: ResponseChannel<QuoteResponse>,
        quote_response: QuoteResponse,
    ) -> anyhow::Result<()> {
        self.quote_response.send(channel, quote_response)?;
        info!("Sent quote response");
        Ok(())
    }

    pub fn start_execution_setup(&mut self, bob_peer_id: PeerId, state0: State0) {
        self.execution_setup.run(bob_peer_id, state0);
        info!("Start execution setup with {}", bob_peer_id);
    }

    /// Send Transfer Proof to Bob.
    pub fn send_transfer_proof(&mut self, bob: PeerId, msg: TransferProof) {
        self.transfer_proof.send(bob, msg);
        debug!("Sent Transfer Proof");
    }

    pub fn send_encrypted_signature_ack(&mut self, channel: ResponseChannel<()>) -> Result<()> {
        self.encrypted_signature.send_ack(channel)
    }
}
