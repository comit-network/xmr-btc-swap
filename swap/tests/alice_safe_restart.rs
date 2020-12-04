use bitcoin_harness::Bitcoind;
use futures::future::try_join;
use libp2p::Multiaddr;
use monero_harness::{image, Monero};
use rand::rngs::OsRng;
use std::sync::Arc;
use swap::{
    alice, alice::swap::AliceState, bitcoin, bob, bob::swap::BobState, monero,
    network::transport::build, storage::Database, SwapAmounts, PUNISH_TIMELOCK, REFUND_TIMELOCK,
};
use tempfile::{tempdir, TempDir};
use testcontainers::{clients::Cli, Container};
use uuid::Uuid;
use xmr_btc::cross_curve_dleq;

fn setup_tracing() {
    use tracing_subscriber::util::SubscriberInitExt as _;
    let _guard = tracing_subscriber::fmt()
        .with_env_filter("swap=info,xmr_btc=info")
        .with_ansi(false)
        .set_default();
}

// This is just to keep the containers alive
#[allow(dead_code)]
struct Containers<'a> {
    bitcoind: Bitcoind<'a>,
    monerods: Vec<Container<'a, Cli, image::Monero>>,
}

/// Returns Alice's and Bob's wallets, in this order
async fn setup_wallets(
    cli: &Cli,
    _init_btc_alice: bitcoin::Amount,
    init_xmr_alice: monero::Amount,
    init_btc_bob: bitcoin::Amount,
    init_xmr_bob: monero::Amount,
) -> (
    bitcoin::Wallet,
    monero::Wallet,
    bitcoin::Wallet,
    monero::Wallet,
    Containers<'_>,
) {
    let bitcoind = Bitcoind::new(&cli, "0.19.1").unwrap();
    let _ = bitcoind.init(5).await;

    let alice_btc_wallet = swap::bitcoin::Wallet::new("alice", bitcoind.node_url.clone())
        .await
        .unwrap();
    let bob_btc_wallet = swap::bitcoin::Wallet::new("bob", bitcoind.node_url.clone())
        .await
        .unwrap();
    bitcoind
        .mint(bob_btc_wallet.0.new_address().await.unwrap(), init_btc_bob)
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

#[tokio::test]
async fn alice_safe_restart_after_btc_is_locked() {
    setup_tracing();

    let alice_multiaddr: Multiaddr = "/ip4/127.0.0.1/tcp/9876"
        .parse()
        .expect("failed to parse Alice's address");

    let btc_to_swap = bitcoin::Amount::from_sat(1_000_000);
    let init_btc_alice = bitcoin::Amount::ZERO;
    let init_btc_bob = btc_to_swap * 10;

    let xmr_to_swap = monero::Amount::from_piconero(1_000_000_000_000);
    let init_xmr_alice = xmr_to_swap * 10;
    let init_xmr_bob = monero::Amount::ZERO;

    let cli = Cli::default();
    let (alice_btc_wallet, alice_xmr_wallet, bob_btc_wallet, bob_xmr_wallet, _containers) =
        setup_wallets(
            &cli,
            init_btc_alice,
            init_xmr_alice,
            init_btc_bob,
            init_xmr_bob,
        )
        .await;

    let alice_btc_wallet = Arc::new(alice_btc_wallet);
    let alice_xmr_wallet = Arc::new(alice_xmr_wallet);
    let bob_btc_wallet = Arc::new(bob_btc_wallet);
    let bob_xmr_wallet = Arc::new(bob_xmr_wallet);

    let amounts = SwapAmounts {
        btc: btc_to_swap,
        xmr: xmr_to_swap,
    };

    let (alice_swap, alice_peer_id) = {
        let rng = &mut OsRng;
        let alice_behaviour = alice::Behaviour::default();
        let alice_peer_id = alice_behaviour.peer_id();
        let alice_transport = build(alice_behaviour.identity()).unwrap();
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
        let alice_swarm =
            alice::new_swarm(alice_multiaddr.clone(), alice_transport, alice_behaviour).unwrap();
        let config = xmr_btc::config::Config::regtest();
        let swap_id = Uuid::new_v4();
        let tmp_dir = TempDir::new().unwrap();
        let db = Database::open(tmp_dir.path()).unwrap();

        (
            alice::swap::swap(
                alice_state,
                alice_swarm,
                alice_btc_wallet.clone(),
                alice_xmr_wallet.clone(),
                config,
                swap_id,
                db,
            ),
            alice_peer_id,
        )
    };

    let bob_swap = {
        let rng = &mut OsRng;
        let bob_db_dir = tempdir().unwrap();
        let bob_db = Database::open(bob_db_dir.path()).unwrap();
        let bob_behaviour = bob::Behaviour::default();
        let bob_transport = build(bob_behaviour.identity()).unwrap();

        let refund_address = bob_btc_wallet.new_address().await.unwrap();
        let state0 = xmr_btc::bob::State0::new(
            rng,
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
        bob::swap::swap(
            bob_state,
            bob_swarm,
            bob_db,
            bob_btc_wallet.clone(),
            bob_xmr_wallet.clone(),
            OsRng,
            Uuid::new_v4(),
        )
    };

    try_join(alice_swap, bob_swap).await.unwrap();

    let btc_alice_final = alice_btc_wallet.balance().await.unwrap();
    let btc_bob_final = bob_btc_wallet.balance().await.unwrap();

    let xmr_alice_final = alice_xmr_wallet.get_balance().await.unwrap();

    bob_xmr_wallet.0.refresh().await.unwrap();
    let xmr_bob_final = bob_xmr_wallet.get_balance().await.unwrap();

    assert_eq!(
        btc_alice_final,
        init_btc_alice + btc_to_swap - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
    );
    assert!(btc_bob_final <= init_btc_bob - btc_to_swap);

    assert!(xmr_alice_final <= init_xmr_alice - xmr_to_swap);
    assert_eq!(xmr_bob_final, init_xmr_bob + xmr_to_swap);
}
