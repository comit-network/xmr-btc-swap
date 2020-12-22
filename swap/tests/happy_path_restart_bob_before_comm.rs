use crate::testutils::{init_alice, init_bob};
use get_port::get_port;
use libp2p::Multiaddr;
use rand::rngs::OsRng;
use swap::{alice, alice::swap::AliceState, bitcoin, bob, bob::swap::BobState, storage::Database};
use tempfile::tempdir;
use testcontainers::clients::Cli;
use testutils::init_tracing;
use tokio::select;
use uuid::Uuid;
use xmr_btc::config::Config;

pub mod testutils;

#[tokio::test]
async fn given_bob_restarts_after_xmr_is_locked_resume_swap() {
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
    let bob_xmr_starting_balance = xmr_btc::monero::Amount::from_piconero(0);

    let alice_btc_starting_balance = bitcoin::Amount::ZERO;
    let alice_xmr_starting_balance = xmr_to_swap * 10;

    let port = get_port().expect("Failed to find a free port");
    let alice_multiaddr: Multiaddr = format!("/ip4/127.0.0.1/tcp/{}", port)
        .parse()
        .expect("failed to parse Alice's address");

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
        Config::regtest(),
    )
    .await;

    let alice_peer_id = alice_event_loop.peer_id();
    let (bob_state, bob_event_loop_1, bob_event_loop_handle_1, bob_btc_wallet, bob_xmr_wallet, _) =
        init_bob(
            alice_multiaddr.clone(),
            alice_peer_id.clone(),
            &bitcoind,
            &monero,
            btc_to_swap,
            bob_btc_starting_balance,
            xmr_to_swap,
            Config::regtest(),
        )
        .await;

    let alice_fut = alice::swap::swap(
        alice_state,
        alice_event_loop_handle,
        alice_btc_wallet.clone(),
        alice_xmr_wallet.clone(),
        Config::regtest(),
        Uuid::new_v4(),
        alice_db,
    );

    let bob_swap_id = Uuid::new_v4();
    let bob_db_datadir = tempdir().unwrap();

    let bob_xmr_locked_fut = {
        let bob_db = Database::open(bob_db_datadir.path()).unwrap();
        bob::swap::run_until(
            bob_state,
            bob::swap::is_xmr_locked,
            bob_event_loop_handle_1,
            bob_db,
            bob_btc_wallet.clone(),
            bob_xmr_wallet.clone(),
            OsRng,
            bob_swap_id,
        )
    };

    tokio::spawn(async move { alice_event_loop.run().await });

    let alice_fut_handle = tokio::spawn(alice_fut);

    // We are selecting with bob_event_loop_1 so that we stop polling on it once
    // bob reaches `xmr locked` state.
    let bob_restart_state = select! {
        res = bob_xmr_locked_fut => res.unwrap(),
        _ = bob_event_loop_1.run() => panic!("The event loop should never finish")
    };

    let (bob_event_loop_2, bob_event_loop_handle_2) =
        testutils::init_bob_event_loop(alice_peer_id, alice_multiaddr);

    let bob_fut = bob::swap::swap(
        bob_restart_state,
        bob_event_loop_handle_2,
        Database::open(bob_db_datadir.path()).unwrap(),
        bob_btc_wallet.clone(),
        bob_xmr_wallet.clone(),
        OsRng,
        bob_swap_id,
    );

    let bob_final_state = select! {
     bob_final_state = bob_fut => bob_final_state.unwrap(),
     _ = bob_event_loop_2.run() => panic!("Event loop is not expected to stop")
    };

    assert!(matches!(bob_final_state, BobState::XmrRedeemed));

    // Wait for Alice to finish too.
    let alice_final_state = alice_fut_handle.await.unwrap().unwrap();
    assert!(matches!(alice_final_state, AliceState::BtcRedeemed));

    let btc_alice_final = alice_btc_wallet.as_ref().balance().await.unwrap();
    let btc_bob_final = bob_btc_wallet.as_ref().balance().await.unwrap();

    let xmr_alice_final = alice_xmr_wallet.as_ref().get_balance().await.unwrap();

    bob_xmr_wallet.as_ref().0.refresh().await.unwrap();
    let xmr_bob_final = bob_xmr_wallet.as_ref().get_balance().await.unwrap();

    assert_eq!(
        btc_alice_final,
        alice_btc_starting_balance + btc_to_swap - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
    );
    assert!(btc_bob_final <= bob_btc_starting_balance - btc_to_swap);

    assert!(xmr_alice_final <= alice_xmr_starting_balance - xmr_to_swap);
    assert_eq!(xmr_bob_final, bob_xmr_starting_balance + xmr_to_swap);
}
