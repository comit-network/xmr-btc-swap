use crate::{
    monero,
    network::request_response::{Message4Protocol, OneShotCodec, Request, Response, TIMEOUT},
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
pub struct Message4 {
    pub tx_lock_proof: monero::TransferProof,
}

#[derive(Debug, Copy, Clone)]
pub enum OutEvent {
    Msg,
}

/// A `NetworkBehaviour` that represents sending message 4 to Bob.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", poll_method = "poll")]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    rr: RequestResponse<OneShotCodec<Message4Protocol>>,
    #[behaviour(ignore)]
    events: VecDeque<OutEvent>,
}

impl Behaviour {
    pub fn send(&mut self, bob: PeerId, msg: Message4) {
        let msg = Request::Message4(Box::new(msg));
        let _id = self.rr.send_request(&bob, msg);
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<RequestProtocol<OneShotCodec<Message4Protocol>>, OutEvent>>
    {
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
                vec![(Message4Protocol, ProtocolSupport::Full)],
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
            } => panic!("Alice should never get a message 4 request from Bob"),
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Response { response, .. },
                ..
            } => {
                if let Response::Message4 = response {
                    self.events.push_back(OutEvent::Msg);
                }
            }
            RequestResponseEvent::InboundFailure { error, .. } => {
                error!("Inbound failure: {:?}", error);
            }
            RequestResponseEvent::OutboundFailure { error, .. } => {
                error!("Outbound failure: {:?}", error);
            }
        }
    }
}
