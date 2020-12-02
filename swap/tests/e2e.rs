use bitcoin_harness::Bitcoind;
use futures::{channel::mpsc, future::try_join};
use libp2p::Multiaddr;
use monero_harness::Monero;
use rand::rngs::OsRng;
use std::sync::Arc;
use swap::{
    alice, alice::swap::AliceState, bob, bob::swap::BobState, network::transport::build,
    storage::Database, SwapAmounts, PUNISH_TIMELOCK, REFUND_TIMELOCK,
};
use tempfile::{tempdir, TempDir};
use testcontainers::clients::Cli;
use uuid::Uuid;
use xmr_btc::{bitcoin, cross_curve_dleq};

#[ignore]
#[tokio::test]
async fn swap() {
    use tracing_subscriber::util::SubscriberInitExt as _;
    let _guard = tracing_subscriber::fmt()
        .with_env_filter("swap=info,xmr_btc=info")
        .with_ansi(false)
        .set_default();

    let alice_multiaddr: Multiaddr = "/ip4/127.0.0.1/tcp/9876"
        .parse()
        .expect("failed to parse Alice's address");

    let cli = Cli::default();
    let bitcoind = Bitcoind::new(&cli, "0.19.1").unwrap();
    dbg!(&bitcoind.node_url);
    let _ = bitcoind.init(5).await;

    let btc = bitcoin::Amount::from_sat(1_000_000);
    let btc_alice = bitcoin::Amount::ZERO;
    let btc_bob = btc * 10;

    // this xmr value matches the logic of alice::calculate_amounts i.e. btc *
    // 10_000 * 100
    let xmr = 1_000_000_000_000;
    let xmr_alice = xmr * 10;
    let xmr_bob = 0;

    let alice_btc_wallet = Arc::new(
        swap::bitcoin::Wallet::new("alice", bitcoind.node_url.clone())
            .await
            .unwrap(),
    );
    let bob_btc_wallet = Arc::new(
        swap::bitcoin::Wallet::new("bob", bitcoind.node_url.clone())
            .await
            .unwrap(),
    );
    bitcoind
        .mint(bob_btc_wallet.0.new_address().await.unwrap(), btc_bob)
        .await
        .unwrap();

    let (monero, _container) =
        Monero::new(&cli, None, vec!["alice".to_string(), "bob".to_string()])
            .await
            .unwrap();
    monero
        .init(vec![("alice", xmr_alice), ("bob", xmr_bob)])
        .await
        .unwrap();

    let alice_xmr_wallet = Arc::new(swap::monero::Wallet(
        monero.wallet("alice").unwrap().client(),
    ));
    let bob_xmr_wallet = Arc::new(swap::monero::Wallet(monero.wallet("bob").unwrap().client()));

    let alice_behaviour = alice::Behaviour::default();
    let alice_transport = build(alice_behaviour.identity()).unwrap();

    let db = Database::open(std::path::Path::new("../.swap-db/")).unwrap();
    let alice_swap = alice::swap(
        alice_btc_wallet.clone(),
        alice_xmr_wallet.clone(),
        db,
        alice_multiaddr.clone(),
        alice_transport,
        alice_behaviour,
    );

    let db_dir = tempdir().unwrap();
    let db = Database::open(db_dir.path()).unwrap();
    let (cmd_tx, mut _cmd_rx) = mpsc::channel(1);
    let (mut rsp_tx, rsp_rx) = mpsc::channel(1);
    let bob_behaviour = bob::Behaviour::default();
    let bob_transport = build(bob_behaviour.identity()).unwrap();
    let bob_swap = bob::swap(
        bob_btc_wallet.clone(),
        bob_xmr_wallet.clone(),
        db,
        btc.as_sat(),
        alice_multiaddr,
        cmd_tx,
        rsp_rx,
        bob_transport,
        bob_behaviour,
    );

    // automate the verification step by accepting any amounts sent over by Alice
    rsp_tx.try_send(swap::Rsp::VerifiedAmounts).unwrap();

    try_join(alice_swap, bob_swap).await.unwrap();

    let btc_alice_final = alice_btc_wallet.as_ref().balance().await.unwrap();
    let btc_bob_final = bob_btc_wallet.as_ref().balance().await.unwrap();

    let xmr_alice_final = alice_xmr_wallet.as_ref().get_balance().await.unwrap();

    bob_xmr_wallet.as_ref().0.refresh().await.unwrap();
    let xmr_bob_final = bob_xmr_wallet.as_ref().get_balance().await.unwrap();

    assert_eq!(
        btc_alice_final,
        btc_alice + btc - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
    );
    assert!(btc_bob_final <= btc_bob - btc);

    assert!(xmr_alice_final.as_piconero() <= xmr_alice - xmr);
    assert_eq!(xmr_bob_final.as_piconero(), xmr_bob + xmr);
}

