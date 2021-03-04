use crate::monero;
use crate::network::request_response::{CborCodec, TransferProofProtocol, TIMEOUT};
use anyhow::{anyhow, Error};
use libp2p::request_response::{
    ProtocolSupport, RequestResponse, RequestResponseConfig, RequestResponseEvent,
    RequestResponseMessage,
};
use libp2p::{NetworkBehaviour, PeerId};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferProof {
    pub tx_lock_proof: monero::TransferProof,
}

#[derive(Debug)]
pub enum OutEvent {
    Acknowledged,
    Failure(Error),
}

/// A `NetworkBehaviour` that represents sending the Monero transfer proof to
/// Bob.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    rr: RequestResponse<CborCodec<TransferProofProtocol, TransferProof, ()>>,
}

impl Behaviour {
    pub fn send(&mut self, bob: PeerId, msg: TransferProof) {
        let _id = self.rr.send_request(&bob, msg);
    }
}

impl Default for Behaviour {
    fn default() -> Self {
        let timeout = Duration::from_secs(TIMEOUT);
        let mut config = RequestResponseConfig::default();
        config.set_request_timeout(timeout);

        Self {
            rr: RequestResponse::new(
                CborCodec::default(),
                vec![(TransferProofProtocol, ProtocolSupport::Outbound)],
                config,
            ),
        }
    }
}

impl From<RequestResponseEvent<TransferProof, ()>> for OutEvent {
    fn from(event: RequestResponseEvent<TransferProof, ()>) -> Self {
        match event {
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Request { .. },
                ..
            } => OutEvent::Failure(anyhow!(
                "Alice should never get a transfer proof request from Bob"
            )),
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Response { .. },
                ..
            } => OutEvent::Acknowledged,
            RequestResponseEvent::InboundFailure { error, .. } => {
                OutEvent::Failure(anyhow!("Inbound failure: {:?}", error))
            }
            RequestResponseEvent::OutboundFailure { error, .. } => {
                OutEvent::Failure(anyhow!("Outbound failure: {:?}", error))
            }
            RequestResponseEvent::ResponseSent { .. } => {
                OutEvent::Failure(anyhow!("Alice should not send a response"))
            }
        }
    }
}
