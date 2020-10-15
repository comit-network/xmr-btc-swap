use anyhow::Result;
use libp2p::{
    request_response::{
        handler::RequestProtocol, ProtocolSupport, RequestId, RequestResponse,
        RequestResponseConfig, RequestResponseEvent, RequestResponseMessage, ResponseChannel,
    },
    swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters},
    NetworkBehaviour, PeerId,
};
use std::{
    collections::VecDeque,
    task::{Context, Poll},
    time::Duration,
};
use tracing::{debug, error};

use crate::{
    bitcoin,
    network::request_response::{AliceToBob, BobToAlice, Codec, Protocol},
    Never,
};

#[derive(Debug)]
pub enum BehaviourOutEvent {
    Btc {
        btc: bitcoin::Amount,
        channel: ResponseChannel<AliceToBob>,
    },
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Bob.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "BehaviourOutEvent", poll_method = "poll")]
#[allow(missing_debug_implementations)]
pub struct Messenger {
    rr: RequestResponse<Codec>,
    #[behaviour(ignore)]
    events: VecDeque<BehaviourOutEvent>,
}

impl Messenger {
    pub fn new(timeout: Duration) -> Self {
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

    /// Alice always sends her messages as a response to a request from Bob.
    pub fn send(&mut self, channel: ResponseChannel<AliceToBob>, msg: AliceToBob) {
        self.rr.send_response(channel, msg);
    }

    pub async fn request_amounts(
        &mut self,
        alice: PeerId,
        btc: bitcoin::Amount,
    ) -> Result<RequestId> {
        let msg = BobToAlice::AmountsFromBtc(btc);
        let id = self.rr.send_request(&alice, msg);
        debug!("Request sent to: {}", alice);

        Ok(id)
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<RequestProtocol<Codec>, BehaviourOutEvent>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }

        Poll::Pending
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<BobToAlice, AliceToBob>> for Messenger {
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
                BobToAlice::AmountsFromBtc(btc) => self
                    .events
                    .push_back(BehaviourOutEvent::Btc { btc, channel }),
                _ => panic!("unexpected request"),
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

impl libp2p::swarm::NetworkBehaviourEventProcess<()> for Messenger {
    fn inject_event(&mut self, _event: ()) {}
}

impl libp2p::swarm::NetworkBehaviourEventProcess<Never> for Messenger {
    fn inject_event(&mut self, _: Never) {}
}
