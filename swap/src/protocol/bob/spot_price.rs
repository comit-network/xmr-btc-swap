use crate::network::cbor_request_response::CborCodec;
use crate::network::spot_price;
use crate::network::spot_price::SpotPriceProtocol;
use crate::protocol::bob::OutEvent;
use libp2p::request_response::{ProtocolSupport, RequestResponseConfig};
use libp2p::PeerId;

const PROTOCOL: &str = spot_price::PROTOCOL;
pub type SpotPriceOutEvent = spot_price::OutEvent;

/// Constructs a new instance of the `spot-price` behaviour to be used by Bob.
///
/// Bob only supports outbound connections, i.e. requesting a spot price for a
/// given amount of BTC in XMR.
pub fn bob() -> spot_price::Behaviour {
    spot_price::Behaviour::new(
        CborCodec::default(),
        vec![(SpotPriceProtocol, ProtocolSupport::Outbound)],
        RequestResponseConfig::default(),
    )
}

impl From<(PeerId, spot_price::Message)> for OutEvent {
    fn from((peer, message): (PeerId, spot_price::Message)) -> Self {
        match message {
            spot_price::Message::Request { .. } => Self::unexpected_request(peer),
            spot_price::Message::Response {
                response,
                request_id,
            } => Self::SpotPriceReceived {
                id: request_id,
                response,
            },
        }
    }
}

crate::impl_from_rr_event!(SpotPriceOutEvent, OutEvent, PROTOCOL);

#[derive(Clone, Debug, thiserror::Error, PartialEq)]
pub enum Error {
    #[error("Seller currently does not accept incoming swap requests, please try again later")]
    NoSwapsAccepted,
    #[error("Seller refused to buy {buy} because the minimum configured buy limit is {min}")]
    AmountBelowMinimum {
        min: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    #[error("Seller refused to buy {buy} because the maximum configured buy limit is {max}")]
    AmountAboveMaximum {
        max: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    #[error("Seller's XMR balance is currently too low to fulfill the swap request to buy {buy}, please try again later")]
    BalanceTooLow { buy: bitcoin::Amount },

    /// To be used for errors that cannot be explained on the CLI side (e.g.
    /// rate update problems on the seller side)
    #[error("Seller encountered a problem, please try again later.")]
    Other,
}

impl From<spot_price::Error> for Error {
    fn from(error: spot_price::Error) -> Self {
        match error {
            spot_price::Error::NoSwapsAccepted => Error::NoSwapsAccepted,
            spot_price::Error::AmountBelowMinimum { min, buy } => {
                Error::AmountBelowMinimum { min, buy }
            }
            spot_price::Error::AmountAboveMaximum { max, buy } => {
                Error::AmountAboveMaximum { max, buy }
            }
            spot_price::Error::BalanceTooLow { buy } => Error::BalanceTooLow { buy },
            spot_price::Error::Other => Error::Other,
        }
    }
}
