use crate::{
    network::request_response::{CborCodec, TransferProofProtocol, TIMEOUT},
    protocol::alice::TransferProof,
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
    Msg(TransferProof),
}

/// A `NetworkBehaviour` that represents receiving the transfer proof from
/// Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", poll_method = "poll")]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    rr: RequestResponse<CborCodec<TransferProofProtocol, TransferProof, ()>>,
    #[behaviour(ignore)]
    events: VecDeque<OutEvent>,
}

impl Behaviour {
    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<
        NetworkBehaviourAction<
            RequestProtocol<CborCodec<TransferProofProtocol, TransferProof, ()>>,
            OutEvent,
        >,
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
                CborCodec::default(),
                vec![(TransferProofProtocol, ProtocolSupport::Inbound)],
                config,
            ),
            events: Default::default(),
        }
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<TransferProof, ()>> for Behaviour {
    fn inject_event(&mut self, event: RequestResponseEvent<TransferProof, ()>) {
        match event {
            RequestResponseEvent::Message {
                message:
                    RequestResponseMessage::Request {
                        request, channel, ..
                    },
                ..
            } => {
                debug!("Received Transfer Proof");
                self.events.push_back(OutEvent::Msg(request));
                // Send back empty response so that the request/response protocol completes.
                let _ = self
                    .rr
                    .send_response(channel, ())
                    .map_err(|err| error!("Failed to send message 3: {:?}", err));
            }
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Response { .. },
                ..
            } => panic!("Bob should not get a Response"),
            RequestResponseEvent::InboundFailure { error, .. } => {
                error!("Inbound failure: {:?}", error);
            }
            RequestResponseEvent::OutboundFailure { error, .. } => {
                error!("Outbound failure: {:?}", error);
            }
            RequestResponseEvent::ResponseSent { .. } => debug!("Bob ack'd transfer proof message"),
        }
    }
}
