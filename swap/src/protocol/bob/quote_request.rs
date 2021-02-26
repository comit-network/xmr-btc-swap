use crate::{
    network::request_response::{CborCodec, Swap, TIMEOUT},
    protocol::alice::QuoteResponse,
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
pub struct QuoteRequest {
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc_amount: bitcoin::Amount,
}

#[derive(Debug)]
pub enum OutEvent {
    MsgReceived(QuoteResponse),
    Failure(Error),
}

/// A `NetworkBehaviour` that represents doing the negotiation of a swap.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    rr: RequestResponse<CborCodec<Swap, QuoteRequest, QuoteResponse>>,
}

impl Behaviour {
    pub fn send(&mut self, alice: PeerId, quote_request: QuoteRequest) -> Result<RequestId> {
        debug!(
            "Requesting quote for {} from {}",
            quote_request.btc_amount, alice
        );

        let id = self.rr.send_request(&alice, quote_request);

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

impl From<RequestResponseEvent<QuoteRequest, QuoteResponse>> for OutEvent {
    fn from(event: RequestResponseEvent<QuoteRequest, QuoteResponse>) -> Self {
        match event {
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Request { .. },
                ..
            } => OutEvent::Failure(anyhow!("Bob should never get a request from Alice")),
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Response { response, .. },
                ..
            } => OutEvent::MsgReceived(response),
            RequestResponseEvent::InboundFailure { error, .. } => {
                OutEvent::Failure(anyhow!("Inbound failure: {:?}", error))
            }
            RequestResponseEvent::OutboundFailure { error, .. } => {
                OutEvent::Failure(anyhow!("Outbound failure: {:?}", error))
            }
            RequestResponseEvent::ResponseSent { .. } => {
                OutEvent::Failure(anyhow!("Bob does not send a quote response to Alice"))
            }
        }
    }
}
