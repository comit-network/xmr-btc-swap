use crate::network::request_response::{CborCodec, EncryptedSignatureProtocol, TIMEOUT};
use anyhow::{anyhow, Error};
use libp2p::request_response::{
    ProtocolSupport, RequestResponse, RequestResponseConfig, RequestResponseEvent,
    RequestResponseMessage,
};
use libp2p::{NetworkBehaviour, PeerId};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedSignature {
    pub tx_redeem_encsig: crate::bitcoin::EncryptedSignature,
}

#[derive(Debug)]
pub enum OutEvent {
    Acknowledged,
    Failure(Error),
}

/// A `NetworkBehaviour` that represents sending encrypted signature to Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    rr: RequestResponse<CborCodec<EncryptedSignatureProtocol, EncryptedSignature, ()>>,
}

impl Behaviour {
    pub fn send(&mut self, alice: PeerId, msg: EncryptedSignature) {
        let _id = self.rr.send_request(&alice, msg);
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
                vec![(EncryptedSignatureProtocol, ProtocolSupport::Outbound)],
                config,
            ),
        }
    }
}

impl From<RequestResponseEvent<EncryptedSignature, ()>> for OutEvent {
    fn from(event: RequestResponseEvent<EncryptedSignature, ()>) -> Self {
        match event {
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Request { .. },
                ..
            } => OutEvent::Failure(anyhow!("Bob should never get a request from Alice")),
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
            RequestResponseEvent::ResponseSent { .. } => OutEvent::Failure(anyhow!(
                "Bob does not send the encrypted signature response to Alice"
            )),
        }
    }
}
