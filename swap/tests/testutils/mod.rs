use bitcoin_harness::Bitcoind;
use libp2p::{core::Multiaddr, PeerId};
use monero_harness::{image, Monero};
use rand::rngs::OsRng;
use std::sync::Arc;
use swap::{
    alice, alice::swap::AliceState, bitcoin, bob, bob::swap::BobState, monero,
    network::transport::build, storage::Database, SwapAmounts,
};
use tempfile::tempdir;
use testcontainers::{clients::Cli, Container};
use tracing_core::dispatcher::DefaultGuard;
use tracing_log::LogTracer;
use xmr_btc::{alice::State0, config::Config, cross_curve_dleq};

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
    btc_starting_balance: Option<xmr_btc::bitcoin::Amount>,
    xmr_starting_balance: Option<xmr_btc::monero::Amount>,
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

    let xmr_wallet = Arc::new(swap::monero::Wallet(monero.wallet(name).unwrap().client()));

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

    let a = crate::bitcoin::SecretKey::new_random(rng);
    let s_a = cross_curve_dleq::Scalar::random(rng);
    let v_a = xmr_btc::monero::PrivateViewKey::new_random(rng);
    let redeem_address = alice_btc_wallet.as_ref().new_address().await.unwrap();
    let punish_address = redeem_address.clone();
    let state0 = State0::new(
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
) -> (
    alice::event_loop::EventLoop,
    alice::event_loop::EventLoopHandle,
) {
    let alice_behaviour = alice::Behaviour::default();
    let alice_transport = build(alice_behaviour.identity()).unwrap();

    alice::event_loop::EventLoop::new(alice_transport, alice_behaviour, listen).unwrap()
}

#[allow(clippy::too_many_arguments)]
pub async fn init_alice(
    bitcoind: &Bitcoind<'_>,
    monero: &Monero,
    btc_to_swap: bitcoin::Amount,
    xmr_to_swap: monero::Amount,
    xmr_starting_balance: xmr_btc::monero::Amount,
    listen: Multiaddr,
    config: Config,
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

    let (event_loop, event_loop_handle) = init_alice_event_loop(listen);

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
    xmr_to_swap: xmr_btc::monero::Amount,
    bob_btc_wallet: Arc<bitcoin::Wallet>,
    config: Config,
) -> BobState {
    let amounts = SwapAmounts {
        btc: btc_to_swap,
        xmr: xmr_to_swap,
    };

    let refund_address = bob_btc_wallet.new_address().await.unwrap();
    let state0 = xmr_btc::bob::State0::new(
        &mut OsRng,
        btc_to_swap,
        xmr_to_swap,
        config.bitcoin_cancel_timelock,
        config.bitcoin_punish_timelock,
        refund_address,
    );

    BobState::Started { state0, amounts }
}

pub fn init_bob_event_loop(
    alice_peer_id: PeerId,
    alice_addr: Multiaddr,
) -> (bob::event_loop::EventLoop, bob::event_loop::EventLoopHandle) {
    let bob_behaviour = bob::Behaviour::default();
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
    xmr_to_swap: xmr_btc::monero::Amount,
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
