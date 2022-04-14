use crate::network::json_pull_codec::JsonPullCodec;
use crate::{asb, bitcoin, cli};
use libp2p::core::ProtocolName;
use libp2p::request_response::{
    ProtocolSupport, RequestResponse, RequestResponseConfig, RequestResponseEvent,
    RequestResponseMessage,
};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};

const PROTOCOL: &str = "/comit/xmr/btc/bid-quote/1.0.0";
pub type OutEvent = RequestResponseEvent<(), BidQuote>;
pub type Message = RequestResponseMessage<(), BidQuote>;

pub type Behaviour = RequestResponse<JsonPullCodec<BidQuoteProtocol, BidQuote>>;

#[derive(Debug, Clone, Copy, Default)]
pub struct BidQuoteProtocol;

impl ProtocolName for BidQuoteProtocol {
    fn protocol_name(&self) -> &[u8] {
        PROTOCOL.as_bytes()
    }
}

/// Represents a quote for buying XMR.
#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct BidQuote {
    /// The price at which the maker is willing to buy at.
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub price: bitcoin::Amount,
    /// The minimum quantity the maker is willing to buy.
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub min_quantity: bitcoin::Amount,
    /// The maximum quantity the maker is willing to buy.
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub max_quantity: bitcoin::Amount,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("Received quote of 0")]
pub struct ZeroQuoteReceived;

/// Constructs a new instance of the `quote` behaviour to be used by the ASB.
///
/// The ASB is always listening and only supports inbound connections, i.e.
/// handing out quotes.
pub fn asb() -> Behaviour {
    Behaviour::new(
        JsonPullCodec::default(),
        vec![(BidQuoteProtocol, ProtocolSupport::Inbound)],
        RequestResponseConfig::default(),
    )
}

/// Constructs a new instance of the `quote` behaviour to be used by the CLI.
///
/// The CLI is always dialing and only supports outbound connections, i.e.
/// requesting quotes.
pub fn cli() -> Behaviour {
    Behaviour::new(
        JsonPullCodec::default(),
        vec![(BidQuoteProtocol, ProtocolSupport::Outbound)],
        RequestResponseConfig::default(),
    )
}

impl From<(PeerId, Message)> for asb::OutEvent {
    fn from((peer, message): (PeerId, Message)) -> Self {
        match message {
            Message::Request { channel, .. } => Self::QuoteRequested { channel, peer },
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
            } => Self::QuoteReceived {
                id: request_id,
                response,
            },
        }
    }
}
crate::impl_from_rr_event!(OutEvent, cli::OutEvent, PROTOCOL);
