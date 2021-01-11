use get_port::get_port;
use libp2p::Multiaddr;
use rand::rngs::OsRng;
use tempfile::tempdir;
use testcontainers::clients::Cli;
use uuid::Uuid;

use crate::{
    bitcoin,
    config::Config,
    database,
    database::Database,
    monero,
    protocol::{alice, bob, bob::BobState},
    seed::Seed,
    tests::{init_alice, init_bob, init_bob_event_loop, init_containers, init_tracing, Containers},
};

#[tokio::test]
async fn given_bob_restarts_after_encsig_is_sent_resume_swap() {
    let _guard = init_tracing();

    let cli = Cli::default();
    let (
        monero,
        Containers {
            bitcoind,
            monerods: _monerods,
        },
    ) = init_containers(&cli).await;

    let btc_to_swap = bitcoin::Amount::from_sat(1_000_000);
    let xmr_to_swap = monero::Amount::from_piconero(1_000_000_000_000);

    let bob_btc_starting_balance = btc_to_swap * 10;
    let alice_xmr_starting_balance = xmr_to_swap * 10;

    let port = get_port().expect("Failed to find a free port");
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
        Seed::random().unwrap(),
    )
    .await;

    let alice_peer_id = alice_event_loop.peer_id();
    let (bob_state, bob_event_loop, bob_event_loop_handle, bob_btc_wallet, bob_xmr_wallet, _) =
        init_bob(
            alice_multiaddr.clone(),
            alice_peer_id.clone(),
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

    let alice_swap_handle = tokio::spawn(alice::swap(
        alice_state,
        alice_event_loop_handle,
        alice_btc_wallet.clone(),
        alice_xmr_wallet.clone(),
        config,
        Uuid::new_v4(),
        alice_db,
    ));

    tokio::spawn(async move { alice_event_loop.run().await });

    tokio::spawn(bob_event_loop.run());

    let bob_swap_id = Uuid::new_v4();
    let bob_db_datadir = tempdir().unwrap();
    let bob_db = Database::open(bob_db_datadir.path()).unwrap();

    let bob_state = bob::run_until(
        bob_state,
        bob::is_encsig_sent,
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

    let resume_state = if let database::Swap::Bob(state) = bob_db.get_state(bob_swap_id).unwrap() {
        assert!(matches!(state, database::Bob::EncSigSent {..}));
        state.into()
    } else {
        unreachable!()
    };

    let (event_loop_after_restart, event_loop_handle_after_restart) =
        init_bob_event_loop(alice_peer_id, alice_multiaddr);
    tokio::spawn(event_loop_after_restart.run());

    let bob_state = bob::swap(
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

    // Wait for Alice to finish too
    alice_swap_handle.await.unwrap().unwrap();

    assert!(matches!(bob_state, BobState::XmrRedeemed {..}));

    let btc_alice_final = alice_btc_wallet_clone.as_ref().balance().await.unwrap();
    let btc_bob_final = bob_btc_wallet_clone.as_ref().balance().await.unwrap();

    assert_eq!(
        btc_alice_final,
        btc_to_swap - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
    );
    assert!(btc_bob_final <= bob_btc_starting_balance - btc_to_swap);

    let xmr_alice_final = alice_xmr_wallet_clone.as_ref().get_balance().await.unwrap();
    bob_xmr_wallet_clone.as_ref().inner.refresh().await.unwrap();
    let xmr_bob_final = bob_xmr_wallet_clone.as_ref().get_balance().await.unwrap();

    assert!(xmr_alice_final <= alice_xmr_starting_balance - xmr_to_swap);
    assert_eq!(xmr_bob_final, xmr_to_swap);
}
