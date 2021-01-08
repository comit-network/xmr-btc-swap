use crate::testutils::{init_alice, init_bob};
use futures::{
    future::{join, select, Either},
    FutureExt,
};
use get_port::get_port;
use libp2p::Multiaddr;
use rand::rngs::OsRng;
use swap::{
    bitcoin,
    config::Config,
    monero,
    protocol::{alice, alice::AliceState, bob, bob::BobState},
    seed::Seed,
};
use testcontainers::clients::Cli;
use testutils::init_tracing;
use uuid::Uuid;

pub mod testutils;

/// Bob locks Btc and Alice locks Xmr. Bob does not act; he fails to send Alice
/// the encsig and fail to refund or redeem. Alice punishes.
#[tokio::test]
async fn alice_punishes_if_bob_never_acts_after_fund() {
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
    let xmr_to_swap = monero::Amount::from_piconero(1_000_000_000_000);

    let bob_btc_starting_balance = btc_to_swap * 10;

    let alice_btc_starting_balance = bitcoin::Amount::ZERO;
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
        &Seed::random().unwrap(),
    )
    .await;

    let (bob_state, bob_event_loop, bob_event_loop_handle, bob_btc_wallet, bob_xmr_wallet, bob_db) =
        init_bob(
            alice_multiaddr,
            alice_event_loop.peer_id(),
            &bitcoind,
            &monero,
            btc_to_swap,
            bob_btc_starting_balance,
            xmr_to_swap,
            config,
        )
        .await;

    let bob_btc_locked_fut = bob::swap::run_until(
        bob_state,
        bob::swap::is_btc_locked,
        bob_event_loop_handle,
        bob_db,
        bob_btc_wallet.clone(),
        bob_xmr_wallet.clone(),
        OsRng,
        Uuid::new_v4(),
    )
    .boxed();

    let bob_fut = select(bob_btc_locked_fut, bob_event_loop.run().boxed());

    let alice_fut = alice::swap::swap(
        alice_state,
        alice_event_loop_handle,
        alice_btc_wallet.clone(),
        alice_xmr_wallet.clone(),
        Config::regtest(),
        Uuid::new_v4(),
        alice_db,
    )
    .boxed();

    let alice_fut = select(alice_fut, alice_event_loop.run().boxed());

    // Wait until alice has locked xmr and bob has locked btc
    let (alice_state, bob_state) = join(alice_fut, bob_fut).await;

    let alice_state = match alice_state {
        Either::Left((state, _)) => state.unwrap(),
        Either::Right(_) => panic!("Alice event loop should not terminate."),
    };

    let bob_state = match bob_state {
        Either::Left((state, _)) => state.unwrap(),
        Either::Right(_) => panic!("Bob event loop should not terminate."),
    };

    assert!(matches!(alice_state, AliceState::BtcPunished));
    let bob_state3 = if let BobState::BtcLocked(state3, ..) = bob_state {
        state3
    } else {
        panic!("Bob in unexpected state");
    };

    let btc_alice_final = alice_btc_wallet.as_ref().balance().await.unwrap();
    let btc_bob_final = bob_btc_wallet.as_ref().balance().await.unwrap();

    // lock_tx_bitcoin_fee is determined by the wallet, it is not necessarily equal
    // to TX_FEE
    let lock_tx_bitcoin_fee = bob_btc_wallet
        .transaction_fee(bob_state3.tx_lock_id())
        .await
        .unwrap();

    assert_eq!(
        btc_alice_final,
        alice_btc_starting_balance + btc_to_swap - bitcoin::Amount::from_sat(2 * bitcoin::TX_FEE)
    );

    assert_eq!(
        btc_bob_final,
        bob_btc_starting_balance - btc_to_swap - lock_tx_bitcoin_fee
    );
}
