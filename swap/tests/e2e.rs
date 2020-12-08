use bitcoin_harness::Bitcoind;
use futures::future::try_join;
use libp2p::{Multiaddr, PeerId};
use monero_harness::Monero;
use rand::rngs::OsRng;
use std::sync::Arc;
use swap::{
    alice, alice::swap::AliceState, bob, bob::swap::BobState, network::transport::build,
    storage::Database, SwapAmounts, PUNISH_TIMELOCK, REFUND_TIMELOCK,
};
use tempfile::tempdir;
use testcontainers::clients::Cli;
use uuid::Uuid;
use xmr_btc::{bitcoin, config::Config, cross_curve_dleq};

/// Run the following tests with RUST_MIN_STACK=10000000

#[tokio::test]
async fn happy_path() {
    init_tracing();
    let cli = Cli::default();
    let bitcoind = Bitcoind::new(&cli, "0.19.1").unwrap();
    let _ = bitcoind.init(5).await;
    let (monero, _container) =
        Monero::new(&cli, None, vec!["alice".to_string(), "bob".to_string()])
            .await
            .unwrap();

    let btc_to_swap = bitcoin::Amount::from_sat(1_000_000);
    let btc_alice = bitcoin::Amount::ZERO;
    let btc_bob = btc_to_swap * 10;

    // this xmr value matches the logic of alice::calculate_amounts i.e. btc *
    // 10_000 * 100
    let xmr_to_swap = xmr_btc::monero::Amount::from_piconero(1_000_000_000_000);
    let xmr_alice = xmr_to_swap * 10;
    let xmr_bob = xmr_btc::monero::Amount::from_piconero(0);

    let alice_multiaddr: Multiaddr = "/ip4/127.0.0.1/tcp/9876"
        .parse()
        .expect("failed to parse Alice's address");

    let (alice_state, alice_swarm, alice_btc_wallet, alice_xmr_wallet, alice_peer_id) = init_alice(
        &bitcoind,
        &monero,
        btc_to_swap,
        btc_alice,
        xmr_to_swap,
        xmr_alice,
        alice_multiaddr.clone(),
    )
    .await;

    let (bob_state, bob_swarm, bob_btc_wallet, bob_xmr_wallet, bob_db) = init_bob(
        alice_multiaddr,
        alice_peer_id,
        &bitcoind,
        &monero,
        btc_to_swap,
        btc_bob,
        xmr_to_swap,
        xmr_bob,
    )
    .await;

    let alice_swap = alice::swap::swap(
        alice_state,
        alice_swarm,
        alice_btc_wallet.clone(),
        alice_xmr_wallet.clone(),
        Config::regtest(),
    );

    let bob_swap = bob::swap::swap(
        bob_state,
        bob_swarm,
        bob_db,
        bob_btc_wallet.clone(),
        bob_xmr_wallet.clone(),
        OsRng,
        Uuid::new_v4(),
    );

    try_join(alice_swap, bob_swap).await.unwrap();

    let btc_alice_final = alice_btc_wallet.as_ref().balance().await.unwrap();
    let btc_bob_final = bob_btc_wallet.as_ref().balance().await.unwrap();

    let xmr_alice_final = alice_xmr_wallet.as_ref().get_balance().await.unwrap();

    bob_xmr_wallet.as_ref().0.refresh().await.unwrap();
    let xmr_bob_final = bob_xmr_wallet.as_ref().get_balance().await.unwrap();

    assert_eq!(
        btc_alice_final,
        btc_alice + btc_to_swap - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
    );
    assert!(btc_bob_final <= btc_bob - btc_to_swap);

    assert!(xmr_alice_final <= xmr_alice - xmr_to_swap);
    assert_eq!(xmr_bob_final, xmr_bob + xmr_to_swap);
}

