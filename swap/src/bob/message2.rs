use libp2p::{
    request_response::{
        handler::RequestProtocol, ProtocolSupport, RequestResponse, RequestResponseConfig,
        RequestResponseEvent, RequestResponseMessage,
    },
    swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters},
    NetworkBehaviour, PeerId,
};
use std::{
    task::{Context, Poll},
    time::Duration,
};
use tracing::{debug, error};

use crate::{
    network::request_response::{AliceToBob, BobToAlice, Codec, Protocol, TIMEOUT},
    Never,
};
use xmr_btc::bob;

/// A `NetworkBehaviour` that represents sending message 2 to Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Never", poll_method = "poll")]
#[allow(missing_debug_implementations)]
pub struct Message2 {
    rr: RequestResponse<Codec>,
}

impl Message2 {
    pub fn send(&mut self, alice: PeerId, msg: bob::Message2) {
        let msg = BobToAlice::Message2(msg);
        let _id = self.rr.send_request(&alice, msg);
    }

    // TODO: Do we need a custom implementation if we are not bubbling any out
    // events?
    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<RequestProtocol<Codec>, Never>> {
        Poll::Pending
    }
}

impl Default for Message2 {
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
        }
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<BobToAlice, AliceToBob>> for Message2 {
    fn inject_event(&mut self, event: RequestResponseEvent<BobToAlice, AliceToBob>) {
        match event {
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Request { .. },
                ..
            } => panic!("Bob should never get a request from Alice"),
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Response { response, .. },
                ..
            } => match response {
                AliceToBob::EmptyResponse => debug!("Alice correctly responded to message 2"),
                other => debug!("unexpected response: {:?}", other),
            },
            RequestResponseEvent::InboundFailure { error, .. } => {
                error!("Inbound failure: {:?}", error);
            }
            RequestResponseEvent::OutboundFailure { error, .. } => {
                error!("Outbound failure: {:?}", error);
            }
        }
    }
}
