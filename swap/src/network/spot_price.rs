use crate::network::request_response::CborCodec;
use crate::{bitcoin, monero};
use libp2p::core::ProtocolName;
use libp2p::request_response::{
    ProtocolSupport, RequestResponse, RequestResponseConfig, RequestResponseEvent,
};
use serde::{Deserialize, Serialize};

pub type OutEvent = RequestResponseEvent<Request, Response>;

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
        b"/comit/xmr/btc/spot-price/1.0.0"
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Request {
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc: bitcoin::Amount,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Response {
    pub xmr: monero::Amount,
}

pub type Behaviour = RequestResponse<CborCodec<SpotPriceProtocol, Request, Response>>;

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
