use crate::bitcoin;
use crate::network::request_response::CborCodec;
use libp2p::core::ProtocolName;
use libp2p::request_response::{
    ProtocolSupport, RequestResponse, RequestResponseConfig, RequestResponseEvent,
};
use serde::{Deserialize, Serialize};

pub type OutEvent = RequestResponseEvent<(), BidQuote>;

#[derive(Debug, Clone, Copy, Default)]
pub struct BidQuoteProtocol;

impl ProtocolName for BidQuoteProtocol {
    fn protocol_name(&self) -> &[u8] {
        b"/comit/xmr/btc/bid-quote/1.0.0"
    }
}

/// Represents a quote for buying XMR.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BidQuote {
    /// The price at which the maker is willing to buy at.
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub price: bitcoin::Amount,
    /// The maximum quantity the maker is willing to buy.
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub max_quantity: bitcoin::Amount,
}

pub type Behaviour = RequestResponse<CborCodec<BidQuoteProtocol, (), BidQuote>>;

/// Constructs a new instance of the `quote` behaviour to be used by Alice.
///
/// Alice only supports inbound connections, i.e. handing out quotes.
pub fn alice() -> Behaviour {
    Behaviour::new(
        CborCodec::default(),
        vec![(BidQuoteProtocol, ProtocolSupport::Inbound)],
        RequestResponseConfig::default(),
    )
}

/// Constructs a new instance of the `quote` behaviour to be used by Bob.
///
/// Bob only supports outbound connections, i.e. requesting quotes.
pub fn bob() -> Behaviour {
    Behaviour::new(
        CborCodec::default(),
        vec![(BidQuoteProtocol, ProtocolSupport::Outbound)],
        RequestResponseConfig::default(),
    )
}
