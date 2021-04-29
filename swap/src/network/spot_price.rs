use crate::network::cbor_request_response::CborCodec;
use crate::protocol::{alice, bob};
use crate::{bitcoin, monero};
use libp2p::core::ProtocolName;
use libp2p::request_response::{
    ProtocolSupport, RequestResponse, RequestResponseConfig, RequestResponseEvent,
    RequestResponseMessage,
};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};

const PROTOCOL: &str = "/comit/xmr/btc/spot-price/1.0.0";
type OutEvent = RequestResponseEvent<Request, Response>;
type Message = RequestResponseMessage<Request, Response>;

pub type Behaviour = RequestResponse<CborCodec<SpotPriceProtocol, Request, Response>>;

/// The spot price protocol allows parties to **initiate** a trade by requesting
/// a spot price.
///
/// A spot price is binding for both parties, i.e. after the spot-price protocol
/// completes, both parties are expected to follow up with the `execution-setup`
/// protocol.
///
/// If a party wishes to only inquire about the current price, they should use
/// the `quote` protocol instead.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpotPriceProtocol;

impl ProtocolName for SpotPriceProtocol {
    fn protocol_name(&self) -> &[u8] {
        PROTOCOL.as_bytes()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Request {
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc: bitcoin::Amount,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Response {
    pub xmr: Option<monero::Amount>,
    pub error: Option<Error>,
}

#[derive(Clone, Debug, thiserror::Error, Serialize, Deserialize)]
pub enum Error {
    #[error(
        "This seller currently does not accept incoming swap requests, please try again later"
    )]
    MaintenanceMode,
}

/// Constructs a new instance of the `spot-price` behaviour to be used by Alice.
///
/// Alice only supports inbound connections, i.e. providing spot prices for BTC
/// in XMR.
pub fn alice() -> Behaviour {
    Behaviour::new(
        CborCodec::default(),
        vec![(SpotPriceProtocol, ProtocolSupport::Inbound)],
        RequestResponseConfig::default(),
    )
}

/// Constructs a new instance of the `spot-price` behaviour to be used by Bob.
///
/// Bob only supports outbound connections, i.e. requesting a spot price for a
/// given amount of BTC in XMR.
pub fn bob() -> Behaviour {
    Behaviour::new(
        CborCodec::default(),
        vec![(SpotPriceProtocol, ProtocolSupport::Outbound)],
        RequestResponseConfig::default(),
    )
}

impl From<(PeerId, Message)> for alice::OutEvent {
    fn from((peer, message): (PeerId, Message)) -> Self {
        match message {
            Message::Request {
                request, channel, ..
            } => Self::SpotPriceRequested {
                request,
                channel,
                peer,
            },
            Message::Response { .. } => Self::unexpected_response(peer),
        }
    }
}
crate::impl_from_rr_event!(OutEvent, alice::OutEvent, PROTOCOL);

impl From<(PeerId, Message)> for bob::OutEvent {
    fn from((peer, message): (PeerId, Message)) -> Self {
        match message {
            Message::Request { .. } => Self::unexpected_request(peer),
            Message::Response {
                response,
                request_id,
            } => Self::SpotPriceReceived {
                id: request_id,
                response,
            },
        }
    }
}
crate::impl_from_rr_event!(OutEvent, bob::OutEvent, PROTOCOL);
