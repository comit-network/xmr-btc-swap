use crate::network::request_response::CborCodec;
use libp2p::core::ProtocolName;
use libp2p::request_response::{
    ProtocolSupport, RequestResponse, RequestResponseConfig, RequestResponseEvent,
    RequestResponseMessage,
};
use serde::{Deserialize, Serialize};

pub type OutEvent = RequestResponseEvent<Request, ()>;

#[derive(Debug, Clone, Copy, Default)]
pub struct EncryptedSignatureProtocol;

impl ProtocolName for EncryptedSignatureProtocol {
    fn protocol_name(&self) -> &[u8] {
        b"/comit/xmr/btc/encrypted_signature/1.0.0"
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub tx_redeem_encsig: crate::bitcoin::EncryptedSignature,
}

pub type Behaviour = RequestResponse<CborCodec<EncryptedSignatureProtocol, Request, ()>>;

pub type Message = RequestResponseMessage<Request, ()>;

pub fn alice() -> Behaviour {
    Behaviour::new(
        CborCodec::default(),
        vec![(EncryptedSignatureProtocol, ProtocolSupport::Inbound)],
        RequestResponseConfig::default(),
    )
}

pub fn bob() -> Behaviour {
    Behaviour::new(
        CborCodec::default(),
        vec![(EncryptedSignatureProtocol, ProtocolSupport::Outbound)],
        RequestResponseConfig::default(),
    )
}
