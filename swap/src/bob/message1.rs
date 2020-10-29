use libp2p::{
    request_response::{
        handler::RequestProtocol, ProtocolSupport, RequestResponse, RequestResponseConfig,
        RequestResponseEvent, RequestResponseMessage,
    },
    swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters},
    NetworkBehaviour, PeerId,
};
use std::{
    collections::VecDeque,
    task::{Context, Poll},
    time::Duration,
};
use tracing::error;

use crate::network::request_response::{AliceToBob, BobToAlice, Codec, Message1Protocol, TIMEOUT};
use xmr_btc::{alice, bob};

#[derive(Debug)]
pub enum OutEvent {
    Msg(alice::Message1),
}

/// A `NetworkBehaviour` that represents send/recv of message 1.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", poll_method = "poll")]
#[allow(missing_debug_implementations)]
pub struct Message1 {
    rr: RequestResponse<Codec<Message1Protocol>>,
    #[behaviour(ignore)]
    events: VecDeque<OutEvent>,
}

impl Message1 {
    pub fn send(&mut self, alice: PeerId, msg: bob::Message1) {
        let msg = BobToAlice::Message1(msg);
        let _id = self.rr.send_request(&alice, msg);
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<RequestProtocol<Codec<Message1Protocol>>, OutEvent>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }

        Poll::Pending
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
                vec![(Message1Protocol, ProtocolSupport::Full)],
                config,
            ),
            events: Default::default(),
        }
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<BobToAlice, AliceToBob>> for Message1 {
    fn inject_event(&mut self, event: RequestResponseEvent<BobToAlice, AliceToBob>) {
        match event {
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Request { .. },
                ..
            } => panic!("Bob should never get a request from Alice"),
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Response { response, .. },
                ..
            } => {
                if let AliceToBob::Message1(msg) = response {
                    self.events.push_back(OutEvent::Msg(msg));
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
