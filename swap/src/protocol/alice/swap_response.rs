use crate::{
    monero,
    network::request_response::{CborCodec, Swap, TIMEOUT},
    protocol::bob::SwapRequest,
};
use anyhow::{anyhow, Error, Result};
use libp2p::{
    request_response::{
        ProtocolSupport, RequestResponse, RequestResponseConfig, RequestResponseEvent,
        RequestResponseMessage, ResponseChannel,
    },
    NetworkBehaviour,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

#[derive(Debug)]
pub enum OutEvent {
    MsgReceived {
        msg: SwapRequest,
        channel: ResponseChannel<SwapResponse>,
    },
    ResponseSent,
    Failure(Error),
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct SwapResponse {
    pub xmr_amount: monero::Amount,
}

impl From<RequestResponseEvent<SwapRequest, SwapResponse>> for OutEvent {
    fn from(event: RequestResponseEvent<SwapRequest, SwapResponse>) -> Self {
        match event {
            RequestResponseEvent::Message {
                peer,
                message:
                    RequestResponseMessage::Request {
                        request, channel, ..
                    },
                ..
            } => {
                debug!("Received swap request from {}", peer);
                OutEvent::MsgReceived {
                    msg: request,
                    channel,
                }
            }
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Response { .. },
                ..
            } => OutEvent::Failure(anyhow!("Alice should not get a Response")),
            RequestResponseEvent::InboundFailure { error, .. } => {
                OutEvent::Failure(anyhow!("Inbound failure: {:?}", error))
            }
            RequestResponseEvent::OutboundFailure { error, .. } => {
                OutEvent::Failure(anyhow!("Outbound failure: {:?}", error))
            }
            RequestResponseEvent::ResponseSent { .. } => OutEvent::ResponseSent,
        }
    }
}

/// A `NetworkBehaviour` that represents negotiate a swap using Swap
/// request/response.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    rr: RequestResponse<CborCodec<Swap, SwapRequest, SwapResponse>>,
}

impl Behaviour {
    /// Alice always sends her messages as a response to a request from Bob.
    pub fn send(
        &mut self,
        channel: ResponseChannel<SwapResponse>,
        msg: SwapResponse,
    ) -> Result<()> {
        self.rr
            .send_response(channel, msg)
            .map_err(|_| anyhow!("Sending swap response failed"))
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
        }
    }
}
