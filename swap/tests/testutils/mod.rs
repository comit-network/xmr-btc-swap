use crate::testutils;
use bitcoin_harness::Bitcoind;
use futures::Future;
use get_port::get_port;
use libp2p::core::Multiaddr;
use monero_harness::{image, Monero};
use std::sync::Arc;
use swap::{
    bitcoin,
    config::Config,
    monero,
    protocol::{alice, alice::AliceState, bob, bob::BobState, StartingBalances},
    seed::Seed,
    SwapAmounts,
};
use tempfile::tempdir;
use testcontainers::{clients::Cli, Container};
use tracing_core::dispatcher::DefaultGuard;
use tracing_log::LogTracer;
use uuid::Uuid;

pub struct TestContext {
    swap_amounts: SwapAmounts,

    alice_swap_factory: alice::SwapFactory,
    bob_swap_factory: bob::SwapFactory,
}

impl TestContext {
    pub async fn new_swap_as_alice(&self) -> alice::Swap {
        let (swap, mut event_loop) = self
            .alice_swap_factory
            .new_swap_as_alice(self.swap_amounts)
            .await
            .unwrap();

        tokio::spawn(async move { event_loop.run().await });

        swap
    }

    pub async fn new_swap_as_bob(&self) -> bob::Swap {
        let (swap, event_loop) = self
            .bob_swap_factory
            .new_swap_as_bob(self.swap_amounts)
            .await
            .unwrap();

        tokio::spawn(async move { event_loop.run().await });

        swap
    }

    pub async fn recover_alice_from_db(&self) -> alice::Swap {
        let (swap, mut event_loop) = self
            .alice_swap_factory
            .recover_alice_from_db()
            .await
            .unwrap();

        tokio::spawn(async move { event_loop.run().await });

        swap
    }

    pub async fn recover_bob_from_db(&self) -> bob::Swap {
        let (swap, event_loop) = self.bob_swap_factory.resume().await.unwrap();

        tokio::spawn(async move { event_loop.run().await });

        swap
    }

    pub async fn assert_alice_redeemed(&self, state: AliceState) {
        assert!(matches!(state, AliceState::BtcRedeemed));

        let btc_balance_after_swap = self
            .alice_swap_factory
            .bitcoin_wallet
            .as_ref()
            .balance()
            .await
            .unwrap();
        assert_eq!(
            btc_balance_after_swap,
            self.alice_swap_factory.starting_balances.btc + self.swap_amounts.btc
                - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
        );

        let xmr_balance_after_swap = self
            .alice_swap_factory
            .monero_wallet
            .as_ref()
            .get_balance()
            .await
            .unwrap();
        assert!(
            xmr_balance_after_swap
                <= self.alice_swap_factory.starting_balances.xmr - self.swap_amounts.xmr
        );
    }

    pub async fn assert_alice_refunded(&self, state: AliceState) {
        assert!(matches!(state, AliceState::XmrRefunded));

        let btc_balance_after_swap = self
            .alice_swap_factory
            .bitcoin_wallet
            .as_ref()
            .balance()
            .await
            .unwrap();
        assert_eq!(
            btc_balance_after_swap,
            self.alice_swap_factory.starting_balances.btc
        );

        // Ensure that Alice's balance is refreshed as we use a newly created wallet
        self.alice_swap_factory
            .monero_wallet
            .as_ref()
            .inner
            .refresh()
            .await
            .unwrap();
        let xmr_balance_after_swap = self
            .alice_swap_factory
            .monero_wallet
            .as_ref()
            .get_balance()
            .await
            .unwrap();
        assert_eq!(xmr_balance_after_swap, self.swap_amounts.xmr);
    }

    pub async fn assert_alice_punished(&self, state: AliceState) {
        assert!(matches!(state, AliceState::BtcPunished));

        let btc_balance_after_swap = self
            .alice_swap_factory
            .bitcoin_wallet
            .as_ref()
            .balance()
            .await
            .unwrap();
        assert_eq!(
            btc_balance_after_swap,
            self.alice_swap_factory.starting_balances.btc + self.swap_amounts.btc
                - bitcoin::Amount::from_sat(2 * bitcoin::TX_FEE)
        );

        let xmr_balance_after_swap = self
            .alice_swap_factory
            .monero_wallet
            .as_ref()
            .get_balance()
            .await
            .unwrap();
        assert!(
            xmr_balance_after_swap
                <= self.alice_swap_factory.starting_balances.xmr - self.swap_amounts.xmr
        );
    }

