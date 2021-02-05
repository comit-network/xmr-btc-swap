use crate::{
    monero,
    network::request_response::{CborCodec, Request, Response, Swap, TIMEOUT},
    protocol::bob,
};
use anyhow::{anyhow, Result};
use libp2p::{
    request_response::{
        handler::RequestProtocol, ProtocolSupport, RequestResponse, RequestResponseConfig,
        RequestResponseEvent, RequestResponseMessage, ResponseChannel,
    },
    swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters},
    NetworkBehaviour,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    task::{Context, Poll},
    time::Duration,
};
use tracing::{debug, error};

#[derive(Debug)]
pub struct OutEvent {
    pub msg: bob::SwapRequest,
    pub channel: ResponseChannel<Response>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct SwapResponse {
    pub xmr_amount: monero::Amount,
}

/// A `NetworkBehaviour` that represents negotiate a swap using Swap
/// request/response.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", poll_method = "poll")]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    rr: RequestResponse<CborCodec<Swap>>,
    #[behaviour(ignore)]
    events: VecDeque<OutEvent>,
}

impl Behaviour {
    /// Alice always sends her messages as a response to a request from Bob.
    pub fn send(&mut self, channel: ResponseChannel<Response>, msg: SwapResponse) -> Result<()> {
        let msg = Response::SwapResponse(Box::new(msg));
        self.rr
            .send_response(channel, msg)
            .map_err(|_| anyhow!("Sending swap response failed"))
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<RequestProtocol<CborCodec<Swap>>, OutEvent>> {
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
                vec![(Swap, ProtocolSupport::Inbound)],
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
                message:
                    RequestResponseMessage::Request {
                        request, channel, ..
                    },
                ..
            } => {
                if let Request::SwapRequest(msg) = request {
                    debug!("Received swap request");
                    self.events.push_back(OutEvent { msg: *msg, channel })
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
                debug!("Alice has sent a swap response to Bob");
            }
        }
    }
}