#[tokio::test]
async fn happy_path_recursive_executor() {
    use tracing_subscriber::util::SubscriberInitExt as _;
    let _guard = tracing_subscriber::fmt()
        .with_env_filter("swap=info,xmr_btc=info")
        .with_ansi(false)
        .set_default();

    let alice_multiaddr: Multiaddr = "/ip4/127.0.0.1/tcp/9876"
        .parse()
        .expect("failed to parse Alice's address");

    let cli = Cli::default();
    let bitcoind = Bitcoind::new(&cli, "0.19.1").unwrap();
    dbg!(&bitcoind.node_url);
    let _ = bitcoind.init(5).await;

    let btc = bitcoin::Amount::from_sat(1_000_000);
    let btc_alice = bitcoin::Amount::ZERO;
    let btc_bob = btc * 10;

    // this xmr value matches the logic of alice::calculate_amounts i.e. btc *
    // 10_000 * 100
    let xmr = 1_000_000_000_000;
    let xmr_alice = xmr * 10;
    let xmr_bob = 0;

    let alice_btc_wallet = Arc::new(
        swap::bitcoin::Wallet::new("alice", bitcoind.node_url.clone())
            .await
            .unwrap(),
    );
    let bob_btc_wallet = Arc::new(
        swap::bitcoin::Wallet::new("bob", bitcoind.node_url.clone())
            .await
            .unwrap(),
    );
    bitcoind
        .mint(bob_btc_wallet.0.new_address().await.unwrap(), btc_bob)
        .await
        .unwrap();

    let (monero, _container) =
        Monero::new(&cli, None, vec!["alice".to_string(), "bob".to_string()])
            .await
            .unwrap();
    monero
        .init(vec![("alice", xmr_alice), ("bob", xmr_bob)])
        .await
        .unwrap();

    let alice_xmr_wallet = Arc::new(swap::monero::Wallet(
        monero.wallet("alice").unwrap().client(),
    ));
    let bob_xmr_wallet = Arc::new(swap::monero::Wallet(monero.wallet("bob").unwrap().client()));

    let amounts = SwapAmounts {
        btc,
        xmr: xmr_btc::monero::Amount::from_piconero(xmr),
    };

    let rng = &mut OsRng;

    let (alice_swap, alice_peer_id) = {
        let alice_behaviour = alice::Behaviour::default();
        let peer_id = alice_behaviour.peer_id();
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

        let tmp_dir = TempDir::new().unwrap();

        let db = Database::open(tmp_dir.path()).unwrap();

        let swap = alice::swap::swap(
            alice_state,
            alice_swarm,
            alice_btc_wallet.clone(),
            alice_xmr_wallet.clone(),
            Uuid::new_v4(),
            db,
        );

        (swap, peer_id)
    };

    let bob_db_dir = tempdir().unwrap();
    let bob_db = Database::open(bob_db_dir.path()).unwrap();
    let bob_behaviour = bob::Behaviour::default();
    let bob_transport = build(bob_behaviour.identity()).unwrap();

    let refund_address = bob_btc_wallet.new_address().await.unwrap();
    let state0 = xmr_btc::bob::State0::new(
        rng,
        btc,
        xmr_btc::monero::Amount::from_piconero(xmr),
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
        btc_alice + btc - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
    );
    assert!(btc_bob_final <= btc_bob - btc);

    assert!(xmr_alice_final.as_piconero() <= xmr_alice - xmr);
    assert_eq!(xmr_bob_final.as_piconero(), xmr_bob + xmr);
}
