use std::time::Duration;

use crate::{asb, cli, monero};
use libp2p::request_response::{self, ProtocolSupport};
use libp2p::{PeerId, StreamProtocol};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const PROTOCOL: &str = "/comit/xmr/btc/transfer_proof/1.0.0";
type OutEvent = request_response::Event<Request, ()>;
type Message = request_response::Message<Request, ()>;

pub type Behaviour = request_response::cbor::Behaviour<Request, ()>;

#[derive(Debug, Clone, Copy, Default)]
pub struct TransferProofProtocol;

impl AsRef<str> for TransferProofProtocol {
    fn as_ref(&self) -> &str {
        PROTOCOL
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub swap_id: Uuid,
    pub tx_lock_proof: monero::TransferProof,
}

pub fn alice() -> Behaviour {
    Behaviour::new(
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Outbound)],
        request_response::Config::default().with_request_timeout(Duration::from_secs(60)),
    )
}

pub fn bob() -> Behaviour {
    Behaviour::new(
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Inbound)],
        request_response::Config::default().with_request_timeout(Duration::from_secs(60)),
    )
}

impl From<(PeerId, Message)> for asb::OutEvent {
    fn from((peer, message): (PeerId, Message)) -> Self {
        match message {
            Message::Request { .. } => Self::unexpected_request(peer),
            Message::Response { request_id, .. } => Self::TransferProofAcknowledged {
                peer,
                id: request_id,
            },
        }
    }
}

crate::impl_from_rr_event!(OutEvent, asb::OutEvent, PROTOCOL);

impl From<(PeerId, Message)> for cli::OutEvent {
    fn from((peer, message): (PeerId, Message)) -> Self {
        match message {
            Message::Request {
                request, channel, ..
            } => Self::TransferProofReceived {
                msg: Box::new(request),
                channel,
                peer,
            },
            Message::Response { .. } => Self::unexpected_response(peer),
        }
    }
}
crate::impl_from_rr_event!(OutEvent, cli::OutEvent, PROTOCOL);
