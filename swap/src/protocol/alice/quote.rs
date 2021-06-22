use libp2p::{NetworkBehaviour, PeerId};
use crate::network::quote::{BidQuote, BidQuoteProtocol};
use crate::protocol::alice::event_loop::LatestRate;
use std::collections::VecDeque;
use libp2p::request_response::{RequestResponseConfig, ProtocolSupport, ResponseChannel};
use crate::network::json_pull_codec::JsonPullCodec;
use crate::monero;
use std::task::{Context, Poll};
use libp2p::swarm::{PollParameters, NetworkBehaviourAction, NetworkBehaviourEventProcess};
use crate::protocol::alice;
use crate::network::quote;

#[derive(Debug)]
pub enum OutEvent {
    QuoteSent {
        peer: PeerId,
        quote: BidQuote
    },
    Error {
        peer: PeerId,
        error: Error,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ASB is running in resume-only mode")]
    FailedToCreateQuote,
    #[error("Balance is {xmr_balance} which is insufficient to fulfill max buy of {max_buy} at price {price}")]
    InsufficientFunds {
        max_buy: bitcoin::Amount,
        price: bitcoin::Amount,
        xmr_balance: monero::Amount
    },
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", poll_method = "poll", event_process = true)]
#[allow(missing_debug_implementations)]
pub struct Behaviour<LR>
    where
        LR: LatestRate + Send + 'static,
{
    behaviour: quote::Behaviour,

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

/// Behaviour that handles quotes
/// All the logic how to react to a quote request is contained here, events
/// reporting the successful handling of a quote request or a failure are
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
            behaviour: quote::Behaviour::new(
                JsonPullCodec::default(),
                vec![(BidQuoteProtocol, ProtocolSupport::Inbound)],
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
        channel: ResponseChannel<quote::Response>,
        error: Error,
    ) {
        if self
            .behaviour
            .send_response(
                channel,
                quote::Response::Error,
            )
            .is_err()
        {
            tracing::debug!(%peer, "Unable to send error response for quote request");
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

impl<LR> NetworkBehaviourEventProcess<quote::OutEvent> for Behaviour<LR>
    where
        LR: LatestRate + Send + 'static,
{
    fn inject_event(&mut self, event: quote::OutEvent) {

        // TODO: Move the quote from the event loop into here

        todo!()
    }
}

impl From<OutEvent> for alice::OutEvent {
    fn from(event: OutEvent) -> Self {
        match event {
            OutEvent::QuoteSent { peer, quote } => {
                Self::QuoteSent { peer, quote }
            }
            OutEvent::Error { peer, error } => Self::QuoteError { peer, error },
        }
    }
}


// TODO: Add tests similar to spot price
