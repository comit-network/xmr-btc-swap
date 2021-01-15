use crate::testutils;
use bitcoin_harness::Bitcoind;
use futures::Future;
use get_port::get_port;
use libp2p::{core::Multiaddr, PeerId};
use monero_harness::{image, Monero};
use rand::rngs::OsRng;
use std::{path::PathBuf, sync::Arc};
use swap::{
    bitcoin,
    config::Config,
    database::Database,
    monero, network,
    network::transport::build,
    protocol::{alice, alice::AliceState, bob, bob::BobState},
    seed::Seed,
    SwapAmounts,
};
use tempfile::tempdir;
use testcontainers::{clients::Cli, Container};
use tracing_core::dispatcher::DefaultGuard;
use tracing_log::LogTracer;
use uuid::Uuid;

pub struct Alice {
    pub event_loop_handle: alice::EventLoopHandle,
    pub btc_wallet: Arc<bitcoin::Wallet>,
    pub xmr_wallet: Arc<monero::Wallet>,
    pub config: Config,
    pub db: Database,

    pub state: AliceState,

    pub xmr_starting_balance: monero::Amount,
    pub btc_starting_balance: bitcoin::Amount,

    // test context (state we have to keep to simulate restart)
    pub swap_id: Uuid,
}

pub struct Bob {
    pub state: BobState,
    pub event_loop_handle: bob::EventLoopHandle,
    pub db: Database,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub swap_id: Uuid,
    pub btc_starting_balance: bitcoin::Amount,
    pub xmr_starting_balance: monero::Amount,
}

pub struct AliceFactory {
    listen_address: Multiaddr,
    peer_id: PeerId,

    seed: Seed,
    db_path: PathBuf,
    swap_id: Uuid,

    // Stuff that should probably not be in here...
    swap_amounts: SwapAmounts,
    btc_wallet: Arc<bitcoin::Wallet>,
    xmr_wallet: Arc<monero::Wallet>,
    config: Config,
    xmr_starting_balance: monero::Amount,
    btc_starting_balance: bitcoin::Amount,
}

impl AliceFactory {
    pub fn peer_id(&self) -> PeerId {
        self.peer_id.clone()
    }

    pub fn listen_address(&self) -> Multiaddr {
        self.listen_address.clone()
    }

    pub async fn new(
        config: Config,
        swap_amounts: SwapAmounts,
        swap_id: Uuid,
        monero: &Monero,
        bitcoind: &Bitcoind<'_>,
        xmr_starting_balance: monero::Amount,
        btc_starting_balance: bitcoin::Amount,
    ) -> Self {
        let port = get_port().expect("Failed to find a free port");

        let listen_address: Multiaddr = format!("/ip4/127.0.0.1/tcp/{}", port)
            .parse()
            .expect("failed to parse Alice's address");

        let seed = Seed::random().unwrap();

        let db_path = tempdir().unwrap().path().to_path_buf();

        let alice_xmr_starting_balance = swap_amounts.xmr * 10;
        let (btc_wallet, xmr_wallet) = init_wallets(
            "alice",
            bitcoind,
            monero,
            None,
            Some(alice_xmr_starting_balance),
            config,
        )
        .await;

        // TODO: This should be done by changing the production code
        let network_seed = network::Seed::new(seed);
        let identity = network_seed.derive_libp2p_identity();
        let peer_id = PeerId::from(identity.public());

        Self {
            seed,
            db_path,
            listen_address,
            peer_id,
            swap_id,
            swap_amounts,
            btc_wallet,
            xmr_wallet,
            config,
            xmr_starting_balance,
            btc_starting_balance,
        }
    }

    pub async fn new_alice(&self) -> Alice {
        let initial_state = init_alice_state(
            self.swap_amounts.btc,
            self.swap_amounts.xmr,
            self.btc_wallet.clone(),
            self.config,
        )
        .await;

        let (mut event_loop, event_loop_handle) =
            init_alice_event_loop(self.listen_address.clone(), self.seed);

        tokio::spawn(async move { event_loop.run().await });

        let db = Database::open(self.db_path.as_path()).unwrap();

        Alice {
            event_loop_handle,
            btc_wallet: self.btc_wallet.clone(),
            xmr_wallet: self.xmr_wallet.clone(),
            config: self.config,
            db,
            state: initial_state,
            xmr_starting_balance: self.xmr_starting_balance,
            btc_starting_balance: self.btc_starting_balance,
            swap_id: self.swap_id,
        }
    }

