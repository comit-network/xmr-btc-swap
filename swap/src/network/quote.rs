use std::time::Duration;

use crate::{asb, bitcoin, cli};
use libp2p::request_response::{self, ProtocolSupport};
use libp2p::{PeerId, StreamProtocol};
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

const PROTOCOL: &str = "/comit/xmr/btc/bid-quote/1.0.0";
pub type OutEvent = request_response::Event<(), BidQuote>;
pub type Message = request_response::Message<(), BidQuote>;

pub type Behaviour = request_response::json::Behaviour<(), BidQuote>;

#[derive(Debug, Clone, Copy, Default)]
pub struct BidQuoteProtocol;

impl AsRef<str> for BidQuoteProtocol {
    fn as_ref(&self) -> &str {
        PROTOCOL
    }
}

/// Represents a quote for buying XMR.
#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[typeshare]
pub struct BidQuote {
    /// The price at which the maker is willing to buy at.
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    #[typeshare(serialized_as = "number")]
    pub price: bitcoin::Amount,
    /// The minimum quantity the maker is willing to buy.
    ///     #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    #[typeshare(serialized_as = "number")]
    pub min_quantity: bitcoin::Amount,
    /// The maximum quantity the maker is willing to buy.
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    #[typeshare(serialized_as = "number")]
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
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Inbound)],
        request_response::Config::default().with_request_timeout(Duration::from_secs(60)),
    )
}

/// Constructs a new instance of the `quote` behaviour to be used by the CLI.
///
/// The CLI is always dialing and only supports outbound connections, i.e.
/// requesting quotes.
pub fn cli() -> Behaviour {
    Behaviour::new(
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Outbound)],
        request_response::Config::default().with_request_timeout(Duration::from_secs(60)),
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
