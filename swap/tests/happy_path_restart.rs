use libp2p::Multiaddr;
use rand::rngs::OsRng;
use swap::{alice, alice::swap::AliceState, bitcoin, bob, storage::Database};
use tempfile::tempdir;
use testcontainers::clients::Cli;
use uuid::Uuid;
use xmr_btc::config::Config;

pub mod testutils;

use crate::testutils::{init_alice, init_bob};
use testutils::init_tracing;

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

    let alice_multiaddr: Multiaddr = "/ip4/127.0.0.1/tcp/9877"
        .parse()
        .expect("failed to parse Alice's address");

    let config = Config::regtest();

    let (
        start_state,
        mut alice_event_loop,
        alice_event_loop_handle,
        alice_btc_wallet,
        alice_xmr_wallet,
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
        Config::regtest(),
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

    let alice_state = alice::swap::recover(
        event_loop_handle_after_restart,
        alice_btc_wallet,
        alice_xmr_wallet,
        Config::regtest(),
        alice_swap_id,
        alice_db,
    )
    .await
    .unwrap();

    assert!(matches!(alice_state, AliceState::BtcRedeemed {..}));

    // TODO: Additionally assert balances
}