    pub async fn assert_bob_redeemed(&self, state: BobState) {
        let lock_tx_id = if let BobState::XmrRedeemed { tx_lock_id } = state {
            tx_lock_id
        } else {
            panic!("Bob in unexpected state");
        };

        let lock_tx_bitcoin_fee = self
            .bob_swap_factory
            .bitcoin_wallet
            .transaction_fee(lock_tx_id)
            .await
            .unwrap();

        let btc_balance_after_swap = self
            .bob_swap_factory
            .bitcoin_wallet
            .as_ref()
            .balance()
            .await
            .unwrap();
        assert_eq!(
            btc_balance_after_swap,
            self.bob_swap_factory.starting_balances.btc
                - self.swap_amounts.btc
                - lock_tx_bitcoin_fee
        );

        // Ensure that Bob's balance is refreshed as we use a newly created wallet
        self.bob_swap_factory
            .monero_wallet
            .as_ref()
            .inner
            .refresh()
            .await
            .unwrap();
        let xmr_balance_after_swap = self
            .bob_swap_factory
            .monero_wallet
            .as_ref()
            .get_balance()
            .await
            .unwrap();
        assert_eq!(
            xmr_balance_after_swap,
            self.bob_swap_factory.starting_balances.xmr + self.swap_amounts.xmr
        );
    }

    pub async fn assert_bob_refunded(&self, state: BobState) {
        let lock_tx_id = if let BobState::BtcRefunded(state4) = state {
            state4.tx_lock_id()
        } else {
            panic!("Bob in unexpected state");
        };
        let lock_tx_bitcoin_fee = self
            .bob_swap_factory
            .bitcoin_wallet
            .transaction_fee(lock_tx_id)
            .await
            .unwrap();

        let btc_balance_after_swap = self
            .bob_swap_factory
            .bitcoin_wallet
            .as_ref()
            .balance()
            .await
            .unwrap();

        let alice_submitted_cancel = btc_balance_after_swap
            == self.bob_swap_factory.starting_balances.btc
                - lock_tx_bitcoin_fee
                - bitcoin::Amount::from_sat(bitcoin::TX_FEE);

        let bob_submitted_cancel = btc_balance_after_swap
            == self.bob_swap_factory.starting_balances.btc
                - lock_tx_bitcoin_fee
                - bitcoin::Amount::from_sat(2 * bitcoin::TX_FEE);

        // The cancel tx can be submitted by both Alice and Bob.
        // Since we cannot be sure who submitted it we have to assert accordingly
        assert!(alice_submitted_cancel || bob_submitted_cancel);

        let xmr_balance_after_swap = self
            .bob_swap_factory
            .monero_wallet
            .as_ref()
            .get_balance()
            .await
            .unwrap();
        assert_eq!(
            xmr_balance_after_swap,
            self.bob_swap_factory.starting_balances.xmr
        );
    }

    pub async fn assert_bob_punished(&self, state: BobState) {
        let lock_tx_id = if let BobState::BtcPunished { tx_lock_id } = state {
            tx_lock_id
        } else {
            panic!("Bob in unexpected state");
        };

        let lock_tx_bitcoin_fee = self
            .bob_swap_factory
            .bitcoin_wallet
            .transaction_fee(lock_tx_id)
            .await
            .unwrap();

        let btc_balance_after_swap = self
            .bob_swap_factory
            .bitcoin_wallet
            .as_ref()
            .balance()
            .await
            .unwrap();
        assert_eq!(
            btc_balance_after_swap,
            self.bob_swap_factory.starting_balances.btc
                - self.swap_amounts.btc
                - lock_tx_bitcoin_fee
        );

        let xmr_balance_after_swap = self
            .bob_swap_factory
            .monero_wallet
            .as_ref()
            .get_balance()
            .await
            .unwrap();
        assert_eq!(
            xmr_balance_after_swap,
            self.bob_swap_factory.starting_balances.xmr
        );
    }
}

