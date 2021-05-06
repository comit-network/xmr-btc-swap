use crate::monero;
use crate::network::cbor_request_response::CborCodec;
use crate::network::spot_price;
use crate::network::spot_price::SpotPriceProtocol;
use crate::protocol::alice;
use crate::protocol::alice::event_loop::LatestRate;
use libp2p::request_response::{
    ProtocolSupport, RequestResponseConfig, RequestResponseEvent, RequestResponseMessage,
    ResponseChannel,
};
use libp2p::swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters};
use libp2p::{NetworkBehaviour, PeerId};
use std::collections::VecDeque;
use std::fmt::Debug;
use std::task::{Context, Poll};

pub enum OutEvent {
    ExecutionSetupParams {
        peer: PeerId,
        btc: bitcoin::Amount,
        xmr: monero::Amount,
    },
    Error {
        peer: PeerId,
        error: Error,
    },
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", poll_method = "poll", event_process = true)]
#[allow(missing_debug_implementations)]
pub struct Behaviour<LR>
where
    LR: LatestRate + Send + 'static,
{
    behaviour: spot_price::Behaviour,

    #[behaviour(ignore)]
    events: VecDeque<OutEvent>,

    #[behaviour(ignore)]
    balance: monero::Amount,
    #[behaviour(ignore)]
    lock_fee: monero::Amount,
    #[behaviour(ignore)]
    max_buy: bitcoin::Amount,
    #[behaviour(ignore)]
    latest_rate: LR,
    #[behaviour(ignore)]
    resume_only: bool,
}

/// Behaviour that handles spot prices.
/// All the logic how to react to a spot price request is contained here, events
/// reporting the successful handling of a spot price request or a failure are
/// bubbled up to the parent behaviour.
impl<LR> Behaviour<LR>
where
    LR: LatestRate + Send + 'static,
{
    pub fn new(
        balance: monero::Amount,
        lock_fee: monero::Amount,
        max_buy: bitcoin::Amount,
        latest_rate: LR,
        resume_only: bool,
    ) -> Self {
        Self {
            behaviour: spot_price::Behaviour::new(
                CborCodec::default(),
                vec![(SpotPriceProtocol, ProtocolSupport::Inbound)],
                RequestResponseConfig::default(),
            ),
            events: Default::default(),
            balance,
            lock_fee,
            max_buy,
            latest_rate,
            resume_only,
        }
    }

    pub fn update_balance(&mut self, balance: monero::Amount) {
        self.balance = balance;
    }

    fn decline(
        &mut self,
        peer: PeerId,
        channel: ResponseChannel<spot_price::Response>,
        error: Error,
    ) {
        if self
            .behaviour
            .send_response(
                channel,
                spot_price::Response::Error(error.to_error_response()),
            )
            .is_err()
        {
            tracing::debug!(%peer, "Unable to send error response for spot price request");
        }

        self.events.push_back(OutEvent::Error { peer, error });
    }

    fn poll<BIE>(
        &mut self,
        _cx: &mut Context<'_>,
        _params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<BIE, OutEvent>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }

        // We trust in libp2p to poll us.
        Poll::Pending
    }
}

impl<LR> NetworkBehaviourEventProcess<spot_price::OutEvent> for Behaviour<LR>
where
    LR: LatestRate + Send + 'static,
{
    fn inject_event(&mut self, event: spot_price::OutEvent) {
        let (peer, message) = match event {
            RequestResponseEvent::Message { peer, message } => (peer, message),
            RequestResponseEvent::OutboundFailure { peer, error, .. } => {
                tracing::error!(%peer, "Failure sending spot price response: {:#}", error);
                return;
            }
            RequestResponseEvent::InboundFailure { peer, error, .. } => {
                tracing::warn!(%peer, "Inbound failure when handling spot price request: {:#}", error);
                return;
            }
            RequestResponseEvent::ResponseSent { peer, .. } => {
                tracing::debug!(%peer, "Spot price response sent");
                return;
            }
        };

        let (request, channel) = match message {
            RequestResponseMessage::Request {
                request, channel, ..
            } => (request, channel),
            RequestResponseMessage::Response { .. } => {
                tracing::error!("Unexpected message");
                return;
            }
        };

        if self.resume_only {
            self.decline(peer, channel, Error::ResumeOnlyMode);
            return;
        }

        let btc = request.btc;
        if btc > self.max_buy {
            self.decline(peer, channel, Error::MaxBuyAmountExceeded {
                max: self.max_buy,
                buy: btc,
            });
            return;
        }

        let rate = match self.latest_rate.latest_rate() {
            Ok(rate) => rate,
            Err(e) => {
                self.decline(peer, channel, Error::LatestRateFetchFailed(Box::new(e)));
                return;
            }
        };
        let xmr = match rate.sell_quote(btc) {
            Ok(xmr) => xmr,
            Err(e) => {
                self.decline(peer, channel, Error::SellQuoteCalculationFailed(e));
                return;
            }
        };

        let xmr_balance = self.balance;
        let xmr_lock_fees = self.lock_fee;

        if xmr_balance < xmr + xmr_lock_fees {
            self.decline(peer, channel, Error::BalanceTooLow { buy: btc });
            return;
        }

        if self
            .behaviour
            .send_response(channel, spot_price::Response::Xmr(xmr))
            .is_err()
        {
            tracing::error!(%peer, "Failed to send spot price response of {} for {}", xmr, btc)
        }

        self.events
            .push_back(OutEvent::ExecutionSetupParams { peer, btc, xmr });
    }
}

impl From<OutEvent> for alice::OutEvent {
    fn from(event: OutEvent) -> Self {
        match event {
            OutEvent::ExecutionSetupParams { peer, btc, xmr } => {
                Self::ExecutionSetupStart { peer, btc, xmr }
            }
            OutEvent::Error { peer, error } => Self::SwapRequestDeclined { peer, error },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ASB is running in resume-only mode")]
    ResumeOnlyMode,
    #[error("Maximum buy {max} exceeded {buy}")]
    MaxBuyAmountExceeded {
        max: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    #[error("This seller's XMR balance is currently too low to fulfill the swap request to buy {buy}, please try again later")]
    BalanceTooLow { buy: bitcoin::Amount },

    #[error("Failed to fetch latest rate")]
    LatestRateFetchFailed(#[source] Box<dyn std::error::Error + Send + 'static>),

    #[error("Failed to calculate quote: {0}")]
    SellQuoteCalculationFailed(#[source] anyhow::Error),
}

impl Error {
    pub fn to_error_response(&self) -> spot_price::Error {
        match self {
            Error::ResumeOnlyMode => spot_price::Error::NoSwapsAccepted,
            Error::MaxBuyAmountExceeded { max, buy } => spot_price::Error::MaxBuyAmountExceeded {
                max: *max,
                buy: *buy,
            },
            Error::BalanceTooLow { buy } => spot_price::Error::BalanceTooLow { buy: *buy },
            Error::LatestRateFetchFailed(_) | Error::SellQuoteCalculationFailed(_) => {
                spot_price::Error::Other
            }
        }
    }
}
