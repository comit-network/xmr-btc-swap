use crate::network::request_response::{
    EncryptedSignatureProtocol, OneShotCodec, Request, Response, TIMEOUT,
};
use libp2p::{
    request_response::{
        handler::RequestProtocol, ProtocolSupport, RequestResponse, RequestResponseConfig,
        RequestResponseEvent, RequestResponseMessage,
    },
    swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters},
    NetworkBehaviour, PeerId,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    task::{Context, Poll},
    time::Duration,
};
use tracing::error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedSignature {
    pub tx_redeem_encsig: crate::bitcoin::EncryptedSignature,
}

#[derive(Debug, Copy, Clone)]
pub enum OutEvent {
    Acknowledged,
}

/// A `NetworkBehaviour` that represents sending encrypted signature to Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", poll_method = "poll")]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    rr: RequestResponse<OneShotCodec<EncryptedSignatureProtocol>>,
    #[behaviour(ignore)]
    events: VecDeque<OutEvent>,
}

impl Behaviour {
    pub fn send(&mut self, alice: PeerId, msg: EncryptedSignature) {
        let msg = Request::EncryptedSignature(Box::new(msg));
        let _id = self.rr.send_request(&alice, msg);
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<
        NetworkBehaviourAction<RequestProtocol<OneShotCodec<EncryptedSignatureProtocol>>, OutEvent>,
    > {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }

        Poll::Pending
    }
}

impl Default for Behaviour {
    fn default() -> Self {
        let timeout = Duration::from_secs(TIMEOUT);
        let mut config = RequestResponseConfig::default();
        config.set_request_timeout(timeout);

        Self {
            rr: RequestResponse::new(
                OneShotCodec::default(),
                vec![(EncryptedSignatureProtocol, ProtocolSupport::Outbound)],
                config,
            ),
            events: Default::default(),
        }
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<Request, Response>> for Behaviour {
    fn inject_event(&mut self, event: RequestResponseEvent<Request, Response>) {
        match event {
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Request { .. },
                ..
            } => panic!("Bob should never get a request from Alice"),
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Response { response, .. },
                ..
            } => {
                if let Response::EncryptedSignature = response {
                    self.events.push_back(OutEvent::Acknowledged);
                }
            }
            RequestResponseEvent::InboundFailure { error, .. } => {
                error!("Inbound failure: {:?}", error);
            }
            RequestResponseEvent::OutboundFailure { error, .. } => {
                error!("Outbound failure: {:?}", error);
            }
            RequestResponseEvent::ResponseSent { .. } => {
                unreachable!("Bob should never send a Amounts response to Alice");
            }
        }
    }
}
