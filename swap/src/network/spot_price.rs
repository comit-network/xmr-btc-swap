use crate::monero;
use crate::network::cbor_request_response::CborCodec;
use crate::protocol::bob;
use libp2p::core::ProtocolName;
use libp2p::request_response::{
    ProtocolSupport, RequestResponse, RequestResponseConfig, RequestResponseEvent,
    RequestResponseMessage,
};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};

const PROTOCOL: &str = "/comit/xmr/btc/spot-price/1.0.0";
pub type OutEvent = RequestResponseEvent<Request, Response>;
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
pub enum Response {
    Xmr(monero::Amount),
    Error(Error),
}

#[derive(Clone, Debug, thiserror::Error, Serialize, Deserialize)]
pub enum Error {
    #[error(
        "This seller currently does not accept incoming swap requests, please try again later"
    )]
    NoSwapsAccepted,
    #[error("Seller refused to buy {buy} because the maximum configured buy limit is {max}")]
    MaxBuyAmountExceeded {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        max: bitcoin::Amount,
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },
    #[error("This seller's XMR balance is currently too low to fulfill the swap request to buy {buy}, please try again later")]
    BalanceTooLow {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },

    /// To be used for errors that cannot be explained on the CLI side (e.g.
    /// rate update problems on the seller side)
    #[error("The seller encountered a problem, please try again later.")]
    Other,
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