/// Bob locks Btc and Alice locks Xmr. Bob does not act; he fails to send Alice
/// the encsig and fail to refund or redeem. Alice punishes.
#[tokio::test]
async fn alice_punishes_if_bob_never_acts_after_fund() {
    init_tracing();
    let cli = Cli::default();
    let bitcoind = Bitcoind::new(&cli, "0.19.1").unwrap();
    let _ = bitcoind.init(5).await;
    let (monero, _container) =
        Monero::new(&cli, None, vec!["alice".to_string(), "bob".to_string()])
            .await
            .unwrap();

    let btc_to_swap = bitcoin::Amount::from_sat(1_000_000);
    let xmr_to_swap = xmr_btc::monero::Amount::from_piconero(1_000_000_000_000);

    let bob_btc_starting_balance = btc_to_swap * 10;
    let bob_xmr_starting_balance = xmr_btc::monero::Amount::from_piconero(0);

    let alice_btc_starting_balance = bitcoin::Amount::ZERO;
    let alice_xmr_starting_balance = xmr_to_swap * 10;

    let alice_multiaddr: Multiaddr = "/ip4/127.0.0.1/tcp/9877"
        .parse()
        .expect("failed to parse Alice's address");

    let (alice_state, alice_swarm, alice_btc_wallet, alice_xmr_wallet, alice_peer_id) = init_alice(
        &bitcoind,
        &monero,
        btc_to_swap,
        alice_btc_starting_balance,
        xmr_to_swap,
        alice_xmr_starting_balance,
        alice_multiaddr.clone(),
    )
    .await;

    let (bob_state, bob_swarm, bob_btc_wallet, bob_xmr_wallet, bob_db) = init_bob(
        alice_multiaddr,
        alice_peer_id,
        &bitcoind,
        &monero,
        btc_to_swap,
        bob_btc_starting_balance,
        xmr_to_swap,
        bob_xmr_starting_balance,
    )
    .await;

    let bob_xmr_locked_fut = bob::swap::run_until(
        bob_state,
        bob::swap::is_xmr_locked,
        bob_swarm,
        bob_db,
        bob_btc_wallet.clone(),
        bob_xmr_wallet.clone(),
        OsRng,
        Uuid::new_v4(),
    );

    let alice_fut = alice::swap::swap(
        alice_state,
        alice_swarm,
        alice_btc_wallet.clone(),
        alice_xmr_wallet.clone(),
        Config::regtest(),
    );

    // Wait until alice has locked xmr and bob h  as locked btc
    let ((alice_state, _), _bob_state) = try_join(alice_fut, bob_xmr_locked_fut).await.unwrap();

    assert!(matches!(alice_state, AliceState::Punished));

    // todo: Add balance assertions
}

#[allow(clippy::too_many_arguments)]
async fn init_alice(
    bitcoind: &Bitcoind<'_>,
    monero: &Monero,
    btc_to_swap: bitcoin::Amount,
    _btc_starting_balance: bitcoin::Amount,
    xmr_to_swap: xmr_btc::monero::Amount,
    xmr_starting_balance: xmr_btc::monero::Amount,
    alice_multiaddr: Multiaddr,
) -> (
    AliceState,
    alice::Swarm,
    Arc<swap::bitcoin::Wallet>,
    Arc<swap::monero::Wallet>,
    PeerId,
) {
    monero
        .init(vec![("alice", xmr_starting_balance.as_piconero())])
        .await
        .unwrap();

    let alice_xmr_wallet = Arc::new(swap::monero::Wallet(
        monero.wallet("alice").unwrap().client(),
    ));

    let alice_btc_wallet = Arc::new(
        swap::bitcoin::Wallet::new("alice", bitcoind.node_url.clone())
            .await
            .unwrap(),
    );

    let amounts = SwapAmounts {
        btc: btc_to_swap,
        xmr: xmr_to_swap,
    };

    let alice_behaviour = alice::Behaviour::default();
    let alice_peer_id = alice_behaviour.peer_id();
    let alice_transport = build(alice_behaviour.identity()).unwrap();
    let rng = &mut OsRng;
    let alice_state = {
        let a = bitcoin::SecretKey::new_random(rng);
        let s_a = cross_curve_dleq::Scalar::random(rng);
        let v_a = xmr_btc::monero::PrivateViewKey::new_random(rng);
        AliceState::Started {
            amounts,
            a,
            s_a,
            v_a,
        }
    };

    let alice_swarm = alice::new_swarm(alice_multiaddr, alice_transport, alice_behaviour).unwrap();

    (
        alice_state,
        alice_swarm,
        alice_btc_wallet,
        alice_xmr_wallet,
        alice_peer_id,
    )
}

#[allow(clippy::too_many_arguments)]
async fn init_bob(
    alice_multiaddr: Multiaddr,
    alice_peer_id: PeerId,
    bitcoind: &Bitcoind<'_>,
    monero: &Monero,
    btc_to_swap: bitcoin::Amount,
    btc_starting_balance: bitcoin::Amount,
    xmr_to_swap: xmr_btc::monero::Amount,
    xmr_stating_balance: xmr_btc::monero::Amount,
) -> (
    BobState,
    bob::Swarm,
    Arc<swap::bitcoin::Wallet>,
    Arc<swap::monero::Wallet>,
    Database,
) {
    let bob_btc_wallet = Arc::new(
        swap::bitcoin::Wallet::new("bob", bitcoind.node_url.clone())
            .await
            .unwrap(),
    );
    bitcoind
        .mint(
            bob_btc_wallet.0.new_address().await.unwrap(),
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
        REFUND_TIMELOCK,
        PUNISH_TIMELOCK,
        refund_address,
    );
    let bob_state = BobState::Started {
        state0,
        amounts,
        peer_id: alice_peer_id,
        addr: alice_multiaddr,
    };
    let bob_swarm = bob::new_swarm(bob_transport, bob_behaviour).unwrap();

    (bob_state, bob_swarm, bob_btc_wallet, bob_xmr_wallet, bob_db)
}

fn init_tracing() {
    use tracing_subscriber::util::SubscriberInitExt as _;
    let _guard = tracing_subscriber::fmt()
        .with_env_filter("swap=info,xmr_btc=info")
        .with_ansi(false)
        .set_default();
}