    pub async fn recover_alice_from_db(&self) -> Alice {
        // TODO: "simulated restart" issues:
        //  - create new wallets instead of reusing (hard because of container
        //    lifetimes)
        //  - consider aborting the old event loop (currently just keeps running)

        // reopen the existing database
        let db = Database::open(self.db_path.clone().as_path()).unwrap();

        let resume_state =
            if let swap::database::Swap::Alice(state) = db.get_state(self.swap_id).unwrap() {
                state.into()
            } else {
                unreachable!()
            };

        let (mut event_loop, event_loop_handle) =
            init_alice_event_loop(self.listen_address.clone(), self.seed);

        tokio::spawn(async move { event_loop.run().await });

        Alice {
            state: resume_state,
            event_loop_handle,
            btc_wallet: self.btc_wallet.clone(),
            xmr_wallet: self.xmr_wallet.clone(),
            config: self.config,
            swap_id: self.swap_id,
            db,
            xmr_starting_balance: self.xmr_starting_balance,
            btc_starting_balance: self.btc_starting_balance,
        }
    }
}

pub async fn test<T, F>(testfn: T)
where
    T: Fn(AliceFactory, Bob, SwapAmounts) -> F,
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

    let alice_factory = AliceFactory::new(
        config,
        swap_amounts,
        Uuid::new_v4(),
        &monero,
        &containers.bitcoind,
        swap_amounts.xmr * 10,
        bitcoin::Amount::ZERO,
    )
    .await;

    let bob_btc_starting_balance = swap_amounts.btc * 10;

    let (bob_btc_wallet, bob_xmr_wallet) = init_wallets(
        "bob",
        &containers.bitcoind,
        &monero,
        Some(bob_btc_starting_balance),
        None,
        config,
    )
    .await;

    let bob_state = init_bob_state(
        swap_amounts.btc,
        swap_amounts.xmr,
        bob_btc_wallet.clone(),
        config,
    )
    .await;

    let (bob_event_loop, bob_event_loop_handle) =
        init_bob_event_loop(alice_factory.peer_id(), alice_factory.listen_address());

    let bob_db_dir = tempdir().unwrap();
    let bob_db = Database::open(bob_db_dir.path()).unwrap();

    tokio::spawn(async move { bob_event_loop.run().await });

    let bob = Bob {
        state: bob_state,
        event_loop_handle: bob_event_loop_handle,
        db: bob_db,
        bitcoin_wallet: bob_btc_wallet,
        monero_wallet: bob_xmr_wallet,
        swap_id: Uuid::new_v4(),
        xmr_starting_balance: monero::Amount::ZERO,
        btc_starting_balance: bob_btc_starting_balance,
    };

    testfn(alice_factory, bob, swap_amounts).await
}

pub async fn init_containers(cli: &Cli) -> (Monero, Containers<'_>) {
    let bitcoind = Bitcoind::new(&cli, "0.19.1").unwrap();
    let _ = bitcoind.init(5).await;
    let (monero, monerods) = Monero::new(&cli, None, vec!["alice".to_string(), "bob".to_string()])
        .await
        .unwrap();

    (monero, Containers { bitcoind, monerods })
}

pub async fn init_wallets(
    name: &str,
    bitcoind: &Bitcoind<'_>,
    monero: &Monero,
    btc_starting_balance: Option<bitcoin::Amount>,
    xmr_starting_balance: Option<monero::Amount>,
    config: Config,
) -> (Arc<bitcoin::Wallet>, Arc<monero::Wallet>) {
    match xmr_starting_balance {
        Some(amount) => {
            monero
                .init(vec![(name, amount.as_piconero())])
                .await
                .unwrap();
        }
        None => {
            monero
                .init(vec![(name, monero::Amount::ZERO.as_piconero())])
                .await
                .unwrap();
        }
    };

    let xmr_wallet = Arc::new(swap::monero::Wallet {
        inner: monero.wallet(name).unwrap().client(),
        network: config.monero_network,
    });

    let btc_wallet = Arc::new(
        swap::bitcoin::Wallet::new(name, bitcoind.node_url.clone(), config.bitcoin_network)
            .await
            .unwrap(),
    );

    if let Some(amount) = btc_starting_balance {
        bitcoind
            .mint(btc_wallet.inner.new_address().await.unwrap(), amount)
            .await
            .unwrap();
    }

    (btc_wallet, xmr_wallet)
}

pub async fn init_alice_state(
    btc_to_swap: bitcoin::Amount,
    xmr_to_swap: monero::Amount,
    alice_btc_wallet: Arc<bitcoin::Wallet>,
    config: Config,
) -> AliceState {
    let rng = &mut OsRng;

    let amounts = SwapAmounts {
        btc: btc_to_swap,
        xmr: xmr_to_swap,
    };

    let a = bitcoin::SecretKey::new_random(rng);
    let s_a = cross_curve_dleq::Scalar::random(rng);
    let v_a = monero::PrivateViewKey::new_random(rng);
    let redeem_address = alice_btc_wallet.as_ref().new_address().await.unwrap();
    let punish_address = redeem_address.clone();
    let state0 = alice::State0::new(
        a,
        s_a,
        v_a,
        amounts.btc,
        amounts.xmr,
        config.bitcoin_cancel_timelock,
        config.bitcoin_punish_timelock,
        redeem_address,
        punish_address,
    );

    AliceState::Started { amounts, state0 }
}

