use bitcoin_harness::Bitcoind;
use libp2p::core::Multiaddr;
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

#[allow(clippy::too_many_arguments)]
pub async fn init_bob(
    alice_multiaddr: Multiaddr,
    bitcoind: &Bitcoind<'_>,
    monero: &Monero,
    btc_to_swap: bitcoin::Amount,
    btc_starting_balance: bitcoin::Amount,
    xmr_to_swap: xmr_btc::monero::Amount,
    xmr_stating_balance: xmr_btc::monero::Amount,
    config: Config,
) -> (
    BobState,
    bob::event_loop::EventLoop,
    bob::event_loop::EventLoopHandle,
    Arc<swap::bitcoin::Wallet>,
    Arc<swap::monero::Wallet>,
    Database,
) {
    let bob_btc_wallet = Arc::new(
        swap::bitcoin::Wallet::new("bob", bitcoind.node_url.clone(), config.bitcoin_network)
            .await
            .unwrap(),
    );
    bitcoind
        .mint(
            bob_btc_wallet.inner.new_address().await.unwrap(),
            btc_starting_balance,
        )
        .await
        .unwrap();

    monero
        .init(vec![("bob", xmr_stating_balance.as_piconero())])
        .await
        .unwrap();

    let bob_xmr_wallet = Arc::new(swap::monero::Wallet(monero.wallet("bob").unwrap().client()));

    let amounts = SwapAmounts {
        btc: btc_to_swap,
        xmr: xmr_to_swap,
    };

    let bob_db_dir = tempdir().unwrap();
    let bob_db = Database::open(bob_db_dir.path()).unwrap();
    let bob_behaviour = bob::Behaviour::default();
    let bob_transport = build(bob_behaviour.identity()).unwrap();

    let refund_address = bob_btc_wallet.new_address().await.unwrap();
    let state0 = xmr_btc::bob::State0::new(
        &mut OsRng,
        btc_to_swap,
        xmr_to_swap,
        config.bitcoin_refund_timelock,
        config.bitcoin_punish_timelock,
        refund_address,
    );
    let bob_state = BobState::Started {
        state0,
        amounts,
        addr: alice_multiaddr,
    };

    let (swarm_driver, swarm_handle) =
        bob::event_loop::EventLoop::new(bob_transport, bob_behaviour).unwrap();

    (
        bob_state,
        swarm_driver,
        swarm_handle,
        bob_btc_wallet,
        bob_xmr_wallet,
        bob_db,
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn init_alice(
    bitcoind: &Bitcoind<'_>,
    monero: &Monero,
    btc_to_swap: bitcoin::Amount,
    _btc_starting_balance: bitcoin::Amount,
    xmr_to_swap: xmr_btc::monero::Amount,
    xmr_starting_balance: xmr_btc::monero::Amount,
    listen: Multiaddr,
    config: Config,
) -> (
    AliceState,
    alice::event_loop::EventLoop,
    alice::event_loop::EventLoopHandle,
    Arc<swap::bitcoin::Wallet>,
    Arc<swap::monero::Wallet>,
) {
    monero
        .init(vec![("alice", xmr_starting_balance.as_piconero())])
        .await
        .unwrap();

    let alice_xmr_wallet = Arc::new(swap::monero::Wallet(
        monero.wallet("alice").unwrap().client(),
    ));

    let alice_btc_wallet = Arc::new(
        swap::bitcoin::Wallet::new("alice", bitcoind.node_url.clone(), config.bitcoin_network)
            .await
            .unwrap(),
    );

    let amounts = SwapAmounts {
        btc: btc_to_swap,
        xmr: xmr_to_swap,
    };

    let rng = &mut OsRng;
    let (alice_state, alice_behaviour) = {
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
            config.bitcoin_refund_timelock,
            config.bitcoin_punish_timelock,
            redeem_address,
            punish_address,
        );

        (
            AliceState::Started {
                amounts,
                state0: state0.clone(),
            },
            alice::Behaviour::new(state0),
        )
    };

    let alice_transport = build(alice_behaviour.identity()).unwrap();

    let (swarm_driver, handle) =
        alice::event_loop::EventLoop::new(alice_transport, alice_behaviour, listen).unwrap();

    (
        alice_state,
        swarm_driver,
        handle,
        alice_btc_wallet,
        alice_xmr_wallet,
    )
}

/// Returns Alice's and Bob's wallets, in this order
pub async fn setup_wallets(
    cli: &Cli,
    _init_btc_alice: bitcoin::Amount,
    init_xmr_alice: xmr_btc::monero::Amount,
    init_btc_bob: bitcoin::Amount,
    init_xmr_bob: xmr_btc::monero::Amount,
    config: Config,
) -> (
    bitcoin::Wallet,
    monero::Wallet,
    bitcoin::Wallet,
    monero::Wallet,
    Containers<'_>,
) {
    let bitcoind = Bitcoind::new(&cli, "0.19.1").unwrap();
    let _ = bitcoind.init(5).await;

    let alice_btc_wallet =
        swap::bitcoin::Wallet::new("alice", bitcoind.node_url.clone(), config.bitcoin_network)
            .await
            .unwrap();
    let bob_btc_wallet =
        swap::bitcoin::Wallet::new("bob", bitcoind.node_url.clone(), config.bitcoin_network)
            .await
            .unwrap();
    bitcoind
        .mint(
            bob_btc_wallet.inner.new_address().await.unwrap(),
            init_btc_bob,
        )
        .await
        .unwrap();

    let (monero, monerods) = Monero::new(&cli, None, vec!["alice".to_string(), "bob".to_string()])
        .await
        .unwrap();
    monero
        .init(vec![
            ("alice", init_xmr_alice.as_piconero()),
            ("bob", init_xmr_bob.as_piconero()),
        ])
        .await
        .unwrap();

    let alice_xmr_wallet = swap::monero::Wallet(monero.wallet("alice").unwrap().client());
    let bob_xmr_wallet = swap::monero::Wallet(monero.wallet("bob").unwrap().client());

    (
        alice_btc_wallet,
        alice_xmr_wallet,
        bob_btc_wallet,
        bob_xmr_wallet,
        Containers { bitcoind, monerods },
    )
}

// This is just to keep the containers alive
#[allow(dead_code)]
pub struct Containers<'a> {
    bitcoind: Bitcoind<'a>,
    monerods: Vec<Container<'a, Cli, image::Monero>>,
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
            "{},swap={},xmr-btc={},monero_harness={},bitcoin_harness={}",
            global_filter,
            swap_filter,
            xmr_btc_filter,
            monero_harness_filter,
            bitcoin_harness_filter,
        ))
        .set_default()
}
