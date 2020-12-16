use libp2p::Multiaddr;
use swap::{alice, bitcoin, bob, storage::Database};
use tempfile::tempdir;
use testcontainers::clients::Cli;
use uuid::Uuid;
use xmr_btc::config::Config;

pub mod testutils;

use crate::testutils::{init_alice, init_bob};
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

    // TODO: we are making a clone of Alices's wallets here to keep them in scope
    // after Alices's wallets are moved into an async task.
    let alice_btc_wallet_clone = alice_btc_wallet.clone();
    let alice_xmr_wallet_clone = alice_xmr_wallet.clone();

    // TODO: we are making a clone of Bob's wallets here to keep them in scope after
    // Bob's wallets are moved into an async task.
    let bob_btc_wallet_clone = bob_btc_wallet.clone();
    let bob_xmr_wallet_clone = bob_xmr_wallet.clone();

    tokio::spawn(async move { alice_event_loop.run().await });
    tokio::spawn(async move {
        alice::swap::Swap::new(
            alice_event_loop_handle,
            alice_btc_wallet.clone(),
            alice_xmr_wallet.clone(),
            config,
            Uuid::new_v4(),
            alice_db,
        )
        .swap(alice_state)
        .await
    });

    tokio::spawn(async move { bob_event_loop.run().await });

    let bob_swap_id = Uuid::new_v4();
    let bob_db_datadir = tempdir().unwrap();

    let bob_state = {
        let bob_db = Database::open(bob_db_datadir.path()).unwrap();

        bob::swap::Swap::new(
            bob_event_loop_handle,
            bob_db,
            bob_btc_wallet.clone(),
            bob_xmr_wallet.clone(),
            bob_swap_id,
        )
        .run_until(bob_state, |state| matches!(state, BobState::EncSigSent(..)))
        .await
        .unwrap()
    };

    assert!(matches!(bob_state, BobState::EncSigSent {..}));

    let bob_db = Database::open(bob_db_datadir.path()).unwrap();
    let state_before_restart = bob_db.get_state(bob_swap_id).unwrap();

    if let swap::state::Swap::Bob(state) = state_before_restart.clone() {
        assert!(matches!(state, swap::state::Bob::EncSigSent {..}));
    }

    let (event_loop_after_restart, event_loop_handle_after_restart) =
        testutils::init_bob_event_loop();

    tokio::spawn(async move { event_loop_after_restart.run().await });
    let alice_state = bob::swap::Swap::new(
        event_loop_handle_after_restart,
        bob_db,
        bob_btc_wallet,
        bob_xmr_wallet,
        bob_swap_id,
    )
    .resume_from_database()
    .await
    .unwrap();

    assert!(matches!(alice_state, BobState::XmrRedeemed {..}));

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
