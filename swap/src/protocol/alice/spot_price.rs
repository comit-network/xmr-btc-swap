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

#[derive(Debug)]
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
            self.decline(peer, channel, Error::BalanceTooLow {
                balance: xmr_balance,
                buy: btc,
            });
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
    #[error("Balance {balance} too low to fulfill swapping {buy}")]
    BalanceTooLow {
        balance: monero::Amount,
        buy: bitcoin::Amount,
    },

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
            Error::BalanceTooLow { buy, .. } => spot_price::Error::BalanceTooLow { buy: *buy },
            Error::LatestRateFetchFailed(_) | Error::SellQuoteCalculationFailed(_) => {
                spot_price::Error::Other
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asb::Rate;
    use crate::monero;
    use crate::network::test::{await_events_or_timeout, connect, new_swarm};
    use crate::protocol::{alice, bob};
    use anyhow::anyhow;
    use libp2p::Swarm;
    use rust_decimal::Decimal;

    impl Default for AliceBehaviourValues {
        fn default() -> Self {
            Self {
                balance: monero::Amount::from_monero(1.0).unwrap(),
                lock_fee: monero::Amount::ZERO,
                max_buy: bitcoin::Amount::from_btc(0.01).unwrap(),
                rate: TestRate::default(), // 0.01
                resume_only: false,
            }
        }
    }

    #[tokio::test]
    async fn given_alice_has_sufficient_balance_then_returns_price() {
        let mut test = SpotPriceTest::setup(AliceBehaviourValues::default()).await;

        let btc_to_swap = bitcoin::Amount::from_btc(0.01).unwrap();
        let expected_xmr = monero::Amount::from_monero(1.0).unwrap();

        let request = spot_price::Request { btc: btc_to_swap };

        test.send_request(request);
        test.assert_price((btc_to_swap, expected_xmr), expected_xmr)
            .await;
    }

    #[tokio::test]
    async fn given_alice_has_insufficient_balance_then_returns_error() {
        let mut test = SpotPriceTest::setup(
            AliceBehaviourValues::default().with_balance(monero::Amount::ZERO),
        )
        .await;

        let btc_to_swap = bitcoin::Amount::from_btc(0.01).unwrap();

        let request = spot_price::Request { btc: btc_to_swap };

        test.send_request(request);
        test.assert_error(
            alice::spot_price::Error::BalanceTooLow {
                balance: monero::Amount::ZERO,
                buy: btc_to_swap,
            },
            bob::spot_price::Error::BalanceTooLow { buy: btc_to_swap },
        )
        .await;
    }

    #[tokio::test]
    async fn given_alice_has_insufficient_balance_after_balance_update_then_returns_error() {
        let mut test = SpotPriceTest::setup(AliceBehaviourValues::default()).await;

        let btc_to_swap = bitcoin::Amount::from_btc(0.01).unwrap();
        let expected_xmr = monero::Amount::from_monero(1.0).unwrap();

        let request = spot_price::Request { btc: btc_to_swap };

        test.send_request(request);
        test.assert_price((btc_to_swap, expected_xmr), expected_xmr)
            .await;

        test.alice_swarm
            .behaviour_mut()
            .update_balance(monero::Amount::ZERO);

        let request = spot_price::Request { btc: btc_to_swap };

        test.send_request(request);
        test.assert_error(
            alice::spot_price::Error::BalanceTooLow {
                balance: monero::Amount::ZERO,
                buy: btc_to_swap,
            },
            bob::spot_price::Error::BalanceTooLow { buy: btc_to_swap },
        )
        .await;
    }

    #[tokio::test]
    async fn given_alice_has_insufficient_balance_because_of_lock_fee_then_returns_error() {
        let balance = monero::Amount::from_monero(1.0).unwrap();

        let mut test = SpotPriceTest::setup(
            AliceBehaviourValues::default()
                .with_balance(balance)
                .with_lock_fee(monero::Amount::from_piconero(1)),
        )
        .await;

        let btc_to_swap = bitcoin::Amount::from_btc(0.01).unwrap();

        let request = spot_price::Request { btc: btc_to_swap };

        test.send_request(request);
        test.assert_error(
            alice::spot_price::Error::BalanceTooLow {
                balance,
                buy: btc_to_swap,
            },
            bob::spot_price::Error::BalanceTooLow { buy: btc_to_swap },
        )
        .await;
    }

    #[tokio::test]
    async fn given_max_buy_exceeded_then_returns_error() {
        let max_buy = bitcoin::Amount::from_btc(0.001).unwrap();

        let mut test =
            SpotPriceTest::setup(AliceBehaviourValues::default().with_max_buy(max_buy)).await;

        let btc_to_swap = bitcoin::Amount::from_btc(0.01).unwrap();

        let request = spot_price::Request { btc: btc_to_swap };

        test.send_request(request);
        test.assert_error(
            alice::spot_price::Error::MaxBuyAmountExceeded {
                buy: btc_to_swap,
                max: max_buy,
            },
            bob::spot_price::Error::MaxBuyAmountExceeded {
                buy: btc_to_swap,
                max: max_buy,
            },
        )
        .await;
    }

    #[tokio::test]
    async fn given_alice_in_resume_only_mode_then_returns_error() {
        let mut test =
            SpotPriceTest::setup(AliceBehaviourValues::default().with_resume_only(true)).await;

        let btc_to_swap = bitcoin::Amount::from_btc(0.01).unwrap();

        let request = spot_price::Request { btc: btc_to_swap };

        test.send_request(request);
        test.assert_error(
            alice::spot_price::Error::ResumeOnlyMode,
            bob::spot_price::Error::NoSwapsAccepted,
        )
        .await;
    }

    #[tokio::test]
    async fn given_rate_fetch_problem_then_returns_error() {
        let mut test =
            SpotPriceTest::setup(AliceBehaviourValues::default().with_rate(TestRate::error_rate()))
                .await;

        let btc_to_swap = bitcoin::Amount::from_btc(0.01).unwrap();

        let request = spot_price::Request { btc: btc_to_swap };

        test.send_request(request);
        test.assert_error(
            alice::spot_price::Error::LatestRateFetchFailed(Box::new(TestRateError {})),
            bob::spot_price::Error::Other,
        )
        .await;
    }

    #[tokio::test]
    async fn given_rate_calculation_problem_then_returns_error() {
        let mut test = SpotPriceTest::setup(
            AliceBehaviourValues::default().with_rate(TestRate::from_rate_and_spread(0.0, 0)),
        )
        .await;

        let btc_to_swap = bitcoin::Amount::from_btc(0.01).unwrap();

        let request = spot_price::Request { btc: btc_to_swap };

        test.send_request(request);
        test.assert_error(
            alice::spot_price::Error::SellQuoteCalculationFailed(anyhow!(
                "Error text irrelevant, won't be checked here"
            )),
            bob::spot_price::Error::Other,
        )
        .await;
    }

    struct SpotPriceTest {
        alice_swarm: Swarm<alice::spot_price::Behaviour<TestRate>>,
        bob_swarm: Swarm<spot_price::Behaviour>,

        alice_peer_id: PeerId,
    }

    impl SpotPriceTest {
        pub async fn setup(values: AliceBehaviourValues) -> Self {
            let (mut alice_swarm, _, alice_peer_id) = new_swarm(|_, _| {
                Behaviour::new(
                    values.balance,
                    values.lock_fee,
                    values.max_buy,
                    values.rate.clone(),
                    values.resume_only,
                )
            });
            let (mut bob_swarm, ..) = new_swarm(|_, _| bob::spot_price::bob());

            connect(&mut alice_swarm, &mut bob_swarm).await;

            Self {
                alice_swarm,
                bob_swarm,
                alice_peer_id,
            }
        }

        pub fn send_request(&mut self, spot_price_request: spot_price::Request) {
            self.bob_swarm
                .behaviour_mut()
                .send_request(&self.alice_peer_id, spot_price_request);
        }

        async fn assert_price(
            &mut self,
            alice_assert: (bitcoin::Amount, monero::Amount),
            bob_assert: monero::Amount,
        ) {
            match await_events_or_timeout(self.alice_swarm.next(), self.bob_swarm.next()).await {
                (
                    alice::spot_price::OutEvent::ExecutionSetupParams { btc, xmr, .. },
                    spot_price::OutEvent::Message { message, .. },
                ) => {
                    assert_eq!(alice_assert, (btc, xmr));

                    let response = match message {
                        RequestResponseMessage::Response { response, .. } => response,
                        _ => panic!("Unexpected message {:?} for Bob", message),
                    };

                    match response {
                        spot_price::Response::Xmr(xmr) => {
                            assert_eq!(bob_assert, xmr)
                        }
                        _ => panic!("Unexpected response {:?} for Bob", response),
                    }
                }
                (alice_event, bob_event) => panic!(
                    "Received unexpected event, alice emitted {:?} and bob emitted {:?}",
                    alice_event, bob_event
                ),
            }
        }

        async fn assert_error(
            &mut self,
            alice_assert: alice::spot_price::Error,
            bob_assert: bob::spot_price::Error,
        ) {
            match await_events_or_timeout(self.alice_swarm.next(), self.bob_swarm.next()).await {
                (
                    alice::spot_price::OutEvent::Error { error, .. },
                    spot_price::OutEvent::Message { message, .. },
                ) => {
                    // TODO: Somehow make PartialEq work on Alice's spot_price::Error
                    match (alice_assert, error) {
                        (
                            alice::spot_price::Error::BalanceTooLow {
                                balance: balance1,
                                buy: buy1,
                            },
                            alice::spot_price::Error::BalanceTooLow {
                                balance: balance2,
                                buy: buy2,
                            },
                        ) => {
                            assert_eq!(balance1, balance2);
                            assert_eq!(buy1, buy2);
                        }
                        (
                            alice::spot_price::Error::MaxBuyAmountExceeded { .. },
                            alice::spot_price::Error::MaxBuyAmountExceeded { .. },
                        )
                        | (
                            alice::spot_price::Error::LatestRateFetchFailed(_),
                            alice::spot_price::Error::LatestRateFetchFailed(_),
                        )
                        | (
                            alice::spot_price::Error::SellQuoteCalculationFailed(_),
                            alice::spot_price::Error::SellQuoteCalculationFailed(_),
                        )
                        | (
                            alice::spot_price::Error::ResumeOnlyMode,
                            alice::spot_price::Error::ResumeOnlyMode,
                        ) => {}
                        (alice_assert, error) => {
                            panic!("Expected: {:?} Actual: {:?}", alice_assert, error)
                        }
                    }

                    let response = match message {
                        RequestResponseMessage::Response { response, .. } => response,
                        _ => panic!("Unexpected message {:?} for Bob", message),
                    };

                    match response {
                        spot_price::Response::Error(error) => {
                            assert_eq!(bob_assert, error.into())
                        }
                        _ => panic!("Unexpected response {:?} for Bob", response),
                    }
                }
                (alice_event, bob_event) => panic!(
                    "Received unexpected event, alice emitted {:?} and bob emitted {:?}",
                    alice_event, bob_event
                ),
            }
        }
    }

    struct AliceBehaviourValues {
        pub balance: monero::Amount,
        pub lock_fee: monero::Amount,
        pub max_buy: bitcoin::Amount,
        pub rate: TestRate, // 0.01
        pub resume_only: bool,
    }

    impl AliceBehaviourValues {
        pub fn with_balance(mut self, balance: monero::Amount) -> AliceBehaviourValues {
            self.balance = balance;
            self
        }

        pub fn with_lock_fee(mut self, lock_fee: monero::Amount) -> AliceBehaviourValues {
            self.lock_fee = lock_fee;
            self
        }

        pub fn with_max_buy(mut self, max_buy: bitcoin::Amount) -> AliceBehaviourValues {
            self.max_buy = max_buy;
            self
        }

        pub fn with_resume_only(mut self, resume_only: bool) -> AliceBehaviourValues {
            self.resume_only = resume_only;
            self
        }

        pub fn with_rate(mut self, rate: TestRate) -> AliceBehaviourValues {
            self.rate = rate;
            self
        }
    }

    #[derive(Clone, Debug)]
    pub enum TestRate {
        Rate(Rate),
        Err(TestRateError),
    }

    impl TestRate {
        pub const RATE: f64 = 0.01;

        pub fn from_rate_and_spread(rate: f64, spread: u64) -> Self {
            let ask = bitcoin::Amount::from_btc(rate).expect("Static value should never fail");
            let spread = Decimal::from(spread);
            Self::Rate(Rate::new(ask, spread))
        }

        pub fn error_rate() -> Self {
            Self::Err(TestRateError {})
        }
    }

    impl Default for TestRate {
        fn default() -> Self {
            TestRate::from_rate_and_spread(Self::RATE, 0)
        }
    }

    #[derive(Debug, Clone, thiserror::Error)]
    #[error("Could not fetch rate")]
    pub struct TestRateError {}

    impl LatestRate for TestRate {
        type Error = TestRateError;

        fn latest_rate(&mut self) -> Result<Rate, Self::Error> {
            match self {
                TestRate::Rate(rate) => Ok(*rate),
                TestRate::Err(error) => Err(error.clone()),
            }
        }
    }
}
