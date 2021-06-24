use anyhow::{Result, Context};
use std::collections::VecDeque;
use std::fmt::Debug;
use std::future;
use std::task::{Context, Poll};

use futures::future::{BoxFuture, OptionFuture};
use libp2p::{Multiaddr, NetworkBehaviour, PeerId};
use libp2p::core::connection::ConnectionId;
use libp2p::core::{Endpoint, upgrade};
use libp2p::core::upgrade::from_fn;
use libp2p::core::upgrade::FromFnUpgrade;
use libp2p::request_response::{
    ProtocolSupport, RequestResponseConfig, RequestResponseEvent, RequestResponseMessage,
    ResponseChannel,
};
use libp2p::swarm::{IntoProtocolsHandler, KeepAlive, NegotiatedSubstream, NetworkBehaviour, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters, ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr, SubstreamProtocol};
use libp2p::swarm::protocols_handler::{InboundUpgradeSend, OutboundUpgradeSend};
use uuid::Uuid;
use void::Void;

use crate::{env, monero};
use crate::network::cbor_request_response::CborCodec;
use crate::network::spot_price;
use crate::network::spot_price::{BlockchainNetwork, SpotPriceProtocol};
use crate::protocol::{alice, bob, Message0, Message2, Message4};
use crate::protocol::alice::event_loop::LatestRate;
use crate::protocol::alice::{State3, State0};
use futures::FutureExt;
use tokio::sync::oneshot;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum OutEvent {
    Initiated {
        send_wallet_snapshot: oneshot::Sender<WalletSnapshot>
    },
    Completed {
        bob_peer_id: PeerId,
        swap_id: Uuid,
        state3: State3,
    },
    Error, // TODO be more descriptive
}

pub struct WalletSnapshot {
    balance: monero::Amount,
    lock_fee: monero::Amount,

    // TODO: Consider using the same address for punish and redeem (they are mutually exclusive, so effectively the address will only be used once)
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,

    redeem_fee: bitcoin::Amount,
    refund_fee: bitcoin::Amount,
}

impl WalletSnapshot {
    pub async fn capture(bitcoin_wallet: &bitcoin::Wallet, monero_wallet: &monero::Wallet) -> Result<Self> {
        Ok(Self {
            balance: monero_wallet.get_balance().await?,
            lock_fee: monero::MONERO_FEE,
            redeem_address: bitcoin_wallet.new_address().await?,
            punish_address: bitcoin_wallet.new_address().await?,
            redeem_fee: bitcoin_wallet
                .estimate_fee(bitcoin::TxRedeem::weight(), btc)
                .await,
            refund_fee: bitcoin_wallet
                .estimate_fee(bitcoin::TxPunish::weight(), btc)
                .await
        })
    }
}

#[allow(missing_debug_implementations)]
pub struct Behaviour<LR>
where
    LR: LatestRate + Send + 'static,
{
    events: VecDeque<OutEvent>,
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    env_config: env::Config,

    latest_rate: LR,
    resume_only: bool,
}

impl<LR> Behaviour<LR>
where
    LR: LatestRate + Send + 'static,
{
    pub fn new(
        balance: monero::Amount,
        lock_fee: monero::Amount,
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
        env_config: env::Config,
        latest_rate: LR,
        resume_only: bool,
    ) -> Self {
        Self {
            events: Default::default(),
            balance,
            lock_fee,
            min_buy,
            max_buy,
            env_config,
            latest_rate,
            resume_only,
        }
    }

    pub fn update(&mut self, monero_balance: monero::Amount, redeem_address: bitcoin::Address, punish_address: bitcoin::Address) {
        self.balance = monero_balance;
    }
}

