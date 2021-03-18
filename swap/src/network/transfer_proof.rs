use crate::monero;
use crate::network::request_response::CborCodec;
use libp2p::core::ProtocolName;
use libp2p::request_response::{
    ProtocolSupport, RequestResponse, RequestResponseConfig, RequestResponseEvent,
    RequestResponseMessage,
};
use serde::{Deserialize, Serialize};

pub type OutEvent = RequestResponseEvent<Request, ()>;

#[derive(Debug, Clone, Copy, Default)]
pub struct TransferProofProtocol;

impl ProtocolName for TransferProofProtocol {
    fn protocol_name(&self) -> &[u8] {
        b"/comit/xmr/btc/transfer_proof/1.0.0"
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub tx_lock_proof: monero::TransferProof,
}

pub type Behaviour = RequestResponse<CborCodec<TransferProofProtocol, Request, ()>>;

pub type Message = RequestResponseMessage<Request, ()>;

pub fn alice() -> Behaviour {
    Behaviour::new(
        CborCodec::default(),
        vec![(TransferProofProtocol, ProtocolSupport::Outbound)],
        RequestResponseConfig::default(),
    )
}

pub fn bob() -> Behaviour {
    Behaviour::new(
        CborCodec::default(),
        vec![(TransferProofProtocol, ProtocolSupport::Inbound)],
        RequestResponseConfig::default(),
    )
}