pub fn init_alice_event_loop(
    listen: Multiaddr,
    seed: Seed,
) -> (
    alice::event_loop::EventLoop,
    alice::event_loop::EventLoopHandle,
) {
    let alice_behaviour = alice::Behaviour::new(network::Seed::new(seed));
    let alice_transport = build(alice_behaviour.identity()).unwrap();
    alice::event_loop::EventLoop::new(alice_transport, alice_behaviour, listen).unwrap()
}

#[allow(clippy::too_many_arguments)]
pub async fn init_alice(
    bitcoind: &Bitcoind<'_>,
    monero: &Monero,
    btc_to_swap: bitcoin::Amount,
    xmr_to_swap: monero::Amount,
    xmr_starting_balance: monero::Amount,
    listen: Multiaddr,
    config: Config,
    seed: Seed,
) -> (
    AliceState,
    alice::event_loop::EventLoop,
    alice::event_loop::EventLoopHandle,
    Arc<swap::bitcoin::Wallet>,
    Arc<swap::monero::Wallet>,
    Database,
) {
    let (alice_btc_wallet, alice_xmr_wallet) = init_wallets(
        "alice",
        bitcoind,
        monero,
        None,
        Some(xmr_starting_balance),
        config,
    )
    .await;

    let alice_start_state =
        init_alice_state(btc_to_swap, xmr_to_swap, alice_btc_wallet.clone(), config).await;

    let (event_loop, event_loop_handle) = init_alice_event_loop(listen, seed);

    let alice_db_datadir = tempdir().unwrap();
    let alice_db = Database::open(alice_db_datadir.path()).unwrap();

    (
        alice_start_state,
        event_loop,
        event_loop_handle,
        alice_btc_wallet,
        alice_xmr_wallet,
        alice_db,
    )
}

pub async fn init_bob_state(
    btc_to_swap: bitcoin::Amount,
    xmr_to_swap: monero::Amount,
    bob_btc_wallet: Arc<bitcoin::Wallet>,
    config: Config,
) -> BobState {
    let amounts = SwapAmounts {
        btc: btc_to_swap,
        xmr: xmr_to_swap,
    };

    let refund_address = bob_btc_wallet.new_address().await.unwrap();
    let state0 = bob::State0::new(
        &mut OsRng,
        btc_to_swap,
        xmr_to_swap,
        config.bitcoin_cancel_timelock,
        config.bitcoin_punish_timelock,
        refund_address,
        config.monero_finality_confirmations,
    );

    BobState::Started { state0, amounts }
}

pub fn init_bob_event_loop(
    alice_peer_id: PeerId,
    alice_addr: Multiaddr,
) -> (bob::event_loop::EventLoop, bob::event_loop::EventLoopHandle) {
    let seed = Seed::random().unwrap();
    let bob_behaviour = bob::Behaviour::new(network::Seed::new(seed));
    let bob_transport = build(bob_behaviour.identity()).unwrap();
    bob::event_loop::EventLoop::new(bob_transport, bob_behaviour, alice_peer_id, alice_addr)
        .unwrap()
}

#[allow(clippy::too_many_arguments)]
pub async fn init_bob(
    alice_multiaddr: Multiaddr,
    alice_peer_id: PeerId,
    bitcoind: &Bitcoind<'_>,
    monero: &Monero,
    btc_to_swap: bitcoin::Amount,
    btc_starting_balance: bitcoin::Amount,
    xmr_to_swap: monero::Amount,
    config: Config,
) -> (
    BobState,
    bob::event_loop::EventLoop,
    bob::event_loop::EventLoopHandle,
    Arc<swap::bitcoin::Wallet>,
    Arc<swap::monero::Wallet>,
    Database,
) {
    let (bob_btc_wallet, bob_xmr_wallet) = init_wallets(
        "bob",
        bitcoind,
        monero,
        Some(btc_starting_balance),
        None,
        config,
    )
    .await;

    let bob_state = init_bob_state(btc_to_swap, xmr_to_swap, bob_btc_wallet.clone(), config).await;

    let (event_loop, event_loop_handle) = init_bob_event_loop(alice_peer_id, alice_multiaddr);

    let bob_db_dir = tempdir().unwrap();
    let bob_db = Database::open(bob_db_dir.path()).unwrap();

    (
        bob_state,
        event_loop,
        event_loop_handle,
        bob_btc_wallet,
        bob_xmr_wallet,
        bob_db,
    )
}

// This is just to keep the containers alive
#[allow(dead_code)]
pub struct Containers<'a> {
    pub bitcoind: Bitcoind<'a>,
    pub monerods: Vec<Container<'a, Cli, image::Monero>>,
}

/// Utility function to initialize logging in the test environment.
/// Note that you have to keep the `_guard` in scope after calling in test:
///
/// ```rust
/// let _guard = init_tracing();
/// ```
pub fn init_tracing() -> DefaultGuard {
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