pub async fn setup_test<T, F>(testfn: T)
where
    T: Fn(TestContext) -> F,
    F: Future<Output = ()>,
{
    let cli = Cli::default();

    let _guard = init_tracing();

    let (monero, containers) = testutils::init_containers(&cli).await;

    let swap_amounts = SwapAmounts {
        btc: bitcoin::Amount::from_sat(1_000_000),
        xmr: monero::Amount::from_piconero(1_000_000_000_000),
    };

    let config = Config::regtest();

    let alice_starting_balances = StartingBalances {
        xmr: swap_amounts.xmr * 10,
        btc: bitcoin::Amount::ZERO,
    };

    let port = get_port().expect("Failed to find a free port");

    let listen_address: Multiaddr = format!("/ip4/127.0.0.1/tcp/{}", port)
        .parse()
        .expect("failed to parse Alice's address");

    let (alice_bitcoin_wallet, alice_monero_wallet) = init_wallets(
        "alice",
        &containers.bitcoind,
        &monero,
        alice_starting_balances.clone(),
        config,
    )
    .await;

    let alice_swap_factory = alice::SwapFactory::new(
        Seed::random().unwrap(),
        config,
        Uuid::new_v4(),
        alice_bitcoin_wallet,
        alice_monero_wallet,
        alice_starting_balances,
        tempdir().unwrap().path().to_path_buf(),
        listen_address,
    )
    .await;

    let bob_starting_balances = StartingBalances {
        xmr: monero::Amount::ZERO,
        btc: swap_amounts.btc * 10,
    };

    let (bob_bitcoin_wallet, bob_monero_wallet) = init_wallets(
        "bob",
        &containers.bitcoind,
        &monero,
        bob_starting_balances.clone(),
        config,
    )
    .await;

    let bob_swap_factory = bob::SwapFactory::new(
        Seed::random().unwrap(),
        tempdir().unwrap().path().to_path_buf(),
        Uuid::new_v4(),
        bob_bitcoin_wallet,
        bob_monero_wallet,
        config,
        bob_starting_balances,
        alice_swap_factory.listen_address(),
        alice_swap_factory.peer_id(),
    );

    let test = TestContext {
        swap_amounts,
        alice_swap_factory,
        bob_swap_factory,
    };

    testfn(test).await
}

async fn init_containers(cli: &Cli) -> (Monero, Containers<'_>) {
    let bitcoind = Bitcoind::new(&cli, "0.19.1").unwrap();
    let _ = bitcoind.init(5).await;
    let (monero, monerods) = Monero::new(&cli, None, vec!["alice".to_string(), "bob".to_string()])
        .await
        .unwrap();

    (monero, Containers { bitcoind, monerods })
}

async fn init_wallets(
    name: &str,
    bitcoind: &Bitcoind<'_>,
    monero: &Monero,
    starting_balances: StartingBalances,
    config: Config,
) -> (Arc<bitcoin::Wallet>, Arc<monero::Wallet>) {
    monero
        .init(vec![(name, starting_balances.xmr.as_piconero())])
        .await
        .unwrap();

    let xmr_wallet = Arc::new(swap::monero::Wallet {
        inner: monero.wallet(name).unwrap().client(),
        network: config.monero_network,
    });

    let btc_wallet = Arc::new(
        swap::bitcoin::Wallet::new(name, bitcoind.node_url.clone(), config.bitcoin_network)
            .await
            .unwrap(),
    );

    if starting_balances.btc != bitcoin::Amount::ZERO {
        bitcoind
            .mint(
                btc_wallet.inner.new_address().await.unwrap(),
                starting_balances.btc,
            )
            .await
            .unwrap();
    }

    (btc_wallet, xmr_wallet)
}

// This is just to keep the containers alive
#[allow(dead_code)]
struct Containers<'a> {
    bitcoind: Bitcoind<'a>,
    monerods: Vec<Container<'a, Cli, image::Monero>>,
}

/// Utility function to initialize logging in the test environment.
/// Note that you have to keep the `_guard` in scope after calling in test:
///
/// ```rust
/// let _guard = init_tracing();
/// ```
fn init_tracing() -> DefaultGuard {
    // converts all log records into tracing events
    // Note: Make sure to initialize without unwrapping, otherwise this causes
    // trouble when running multiple tests.
    let _ = LogTracer::init();

    let global_filter = tracing::Level::WARN;
    let swap_filter = tracing::Level::DEBUG;
    let xmr_btc_filter = tracing::Level::DEBUG;
    let monero_harness_filter = tracing::Level::INFO;
    let bitcoin_harness_filter = tracing::Level::INFO;

    use tracing_subscriber::util::SubscriberInitExt as _;
    tracing_subscriber::fmt()
        .with_env_filter(format!(
            "{},swap={},xmr_btc={},monero_harness={},bitcoin_harness={}",
            global_filter,
            swap_filter,
            xmr_btc_filter,
            monero_harness_filter,
            bitcoin_harness_filter,
        ))
        .set_default()
}
