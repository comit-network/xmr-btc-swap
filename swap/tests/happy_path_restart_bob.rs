use libp2p::Multiaddr;
use rand::rngs::OsRng;
use swap::{alice, bitcoin, bob, storage::Database};
use tempfile::tempdir;
use testcontainers::clients::Cli;
use uuid::Uuid;
use xmr_btc::config::Config;

pub mod testutils;

use crate::testutils::{init_alice, init_bob};
use std::convert::TryFrom;
use swap::bob::swap::BobState;
use testutils::init_tracing;

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

    let alice_multiaddr: Multiaddr = "/ip4/127.0.0.1/tcp/9877"
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

    let _ = tokio::spawn(async move {
        alice::swap::swap(
            alice_state,
            alice_event_loop_handle,
            alice_btc_wallet,
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

    // TODO: Additionally assert balances
}
