use crate::testutils;
use bitcoin_harness::Bitcoind;
use futures::Future;
use get_port::get_port;
use libp2p::{core::Multiaddr, PeerId};
use monero_harness::{image, Monero};
use std::{path::PathBuf, sync::Arc};
use swap::{
    bitcoin,
    config::Config,
    monero,
    protocol::{alice, alice::AliceState, bob, bob::BobState, SwapAmounts},
    seed::Seed,
};
use tempfile::tempdir;
use testcontainers::{clients::Cli, Container};
use tracing_core::dispatcher::DefaultGuard;
use tracing_log::LogTracer;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StartingBalances {
    pub xmr: monero::Amount,
    pub btc: bitcoin::Amount,
}

struct AliceParams {
    seed: Seed,
    config: Config,
    swap_id: Uuid,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    db_path: PathBuf,
    listen_address: Multiaddr,
}

impl AliceParams {
    pub async fn builder(&self) -> alice::Builder {
        alice::Builder::new(
            self.seed,
            self.config,
            self.swap_id,
            self.bitcoin_wallet.clone(),
            self.monero_wallet.clone(),
            self.db_path.clone(),
            self.listen_address.clone(),
        )
        .await
    }

    async fn peer_id(&self) -> PeerId {
        self.builder().await.peer_id()
    }
}

struct BobParams {
    seed: Seed,
    db_path: PathBuf,
    swap_id: Uuid,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    alice_address: Multiaddr,
    alice_peer_id: PeerId,
    config: Config,
}

impl BobParams {
    pub fn builder(&self) -> bob::Builder {
        bob::Builder::new(
            self.seed,
            self.db_path.clone(),
            self.swap_id,
            self.bitcoin_wallet.clone(),
            self.monero_wallet.clone(),
            self.alice_address.clone(),
            self.alice_peer_id.clone(),
            self.config,
        )
    }
}

pub struct TestContext {
    swap_amounts: SwapAmounts,

    alice_params: AliceParams,
    alice_starting_balances: StartingBalances,
    alice_bitcoin_wallet: Arc<bitcoin::Wallet>,
    alice_monero_wallet: Arc<monero::Wallet>,

    bob_params: BobParams,
    bob_starting_balances: StartingBalances,
    bob_bitcoin_wallet: Arc<bitcoin::Wallet>,
    bob_monero_wallet: Arc<monero::Wallet>,
}

impl TestContext {
    pub async fn new_swap_as_alice(&mut self) -> alice::Swap {
        let (swap, mut event_loop) = self
            .alice_params
            .builder()
            .await
            .with_init_params(self.swap_amounts)
            .build()
            .await
            .unwrap();

        tokio::spawn(async move { event_loop.run().await });

        swap
    }

    pub async fn new_swap_as_bob(&mut self) -> bob::Swap {
        let (swap, event_loop) = self
            .bob_params
            .builder()
            .with_init_params(self.swap_amounts)
            .build()
            .await
            .unwrap();

        tokio::spawn(async move { event_loop.run().await });

        swap
    }

    pub async fn recover_alice_from_db(&mut self) -> alice::Swap {
        let (swap, mut event_loop) = self.alice_params.builder().await.build().await.unwrap();

        tokio::spawn(async move { event_loop.run().await });

        swap
    }

    pub async fn recover_bob_from_db(&mut self) -> bob::Swap {
        let (swap, event_loop) = self.bob_params.builder().build().await.unwrap();

        tokio::spawn(async move { event_loop.run().await });

        swap
    }

    pub async fn assert_alice_redeemed(&self, state: AliceState) {
        assert!(matches!(state, AliceState::BtcRedeemed));

        let btc_balance_after_swap = self.alice_bitcoin_wallet.as_ref().balance().await.unwrap();
        assert_eq!(
            btc_balance_after_swap,
            self.alice_starting_balances.btc + self.swap_amounts.btc
                - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
        );

        let xmr_balance_after_swap = self
            .alice_monero_wallet
            .as_ref()
            .get_balance()
            .await
            .unwrap();
        assert!(xmr_balance_after_swap <= self.alice_starting_balances.xmr - self.swap_amounts.xmr);
    }

    pub async fn assert_alice_refunded(&self, state: AliceState) {
        assert!(matches!(state, AliceState::XmrRefunded));

        let btc_balance_after_swap = self.alice_bitcoin_wallet.as_ref().balance().await.unwrap();
        assert_eq!(btc_balance_after_swap, self.alice_starting_balances.btc);

        // Ensure that Alice's balance is refreshed as we use a newly created wallet
        self.alice_monero_wallet
            .as_ref()
            .inner
            .refresh()
            .await
            .unwrap();
        let xmr_balance_after_swap = self
            .alice_monero_wallet
            .as_ref()
            .get_balance()
            .await
            .unwrap();
        assert_eq!(xmr_balance_after_swap, self.swap_amounts.xmr);
    }

    pub async fn assert_alice_punished(&self, state: AliceState) {
        assert!(matches!(state, AliceState::BtcPunished));

        let btc_balance_after_swap = self.alice_bitcoin_wallet.as_ref().balance().await.unwrap();
        assert_eq!(
            btc_balance_after_swap,
            self.alice_starting_balances.btc + self.swap_amounts.btc
                - bitcoin::Amount::from_sat(2 * bitcoin::TX_FEE)
        );

        let xmr_balance_after_swap = self
            .alice_monero_wallet
            .as_ref()
            .get_balance()
            .await
            .unwrap();
        assert!(xmr_balance_after_swap <= self.alice_starting_balances.xmr - self.swap_amounts.xmr);
    }