impl<LR> NetworkBehaviour for Behaviour<LR> {
    type ProtocolsHandler = Handler;
    type OutEvent = OutEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        Handler::default()
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        todo!()
    }

    fn inject_connected(&mut self, peer_id: &PeerId) {
        todo!()
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        todo!()
    }

    fn inject_event(&mut self, peer_id: PeerId, connection: ConnectionId, event: _) {
        todo!()
    }

    fn poll(&mut self, cx: &mut Context<'_>, params: &mut impl PollParameters) -> Poll<NetworkBehaviourAction<_, Self::OutEvent>> {
        todo!()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpotPriceRequest {
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc: bitcoin::Amount,
    pub blockchain_network: BlockchainNetwork,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SpotPriceError {
    NoSwapsAccepted,
    AmountBelowMinimum {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        min: bitcoin::Amount,
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },
    AmountAboveMaximum {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        max: bitcoin::Amount,
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },
    BalanceTooLow {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },
    BlockchainNetworkMismatch {
        cli: BlockchainNetwork,
        asb: BlockchainNetwork,
    },
    /// To be used for errors that cannot be explained on the CLI side (e.g.
    /// rate update problems on the seller side)
    Other,
}

// TODO: This is bob only.
// enum OutboundState {
//     PendingOpen(
//         // TODO: put data in here we pass in when we want to kick of swap setup, just bitcoin amount?
//     ),
//     PendingNegotiate,
//     Executing(BoxFuture<'static, anyhow::Result<(Uuid, bob::State3)>>)
// }

// TODO: Don't just use anyhow::Error
type InboundStream = BoxFuture<'static, anyhow::Result<(Uuid, alice::State3)>>;

struct Handler {
    inbound_stream: OptionFuture<InboundStream>,
    events: VecDeque<HandlerOutEvent>,
    resume_only: bool
}

enum HandlerOutEvent {
    Initiated(oneshot::Sender<WalletSnapshot>),
    Completed(anyhow::Result<(Uuid, alice::State3)>)
}

pub const BUF_SIZE: usize = 1024 * 1024;

impl ProtocolsHandler for Handler {
    type InEvent = ();
    type OutEvent = HandlerOutEvent;
    type Error = ();
    type InboundProtocol = protocol::SwapSetup;
    type OutboundProtocol = ();
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(protocol::new(), todo!("pass data down to handler"))
    }

    fn inject_fully_negotiated_inbound(&mut self, mut protocol: NegotiatedSubstream, _: Self::InboundOpenInfo) {
        let (sender, receiver) = oneshot::channel();
        let resume_only = self.resume_only;
        
        self.inbound_stream = OptionFuture::from(Some(async move {
            let request = read_cbor_message::<SpotPriceRequest>(&mut protocol).await?;
            let wallet_snapshot = receiver.await?; // TODO Put a timeout on this

            async {
                if resume_only {
                    return Err(Error::ResumeOnlyMode)
                };


            }


            let blockchain_network = BlockchainNetwork {
                bitcoin: self.env_config.bitcoin_network,
                monero: self.env_config.monero_network,
            };

            if request.blockchain_network != blockchain_network {
                self.decline(peer, channel, Error::BlockchainNetworkMismatch {
                    cli: request.blockchain_network,
                    asb: blockchain_network,
                });
                return;
            }



            let btc = request.btc;

            if btc < self.min_buy {
                self.decline(peer, channel, Error::AmountBelowMinimum {
                    min: self.min_buy,
                    buy: btc,
                });
                return;
            }

            if btc > self.max_buy {
                self.decline(peer, channel, Error::AmountAboveMaximum {
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

            let state0 = State0::new(spot_price_request.btc, todo!(), todo!(), todo!(), todo!(), todo!(), todo!(), todo!())?;
            
            let message0 = read_cbor_message::<Message0>(&mut protocol).context("Failed to deserialize message0")?;
            let (swap_id, state1) = state0.receive(message0)?;

            write_cbor_message(&mut protocol, state1.next_message()).await?;

            let message2 = read_cbor_message::<Message2>(&mut protocol).context("Failed to deserialize message2")?;
            let state2 = state1
                .receive(message2)
                .context("Failed to receive Message2")?;

            write_cbor_message(&mut protocol, state2.next_message()).await?;
            
            let message4 = read_cbor_message::<Message4>(&mut protocol).context("Failed to deserialize message4")?;
            let state3 = state2
                .receive(message4)
                .context("Failed to receive Message4")?;

            Ok((swap_id, state3))
        }.boxed()));
        self.events.push_back(HandlerOutEvent::Initiated(sender));
    }

    fn inject_fully_negotiated_outbound(&mut self, protocol: NegotiatedSubstream, info: Self::OutboundOpenInfo) {
        unreachable!("we don't support outbound")
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        todo!()
    }

    fn inject_dial_upgrade_error(&mut self, info: Self::OutboundOpenInfo, error: ProtocolsHandlerUpgrErr<_>) {
        todo!()
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        todo!()
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent, Self::Error>> {
        let event = futures::ready!(self.inbound_stream.poll(cx));
        
        Poll::Ready(ProtocolsHandlerEvent::Custom(HandlerOutEvent::Completed(event)))
    }
}

async fn read_cbor_message<T>(substream: &mut NegotiatedSubstream) -> Result<T> where T: Deserialize {
    let bytes = upgrade::read_one(substream, BUF_SIZE).await?;
    let mut de = serde_cbor::Deserializer::from_slice(&bytes);
    let message = T::deserialize(de)?;
    
    Ok(message)
}

async fn write_cbor_message<T>(substream: &mut NegotiatedSubstream, message: T) -> Result<()> where T: Serialize {
    let bytes = serde_cbor::to_vec(&message)?;
    upgrade::write_one(substream, &bytes).await?;
    
    Ok(())
}

async fn write_error_message(substream: &mut NegotiatedSubstream, message: impl Into<SpotPriceError>) -> Result<()> {
    let bytes = serde_cbor::to_vec(&message.into())?;
    upgrade::write_one(substream, &bytes).await?;

    Ok(())
}

mod protocol {
    use super::*;

    pub fn new() -> SwapSetup {
        from_fn(
            b"/comit/xmr/btc/swap_setup/1.0.0",
            Box::new(|socket, endpoint| future::ready(match endpoint {
                Endpoint::Listener => Ok(socket),
                Endpoint::Dialer => todo!("return error")
            })),
        )
    }

    pub type SwapSetup = FromFnUpgrade<
        &'static [u8],
        Box<
            dyn Fn(
                NegotiatedSubstream,
                Endpoint,
            )
                -> future::Ready<Result<NegotiatedSubstream, Void>>
            + Send
            + 'static,
        >,
    >;

}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ASB is running in resume-only mode")]
    ResumeOnlyMode,
    #[error("Amount {buy} below minimum {min}")]
    AmountBelowMinimum {
        min: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    #[error("Amount {buy} above maximum {max}")]
    AmountAboveMaximum {
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
    #[error("Blockchain networks did not match, we are on {asb:?}, but request from {cli:?}")]
    BlockchainNetworkMismatch {
        cli: spot_price::BlockchainNetwork,
        asb: spot_price::BlockchainNetwork,
    },
}

impl Error {
    pub fn to_error_response(&self) -> spot_price::Error {
        match self {
            Error::ResumeOnlyMode => spot_price::Error::NoSwapsAccepted,
            Error::AmountBelowMinimum { min, buy } => spot_price::Error::AmountBelowMinimum {
                min: *min,
                buy: *buy,
            },
            Error::AmountAboveMaximum { max, buy } => spot_price::Error::AmountAboveMaximum {
                max: *max,
                buy: *buy,
            },
            Error::BalanceTooLow { buy, .. } => spot_price::Error::BalanceTooLow { buy: *buy },
            Error::BlockchainNetworkMismatch { cli, asb } => {
                spot_price::Error::BlockchainNetworkMismatch {
                    cli: *cli,
                    asb: *asb,
                }
            }
            Error::LatestRateFetchFailed(_) | Error::SellQuoteCalculationFailed(_) => {
                spot_price::Error::Other
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use libp2p::Swarm;
    use rust_decimal::Decimal;

    use crate::{monero, network};
    use crate::asb::Rate;
    use crate::env::GetConfig;
    use crate::network::test::{await_events_or_timeout, connect, new_swarm};
    use crate::protocol::{alice, bob};

    use super::*;

    impl Default for AliceBehaviourValues {
        fn default() -> Self {
            Self {
                balance: monero::Amount::from_monero(1.0).unwrap(),
                lock_fee: monero::Amount::ZERO,
                min_buy: bitcoin::Amount::from_btc(0.001).unwrap(),
                max_buy: bitcoin::Amount::from_btc(0.01).unwrap(),
                rate: TestRate::default(), // 0.01
                resume_only: false,
                env_config: env::Testnet::get_config(),
            }
        }
    }

    #[tokio::test]
    async fn given_alice_has_sufficient_balance_then_returns_price() {
        let mut test = SpotPriceTest::setup(AliceBehaviourValues::default()).await;

        let btc_to_swap = bitcoin::Amount::from_btc(0.01).unwrap();
        let expected_xmr = monero::Amount::from_monero(1.0).unwrap();

        test.construct_and_send_request(btc_to_swap);
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

        test.construct_and_send_request(btc_to_swap);
        test.assert_error(
            alice::swap_setup::Error::BalanceTooLow {
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

        test.construct_and_send_request(btc_to_swap);
        test.assert_price((btc_to_swap, expected_xmr), expected_xmr)
            .await;

        test.alice_swarm
            .behaviour_mut()
            .update_balance(monero::Amount::ZERO);

        test.construct_and_send_request(btc_to_swap);
        test.assert_error(
            alice::swap_setup::Error::BalanceTooLow {
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
        test.construct_and_send_request(btc_to_swap);
        test.assert_error(
            alice::swap_setup::Error::BalanceTooLow {
                balance,
                buy: btc_to_swap,
            },
            bob::spot_price::Error::BalanceTooLow { buy: btc_to_swap },
        )
        .await;
    }

    #[tokio::test]
    async fn given_below_min_buy_then_returns_error() {
        let min_buy = bitcoin::Amount::from_btc(0.001).unwrap();

        let mut test =
            SpotPriceTest::setup(AliceBehaviourValues::default().with_min_buy(min_buy)).await;

        let btc_to_swap = bitcoin::Amount::from_btc(0.0001).unwrap();
        test.construct_and_send_request(btc_to_swap);
        test.assert_error(
            alice::swap_setup::Error::AmountBelowMinimum {
                buy: btc_to_swap,
                min: min_buy,
            },
            bob::spot_price::Error::AmountBelowMinimum {
                buy: btc_to_swap,
                min: min_buy,
            },
        )
        .await;
    }

    #[tokio::test]
    async fn given_above_max_buy_then_returns_error() {
        let max_buy = bitcoin::Amount::from_btc(0.001).unwrap();

        let mut test =
            SpotPriceTest::setup(AliceBehaviourValues::default().with_max_buy(max_buy)).await;

        let btc_to_swap = bitcoin::Amount::from_btc(0.01).unwrap();

        test.construct_and_send_request(btc_to_swap);
        test.assert_error(
            alice::swap_setup::Error::AmountAboveMaximum {
                buy: btc_to_swap,
                max: max_buy,
            },
            bob::spot_price::Error::AmountAboveMaximum {
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
        test.construct_and_send_request(btc_to_swap);
        test.assert_error(
            alice::swap_setup::Error::ResumeOnlyMode,
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
        test.construct_and_send_request(btc_to_swap);
        test.assert_error(
            alice::swap_setup::Error::LatestRateFetchFailed(Box::new(TestRateError {})),
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

        test.construct_and_send_request(btc_to_swap);
        test.assert_error(
            alice::swap_setup::Error::SellQuoteCalculationFailed(anyhow!(
                "Error text irrelevant, won't be checked here"
            )),
            bob::spot_price::Error::Other,
        )
        .await;
    }

    #[tokio::test]
    async fn given_alice_mainnnet_bob_testnet_then_network_mismatch_error() {
        let mut test = SpotPriceTest::setup(
            AliceBehaviourValues::default().with_env_config(env::Mainnet::get_config()),
        )
        .await;

        let btc_to_swap = bitcoin::Amount::from_btc(0.01).unwrap();
        test.construct_and_send_request(btc_to_swap);
        test.assert_error(
            alice::swap_setup::Error::BlockchainNetworkMismatch {
                cli: BlockchainNetwork {
                    bitcoin: bitcoin::Network::Testnet,
                    monero: monero::Network::Stagenet,
                },
                asb: BlockchainNetwork {
                    bitcoin: bitcoin::Network::Bitcoin,
                    monero: monero::Network::Mainnet,
                },
            },
            bob::spot_price::Error::BlockchainNetworkMismatch {
                cli: BlockchainNetwork {
                    bitcoin: bitcoin::Network::Testnet,
                    monero: monero::Network::Stagenet,
                },
                asb: BlockchainNetwork {
                    bitcoin: bitcoin::Network::Bitcoin,
                    monero: monero::Network::Mainnet,
                },
            },
        )
        .await;
    }

    #[tokio::test]
    async fn given_alice_testnet_bob_mainnet_then_network_mismatch_error() {
        let mut test = SpotPriceTest::setup(AliceBehaviourValues::default()).await;

        let btc_to_swap = bitcoin::Amount::from_btc(0.01).unwrap();
        let request = spot_price::Request {
            btc: btc_to_swap,
            blockchain_network: BlockchainNetwork {
                bitcoin: bitcoin::Network::Bitcoin,
                monero: monero::Network::Mainnet,
            },
        };

        test.send_request(request);
        test.assert_error(
            alice::swap_setup::Error::BlockchainNetworkMismatch {
                cli: BlockchainNetwork {
                    bitcoin: bitcoin::Network::Bitcoin,
                    monero: monero::Network::Mainnet,
                },
                asb: BlockchainNetwork {
                    bitcoin: bitcoin::Network::Testnet,
                    monero: monero::Network::Stagenet,
                },
            },
            bob::spot_price::Error::BlockchainNetworkMismatch {
                cli: BlockchainNetwork {
                    bitcoin: bitcoin::Network::Bitcoin,
                    monero: monero::Network::Mainnet,
                },
                asb: BlockchainNetwork {
                    bitcoin: bitcoin::Network::Testnet,
                    monero: monero::Network::Stagenet,
                },
            },
        )
        .await;
    }

    struct SpotPriceTest {
        alice_swarm: Swarm<alice::swap_setup::Behaviour<TestRate>>,
        bob_swarm: Swarm<spot_price::Behaviour>,

        alice_peer_id: PeerId,
    }

    impl SpotPriceTest {
        pub async fn setup(values: AliceBehaviourValues) -> Self {
            let (mut alice_swarm, _, alice_peer_id) = new_swarm(|_, _| {
                Behaviour::new(
                    values.balance,
                    values.lock_fee,
                    values.min_buy,
                    values.max_buy,
                    values.env_config,
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

        pub fn construct_and_send_request(&mut self, btc_to_swap: bitcoin::Amount) {
            let request = spot_price::Request {
                btc: btc_to_swap,
                blockchain_network: BlockchainNetwork {
                    bitcoin: bitcoin::Network::Testnet,
                    monero: monero::Network::Stagenet,
                },
            };
            self.send_request(request);
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
                    alice::swap_setup::OutEvent::ExecutionSetupParams { btc, xmr, .. },
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
            alice_assert: alice::swap_setup::Error,
            bob_assert: bob::spot_price::Error,
        ) {
            match await_events_or_timeout(self.alice_swarm.next(), self.bob_swarm.next()).await {
                (
                    alice::swap_setup::OutEvent::Error { error, .. },
                    spot_price::OutEvent::Message { message, .. },
                ) => {
                    // TODO: Somehow make PartialEq work on Alice's spot_price::Error
                    match (alice_assert, error) {
                        (
                            alice::swap_setup::Error::BalanceTooLow {
                                balance: balance1,
                                buy: buy1,
                            },
                            alice::swap_setup::Error::BalanceTooLow {
                                balance: balance2,
                                buy: buy2,
                            },
                        ) => {
                            assert_eq!(balance1, balance2);
                            assert_eq!(buy1, buy2);
                        }
                        (
                            alice::swap_setup::Error::BlockchainNetworkMismatch {
                                cli: cli1,
                                asb: asb1,
                            },
                            alice::swap_setup::Error::BlockchainNetworkMismatch {
                                cli: cli2,
                                asb: asb2,
                            },
                        ) => {
                            assert_eq!(cli1, cli2);
                            assert_eq!(asb1, asb2);
                        }
                        (
                            alice::swap_setup::Error::AmountBelowMinimum { .. },
                            alice::swap_setup::Error::AmountBelowMinimum { .. },
                        )
                        | (
                            alice::swap_setup::Error::AmountAboveMaximum { .. },
                            alice::swap_setup::Error::AmountAboveMaximum { .. },
                        )
                        | (
                            alice::swap_setup::Error::LatestRateFetchFailed(_),
                            alice::swap_setup::Error::LatestRateFetchFailed(_),
                        )
                        | (
                            alice::swap_setup::Error::SellQuoteCalculationFailed(_),
                            alice::swap_setup::Error::SellQuoteCalculationFailed(_),
                        )
                        | (
                            alice::swap_setup::Error::ResumeOnlyMode,
                            alice::swap_setup::Error::ResumeOnlyMode,
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
        pub min_buy: bitcoin::Amount,
        pub max_buy: bitcoin::Amount,
        pub rate: TestRate, // 0.01
        pub resume_only: bool,
        pub env_config: env::Config,
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

        pub fn with_min_buy(mut self, min_buy: bitcoin::Amount) -> AliceBehaviourValues {
            self.min_buy = min_buy;
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

        pub fn with_env_config(mut self, env_config: env::Config) -> AliceBehaviourValues {
            self.env_config = env_config;
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
