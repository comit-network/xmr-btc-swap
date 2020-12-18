use crate::testutils::{init_alice, init_bob};
use futures::future::try_join;
use libp2p::Multiaddr;
use portpicker::pick_unused_port;
use rand::rngs::OsRng;
use std::convert::TryFrom;
use swap::{alice, alice::swap::AliceState, bob, bob::swap::BobState, storage::Database};
use tempfile::tempdir;
use testcontainers::clients::Cli;
use testutils::init_tracing;
use uuid::Uuid;
use xmr_btc::{bitcoin, config::Config};

pub mod testutils;

/// Run the following tests with RUST_MIN_STACK=10000000

#[tokio::test]
async fn happy_path() {
    let _guard = init_tracing();

    let cli = Cli::default();
    let (
        monero,
        testutils::Containers {
            bitcoind,
            monerods: _monerods,
        },
    ) = testutils::init_containers(&cli).await;

    let btc_to_swap = bitcoin::Amount::from_sat(1_000_000);
    let btc_alice = bitcoin::Amount::ZERO;
    let btc_bob = btc_to_swap * 10;

    // this xmr value matches the logic of alice::calculate_amounts i.e. btc *
    // 10_000 * 100
    let xmr_to_swap = xmr_btc::monero::Amount::from_piconero(1_000_000_000_000);
    let xmr_alice = xmr_to_swap * 10;
    let xmr_bob = xmr_btc::monero::Amount::ZERO;

    let port = pick_unused_port().expect("No ports free");
    let alice_multiaddr: Multiaddr = format!("/ip4/127.0.0.1/tcp/{}", port)
        .parse()
        .expect("failed to parse Alice's address");

    let config = Config::regtest();

    let (
        alice_state,
        mut alice_event_loop,
        alice_event_loop_handle,
        alice_btc_wallet,
        alice_xmr_wallet,
        alice_db,
    ) = init_alice(
        &bitcoind,
        &monero,
        btc_to_swap,
        xmr_to_swap,
        xmr_alice,
        alice_multiaddr.clone(),
        config,
    )
    .await;

    let (bob_state, bob_event_loop, bob_event_loop_handle, bob_btc_wallet, bob_xmr_wallet, bob_db) =
        init_bob(
            alice_multiaddr,
            &bitcoind,
            &monero,
            btc_to_swap,
            btc_bob,
            xmr_to_swap,
            config,
        )
        .await;

    let alice_swap_fut = alice::swap::swap(
        alice_state,
        alice_event_loop_handle,
        alice_btc_wallet.clone(),
        alice_xmr_wallet.clone(),
        config,
        Uuid::new_v4(),
        alice_db,
    );

    let _alice_swarm_fut = tokio::spawn(async move { alice_event_loop.run().await });

    let bob_swap_fut = bob::swap::swap(
        bob_state,
        bob_event_loop_handle,
        bob_db,
        bob_btc_wallet.clone(),
        bob_xmr_wallet.clone(),
        OsRng,
        Uuid::new_v4(),
    );

    let _bob_swarm_fut = tokio::spawn(async move { bob_event_loop.run().await });

    try_join(alice_swap_fut, bob_swap_fut).await.unwrap();

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

#[tokio::test]
async fn given_alice_restarts_after_encsig_is_learned_resume_swap() {
    let _guard = init_tracing();

    let cli = Cli::default();
    let (
        monero,
        testutils::Containers {
            bitcoind,
            monerods: _monerods,
        },
    ) = testutils::init_containers(&cli).await;

    let btc_to_swap = bitcoin::Amount::from_sat(1_000_000);
    let xmr_to_swap = xmr_btc::monero::Amount::from_piconero(1_000_000_000_000);

    let bob_btc_starting_balance = btc_to_swap * 10;
    let alice_xmr_starting_balance = xmr_to_swap * 10;

    let port = pick_unused_port().expect("No ports free");
    let alice_multiaddr: Multiaddr = format!("/ip4/127.0.0.1/tcp/{}", port)
        .parse()
        .expect("failed to parse Alice's address");

    let config = Config::regtest();

    let (
        start_state,
        mut alice_event_loop,
        alice_event_loop_handle,
        alice_btc_wallet,
        alice_xmr_wallet,
        _,
    ) = init_alice(
        &bitcoind,
        &monero,
        btc_to_swap,
        xmr_to_swap,
        alice_xmr_starting_balance,
        alice_multiaddr.clone(),
        config,
    )
    .await;

    let (bob_state, bob_event_loop, bob_event_loop_handle, bob_btc_wallet, bob_xmr_wallet, bob_db) =
        init_bob(
            alice_multiaddr.clone(),
            &bitcoind,
            &monero,
            btc_to_swap,
            bob_btc_starting_balance,
            xmr_to_swap,
            config,
        )
        .await;

    // TODO: we are making a clone of Bob's wallets here to keep them in scope after
    // Bob's wallets are moved into an async task.
    let bob_btc_wallet_clone = bob_btc_wallet.clone();
    let bob_xmr_wallet_clone = bob_xmr_wallet.clone();

    let _ = tokio::spawn(async move {
        bob::swap::swap(
            bob_state,
            bob_event_loop_handle,
            bob_db,
            bob_btc_wallet.clone(),
            bob_xmr_wallet.clone(),
            OsRng,
            Uuid::new_v4(),
        )
        .await
    });

    let _bob_swarm_fut = tokio::spawn(async move { bob_event_loop.run().await });

    let alice_db_datadir = tempdir().unwrap();
    let alice_db = Database::open(alice_db_datadir.path()).unwrap();

    let _alice_swarm_fut = tokio::spawn(async move { alice_event_loop.run().await });

    let alice_swap_id = Uuid::new_v4();

    let alice_state = alice::swap::run_until(
        start_state,
        alice::swap::is_encsig_learned,
        alice_event_loop_handle,
        alice_btc_wallet.clone(),
        alice_xmr_wallet.clone(),
        config,
        alice_swap_id,
        alice_db,
    )
    .await
    .unwrap();

    assert!(matches!(alice_state, AliceState::EncSignLearned {..}));

    let alice_db = Database::open(alice_db_datadir.path()).unwrap();
    let state_before_restart = alice_db.get_state(alice_swap_id).unwrap();

    if let swap::state::Swap::Alice(state) = state_before_restart.clone() {
        assert!(matches!(state, swap::state::Alice::EncSignLearned {..}));
    }

    let (mut event_loop_after_restart, event_loop_handle_after_restart) =
        testutils::init_alice_event_loop(alice_multiaddr);
    let _alice_swarm_fut = tokio::spawn(async move { event_loop_after_restart.run().await });

    let db_swap = alice_db.get_state(alice_swap_id).unwrap();
    let resume_state = AliceState::try_from(db_swap).unwrap();

    let alice_state = alice::swap::swap(
        resume_state,
        event_loop_handle_after_restart,
        alice_btc_wallet.clone(),
        alice_xmr_wallet.clone(),
        config,
        alice_swap_id,
        alice_db,
    )
    .await
    .unwrap();

    assert!(matches!(alice_state, AliceState::BtcRedeemed {..}));

    let btc_alice_final = alice_btc_wallet.as_ref().balance().await.unwrap();
    let btc_bob_final = bob_btc_wallet_clone.as_ref().balance().await.unwrap();

    assert_eq!(
        btc_alice_final,
        btc_to_swap - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
    );
    assert!(btc_bob_final <= bob_btc_starting_balance - btc_to_swap);

    let xmr_alice_final = alice_xmr_wallet.as_ref().get_balance().await.unwrap();
    bob_xmr_wallet_clone.as_ref().0.refresh().await.unwrap();
    let xmr_bob_final = bob_xmr_wallet_clone.as_ref().get_balance().await.unwrap();

    assert!(xmr_alice_final <= alice_xmr_starting_balance - xmr_to_swap);
    assert_eq!(xmr_bob_final, xmr_to_swap);
}

#[tokio::test]
async fn given_bob_restarts_after_encsig_is_sent_resume_swap() {
    let _guard = init_tracing();

    let cli = Cli::default();
    let (
        monero,
        testutils::Containers {
            bitcoind,
            monerods: _monerods,
        },
    ) = testutils::init_containers(&cli).await;

    let btc_to_swap = bitcoin::Amount::from_sat(1_000_000);
    let xmr_to_swap = xmr_btc::monero::Amount::from_piconero(1_000_000_000_000);

    let bob_btc_starting_balance = btc_to_swap * 10;
    let alice_xmr_starting_balance = xmr_to_swap * 10;

    let port = pick_unused_port().expect("No ports free");
    let alice_multiaddr: Multiaddr = format!("/ip4/127.0.0.1/tcp/{}", port)
        .parse()
        .expect("failed to parse Alice's address");

    let config = Config::regtest();

    let (
        alice_state,
        mut alice_event_loop,
        alice_event_loop_handle,
        alice_btc_wallet,
        alice_xmr_wallet,
        alice_db,
    ) = init_alice(
        &bitcoind,
        &monero,
        btc_to_swap,
        xmr_to_swap,
        alice_xmr_starting_balance,
        alice_multiaddr.clone(),
        config,
    )
    .await;

    let (bob_state, bob_event_loop, bob_event_loop_handle, bob_btc_wallet, bob_xmr_wallet, _) =
        init_bob(
            alice_multiaddr.clone(),
            &bitcoind,
            &monero,
            btc_to_swap,
            bob_btc_starting_balance,
            xmr_to_swap,
            config,
        )
        .await;

    // TODO: we are making a clone of Alices's wallets here to keep them in scope
    // after Alices's wallets are moved into an async task.
    let alice_btc_wallet_clone = alice_btc_wallet.clone();
    let alice_xmr_wallet_clone = alice_xmr_wallet.clone();

    // TODO: we are making a clone of Bob's wallets here to keep them in scope after
    // Bob's wallets are moved into an async task.
    let bob_btc_wallet_clone = bob_btc_wallet.clone();
    let bob_xmr_wallet_clone = bob_xmr_wallet.clone();

    let _ = tokio::spawn(async move {
        alice::swap::swap(
            alice_state,
            alice_event_loop_handle,
            alice_btc_wallet.clone(),
            alice_xmr_wallet.clone(),
            config,
            Uuid::new_v4(),
            alice_db,
        )
        .await
    });

    let _alice_swarm_fut = tokio::spawn(async move { alice_event_loop.run().await });

    let _bob_swarm_fut = tokio::spawn(async move { bob_event_loop.run().await });

    let bob_swap_id = Uuid::new_v4();
    let bob_db_datadir = tempdir().unwrap();
    let bob_db = Database::open(bob_db_datadir.path()).unwrap();

    let bob_state = bob::swap::run_until(
        bob_state,
        bob::swap::is_encsig_sent,
        bob_event_loop_handle,
        bob_db,
        bob_btc_wallet.clone(),
        bob_xmr_wallet.clone(),
        OsRng,
        bob_swap_id,
    )
    .await
    .unwrap();

    assert!(matches!(bob_state, BobState::EncSigSent {..}));

    let bob_db = Database::open(bob_db_datadir.path()).unwrap();
    let state_before_restart = bob_db.get_state(bob_swap_id).unwrap();

    if let swap::state::Swap::Bob(state) = state_before_restart.clone() {
        assert!(matches!(state, swap::state::Bob::EncSigSent {..}));
    }

    let (event_loop_after_restart, event_loop_handle_after_restart) =
        testutils::init_bob_event_loop();
    let _bob_swarm_fut = tokio::spawn(async move { event_loop_after_restart.run().await });

    let db_swap = bob_db.get_state(bob_swap_id).unwrap();
    let resume_state = BobState::try_from(db_swap).unwrap();

    let bob_state = bob::swap::swap(
        resume_state,
        event_loop_handle_after_restart,
        bob_db,
        bob_btc_wallet,
        bob_xmr_wallet,
        OsRng,
        bob_swap_id,
    )
    .await
    .unwrap();

    assert!(matches!(bob_state, BobState::XmrRedeemed {..}));

    let btc_alice_final = alice_btc_wallet_clone.as_ref().balance().await.unwrap();
    let btc_bob_final = bob_btc_wallet_clone.as_ref().balance().await.unwrap();

    assert_eq!(
        btc_alice_final,
        btc_to_swap - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
    );
    assert!(btc_bob_final <= bob_btc_starting_balance - btc_to_swap);

    let xmr_alice_final = alice_xmr_wallet_clone.as_ref().get_balance().await.unwrap();
    bob_xmr_wallet_clone.as_ref().0.refresh().await.unwrap();
    let xmr_bob_final = bob_xmr_wallet_clone.as_ref().get_balance().await.unwrap();

    assert!(xmr_alice_final <= alice_xmr_starting_balance - xmr_to_swap);
    assert_eq!(xmr_bob_final, xmr_to_swap);
}
