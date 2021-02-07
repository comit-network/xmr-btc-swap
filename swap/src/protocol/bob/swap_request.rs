use crate::{
    network::request_response::{CborCodec, Swap, TIMEOUT},
    protocol::alice::SwapResponse,
};
use anyhow::{anyhow, Error, Result};
use libp2p::{
    request_response::{
        ProtocolSupport, RequestId, RequestResponse, RequestResponseConfig, RequestResponseEvent,
        RequestResponseMessage,
    },
    NetworkBehaviour, PeerId,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SwapRequest {
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc_amount: bitcoin::Amount,
}

#[derive(Debug)]
pub enum OutEvent {
    MsgReceived(SwapResponse),
    Failure(Error),
}

/// A `NetworkBehaviour` that represents doing the negotiation of a swap.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    rr: RequestResponse<CborCodec<Swap, SwapRequest, SwapResponse>>,
}

impl Behaviour {
    pub fn send(&mut self, alice: PeerId, swap_request: SwapRequest) -> Result<RequestId> {
        let id = self.rr.send_request(&alice, swap_request);

        Ok(id)
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
                vec![(Swap, ProtocolSupport::Outbound)],
                config,
            ),
        }
    }
}

impl From<RequestResponseEvent<SwapRequest, SwapResponse>> for OutEvent {
    fn from(event: RequestResponseEvent<SwapRequest, SwapResponse>) -> Self {
        match event {
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Request { .. },
                ..
            } => OutEvent::Failure(anyhow!("Bob should never get a request from Alice")),
            RequestResponseEvent::Message {
                peer,
                message: RequestResponseMessage::Response { response, .. },
                ..
            } => {
                debug!("Received swap response from {}", peer);
                OutEvent::MsgReceived(response)
            }
            RequestResponseEvent::InboundFailure { error, .. } => {
                OutEvent::Failure(anyhow!("Inbound failure: {:?}", error))
            }
            RequestResponseEvent::OutboundFailure { error, .. } => {
                OutEvent::Failure(anyhow!("Outbound failure: {:?}", error))
            }
            RequestResponseEvent::ResponseSent { .. } => {
                OutEvent::Failure(anyhow!("Bob does not send a swap response to Alice"))
            }
        }
    }
}
