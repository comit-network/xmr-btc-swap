use libp2p::{
    request_response::{
        handler::RequestProtocol, ProtocolSupport, RequestResponse, RequestResponseConfig,
        RequestResponseEvent, RequestResponseMessage, ResponseChannel,
    },
    swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters},
    NetworkBehaviour,
};
use std::{
    collections::VecDeque,
    task::{Context, Poll},
    time::Duration,
};
use tracing::error;

use crate::network::request_response::{AliceToBob, BobToAlice, Codec, Protocol, TIMEOUT};
use xmr_btc::bob;

#[derive(Debug)]
pub enum OutEvent {
    Msg {
        /// Received message from Bob.
        msg: bob::Message1,
        /// Channel to send back Alice's message 1.
        channel: ResponseChannel<AliceToBob>,
    },
}

/// A `NetworkBehaviour` that represents send/recv of message 1.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", poll_method = "poll")]
#[allow(missing_debug_implementations)]
pub struct Message1 {
    rr: RequestResponse<Codec>,
    #[behaviour(ignore)]
    events: VecDeque<OutEvent>,
}

impl Message1 {
    pub fn send(&mut self, channel: ResponseChannel<AliceToBob>, msg: xmr_btc::alice::Message1) {
        let msg = AliceToBob::Message1(msg);
        self.rr.send_response(channel, msg);
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<RequestProtocol<Codec>, OutEvent>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }

        Poll::Pending
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<BobToAlice, AliceToBob>> for Message1 {
    fn inject_event(&mut self, event: RequestResponseEvent<BobToAlice, AliceToBob>) {
        match event {
            RequestResponseEvent::Message {
                peer: _,
                message:
                    RequestResponseMessage::Request {
                        request,
                        request_id: _,
                        channel,
                    },
            } => match request {
                BobToAlice::Message1(msg) => {
                    self.events.push_back(OutEvent::Msg { msg, channel });
                }
                other => panic!("unexpected request: {:?}", other),
            },
            RequestResponseEvent::Message {
                peer: _,
                message:
                    RequestResponseMessage::Response {
                        response: _,
                        request_id: _,
                    },
            } => panic!("unexpected response"),
            RequestResponseEvent::InboundFailure {
                peer: _,
                request_id: _,
                error,
            } => {
                error!("Inbound failure: {:?}", error);
            }
            RequestResponseEvent::OutboundFailure {
                peer: _,
                request_id: _,
                error,
            } => {
                error!("Outbound failure: {:?}", error);
            }
        }
    }
}

impl Default for Message1 {
    fn default() -> Self {
        let timeout = Duration::from_secs(TIMEOUT);
        let mut config = RequestResponseConfig::default();
        config.set_request_timeout(timeout);

        Self {
            rr: RequestResponse::new(
                Codec::default(),
                vec![(Protocol, ProtocolSupport::Full)],
                config,
            ),
            events: Default::default(),
        }
    }
}
