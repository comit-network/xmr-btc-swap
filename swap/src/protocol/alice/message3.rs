use crate::{
    network::request_response::{AliceToBob, BobToAlice, Codec, Message3Protocol, TIMEOUT},
    protocol::bob,
};
use libp2p::{
    request_response::{
        handler::RequestProtocol, ProtocolSupport, RequestResponse, RequestResponseConfig,
        RequestResponseEvent, RequestResponseMessage,
    },
    swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters},
    NetworkBehaviour,
};
use std::{
    collections::VecDeque,
    task::{Context, Poll},
    time::Duration,
};
use tracing::{debug, error};

#[derive(Debug)]
pub enum OutEvent {
    Msg(bob::Message3),
}

/// A `NetworkBehaviour` that represents receiving of message 3 from Bob.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", poll_method = "poll")]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    rr: RequestResponse<Codec<Message3Protocol>>,
    #[behaviour(ignore)]
    events: VecDeque<OutEvent>,
}

impl Behaviour {
    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<RequestProtocol<Codec<Message3Protocol>>, OutEvent>> {
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
                Codec::default(),
                vec![(Message3Protocol, ProtocolSupport::Full)],
                config,
            ),
            events: Default::default(),
        }
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<BobToAlice, AliceToBob>> for Behaviour {
    fn inject_event(&mut self, event: RequestResponseEvent<BobToAlice, AliceToBob>) {
        match event {
            RequestResponseEvent::Message {
                message:
                    RequestResponseMessage::Request {
                        request, channel, ..
                    },
                ..
            } => {
                if let BobToAlice::Message3(msg) = request {
                    debug!("Received Message3");
                    self.events.push_back(OutEvent::Msg(msg));
                    // Send back empty response so that the request/response protocol completes.
                    if let Err(error) = self.rr.send_response(channel, AliceToBob::Message3) {
                        error!("Send Message3 response failure: {:?}", error);
                    }
                }
            }
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Response { .. },
                ..
            } => panic!("Alice should not get a Response"),
            RequestResponseEvent::InboundFailure { error, .. } => {
                error!("Inbound failure: {:?}", error);
            }
            RequestResponseEvent::OutboundFailure { error, .. } => {
                error!("Outbound failure: {:?}", error);
            }
            RequestResponseEvent::ResponseSent { .. } => {
                debug!("Alice has sent an Message3 response to Bob");
            }
        }
    }
}