    pub async fn assert_bob_redeemed(&self, state: BobState) {
        let lock_tx_id = if let BobState::XmrRedeemed { tx_lock_id } = state {
            tx_lock_id
        } else {
            panic!("Bob in unexpected state");
        };

        let lock_tx_bitcoin_fee = self
            .bob_bitcoin_wallet
            .transaction_fee(lock_tx_id)
            .await
            .unwrap();

        let btc_balance_after_swap = self.bob_bitcoin_wallet.as_ref().balance().await.unwrap();
        assert_eq!(
            btc_balance_after_swap,
            self.bob_starting_balances.btc - self.swap_amounts.btc - lock_tx_bitcoin_fee
        );

        // Ensure that Bob's balance is refreshed as we use a newly created wallet
        self.bob_monero_wallet
            .as_ref()
            .inner
            .refresh()
            .await
            .unwrap();
        let xmr_balance_after_swap = self.bob_monero_wallet.as_ref().get_balance().await.unwrap();
        assert_eq!(
            xmr_balance_after_swap,
            self.bob_starting_balances.xmr + self.swap_amounts.xmr
        );
    }

    pub async fn assert_bob_refunded(&self, state: BobState) {
        let lock_tx_id = if let BobState::BtcRefunded(state4) = state {
            state4.tx_lock_id()
        } else {
            panic!("Bob in unexpected state");
        };
        let lock_tx_bitcoin_fee = self
            .bob_bitcoin_wallet
            .transaction_fee(lock_tx_id)
            .await
            .unwrap();

        let btc_balance_after_swap = self.bob_bitcoin_wallet.as_ref().balance().await.unwrap();

        let alice_submitted_cancel = btc_balance_after_swap
            == self.bob_starting_balances.btc
                - lock_tx_bitcoin_fee
                - bitcoin::Amount::from_sat(bitcoin::TX_FEE);

        let bob_submitted_cancel = btc_balance_after_swap
            == self.bob_starting_balances.btc
                - lock_tx_bitcoin_fee
                - bitcoin::Amount::from_sat(2 * bitcoin::TX_FEE);

        // The cancel tx can be submitted by both Alice and Bob.
        // Since we cannot be sure who submitted it we have to assert accordingly
        assert!(alice_submitted_cancel || bob_submitted_cancel);

        let xmr_balance_after_swap = self.bob_monero_wallet.as_ref().get_balance().await.unwrap();
        assert_eq!(xmr_balance_after_swap, self.bob_starting_balances.xmr);
    }

    pub async fn assert_bob_punished(&self, state: BobState) {
        let lock_tx_id = if let BobState::BtcPunished { tx_lock_id } = state {
            tx_lock_id
        } else {
            panic!("Bob in unexpected state");
        };

        let lock_tx_bitcoin_fee = self
            .bob_bitcoin_wallet
            .transaction_fee(lock_tx_id)
            .await
            .unwrap();

        let btc_balance_after_swap = self.bob_bitcoin_wallet.as_ref().balance().await.unwrap();
        assert_eq!(
            btc_balance_after_swap,
            self.bob_starting_balances.btc - self.swap_amounts.btc - lock_tx_bitcoin_fee
        );

        let xmr_balance_after_swap = self.bob_monero_wallet.as_ref().get_balance().await.unwrap();
        assert_eq!(xmr_balance_after_swap, self.bob_starting_balances.xmr);
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

    let alice_params = AliceParams {
        seed: Seed::random().unwrap(),
        config,
        swap_id: Uuid::new_v4(),
        bitcoin_wallet: alice_bitcoin_wallet.clone(),
        monero_wallet: alice_monero_wallet.clone(),
        db_path: tempdir().unwrap().path().to_path_buf(),
        listen_address,
    };

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

    let bob_params = BobParams {
        seed: Seed::random().unwrap(),
        db_path: tempdir().unwrap().path().to_path_buf(),
        swap_id: Uuid::new_v4(),
        bitcoin_wallet: bob_bitcoin_wallet.clone(),
        monero_wallet: bob_monero_wallet.clone(),
        alice_address: alice_params.listen_address.clone(),
        alice_peer_id: alice_params.peer_id().await,
        config,
    };

    let test = TestContext {
        swap_amounts,
        alice_params,
        alice_starting_balances,
        alice_bitcoin_wallet,
        alice_monero_wallet,
        bob_params,
        bob_starting_balances,
        bob_bitcoin_wallet,
        bob_monero_wallet,
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

pub mod alice_run_until {
    use swap::protocol::alice::AliceState;

    pub fn is_xmr_locked(state: &AliceState) -> bool {
        matches!(
            state,
            AliceState::XmrLocked{..}
        )
    }

    pub fn is_encsig_learned(state: &AliceState) -> bool {
        matches!(
            state,
            AliceState::EncSigLearned{..}
        )
    }
}

pub mod bob_run_until {
    use swap::protocol::bob::BobState;

    pub fn is_btc_locked(state: &BobState) -> bool {
        matches!(state, BobState::BtcLocked(..))
    }

    pub fn is_lock_proof_received(state: &BobState) -> bool {
        matches!(state, BobState::XmrLockProofReceived { .. })
    }

    pub fn is_xmr_locked(state: &BobState) -> bool {
        matches!(state, BobState::XmrLocked(..))
    }

    pub fn is_encsig_sent(state: &BobState) -> bool {
        matches!(state, BobState::EncSigSent(..))
    }
}
