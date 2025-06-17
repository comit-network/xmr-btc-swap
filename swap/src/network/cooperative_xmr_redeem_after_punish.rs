use crate::monero::{Scalar, TransferProof};
use crate::{asb, cli};
use libp2p::request_response::ProtocolSupport;
use libp2p::{request_response, PeerId, StreamProtocol};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

const PROTOCOL: &str = "/comit/xmr/btc/cooperative_xmr_redeem_after_punish/1.0.0";
type OutEvent = request_response::Event<Request, Response>;
type Message = request_response::Message<Request, Response>;

pub type Behaviour = request_response::cbor::Behaviour<Request, Response>;

#[derive(Debug, Clone, Copy, Default)]
pub struct CooperativeXmrRedeemProtocol;

impl AsRef<str> for CooperativeXmrRedeemProtocol {
    fn as_ref(&self) -> &str {
        PROTOCOL
    }
}

#[derive(Debug, thiserror::Error, Clone, Serialize, Deserialize)]
pub enum CooperativeXmrRedeemRejectReason {
    #[error("Alice does not have a record of the swap")]
    UnknownSwap,
    #[error("Alice rejected the request because it deemed it malicious")]
    MaliciousRequest,
    #[error("Alice is in a state where a cooperative redeem is not possible")]
    SwapInvalidState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub swap_id: Uuid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Response {
    Fullfilled {
        swap_id: Uuid,
        s_a: Scalar,
        lock_transfer_proof: TransferProof,
    },
    Rejected {
        swap_id: Uuid,
        reason: CooperativeXmrRedeemRejectReason,
    },
}

pub fn alice() -> Behaviour {
    Behaviour::new(
        vec![(
            StreamProtocol::new(CooperativeXmrRedeemProtocol.as_ref()),
            ProtocolSupport::Inbound,
        )],
        request_response::Config::default().with_request_timeout(Duration::from_secs(60)),
    )
}

pub fn bob() -> Behaviour {
    Behaviour::new(
        vec![(
            StreamProtocol::new(CooperativeXmrRedeemProtocol.as_ref()),
            ProtocolSupport::Outbound,
        )],
        request_response::Config::default().with_request_timeout(Duration::from_secs(60)),
    )
}

impl From<(PeerId, Message)> for asb::OutEvent {
    fn from((peer, message): (PeerId, Message)) -> Self {
        match message {
            Message::Request {
                request, channel, ..
            } => Self::CooperativeXmrRedeemRequested {
                swap_id: request.swap_id,
                channel,
                peer,
            },
            Message::Response { .. } => Self::unexpected_response(peer),
        }
    }
}

crate::impl_from_rr_event!(OutEvent, asb::OutEvent, PROTOCOL);

impl From<(PeerId, Message)> for cli::OutEvent {
    fn from((peer, message): (PeerId, Message)) -> Self {
        match message {
            Message::Request { .. } => Self::unexpected_request(peer),
            Message::Response {
                response,
                request_id,
            } => match response {
                Response::Fullfilled {
                    swap_id,
                    s_a,
                    lock_transfer_proof,
                } => Self::CooperativeXmrRedeemFulfilled {
                    id: request_id,
                    swap_id,
                    s_a,
                    lock_transfer_proof,
                },
                Response::Rejected {
                    swap_id,
                    reason: error,
                } => Self::CooperativeXmrRedeemRejected {
                    id: request_id,
                    swap_id,
                    reason: error,
                },
            },
        }
    }
}

crate::impl_from_rr_event!(OutEvent, cli::OutEvent, PROTOCOL);
