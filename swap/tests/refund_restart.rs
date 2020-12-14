use crate::testutils::{init_alice, init_bob};
use futures::future::try_join;
use libp2p::Multiaddr;
use rand::rngs::OsRng;
use swap::{alice, alice::swap::AliceState, bob, bob::swap::BobState, storage::Database};
use tempfile::tempdir;
use testcontainers::clients::Cli;
use testutils::init_tracing;
use uuid::Uuid;
use xmr_btc::{bitcoin, config::Config};

pub mod testutils;

// Bob locks btc and Alice locks xmr. Alice fails to act so Bob refunds. Alice
// then also refunds.
#[tokio::test]
async fn both_refund() {
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

    // todo: This should not be hardcoded
    let alice_multiaddr: Multiaddr = "/ip4/127.0.0.1/tcp/9879"
        .parse()
        .expect("failed to parse Alice's address");

    let (
        alice_state,
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
        Config::regtest(),
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
            Config::regtest(),
        )
        .await;

    let bob_fut = bob::swap::swap(
        bob_state,
        bob_event_loop_handle,
        bob_db,
        bob_btc_wallet.clone(),
        bob_xmr_wallet.clone(),
        OsRng,
        Uuid::new_v4(),
    );

    tokio::spawn(async move { bob_event_loop.run().await });

    let alice_swap_id = Uuid::new_v4();
    let alice_db_datadir = tempdir().unwrap();
    let alice_db = Database::open(alice_db_datadir.path()).unwrap();

    let alice_xmr_locked_fut = alice::swap::run_until(
        alice_state,
        alice::swap::is_xmr_locked,
        alice_event_loop_handle,
        alice_btc_wallet.clone(),
        alice_xmr_wallet.clone(),
        Config::regtest(),
        alice_swap_id,
        alice_db,
    );

    tokio::spawn(async move { alice_event_loop.run().await });

    // Wait until alice has locked xmr and bob has locked btc
    let (bob_state, alice_state) = try_join(bob_fut, alice_xmr_locked_fut).await.unwrap();

    let bob_state4 = if let BobState::BtcRefunded(state4) = bob_state {
        state4
    } else {
        panic!("Bob in unexpected state");
    };

    let alice_db = Database::open(alice_db_datadir.path()).unwrap();
    let (mut alice_event_loop, alice_event_loop_handle) =
        testutils::init_alice_eventloop(alice_multiaddr);

    let alice_state = alice::swap::swap(
        alice_state,
        alice_event_loop_handle,
        alice_btc_wallet.clone(),
        alice_xmr_wallet.clone(),
        Config::regtest(),
        alice_swap_id,
        alice_db,
    )
    .await
    .unwrap();
    tokio::spawn(async move { alice_event_loop.run().await });

    assert!(matches!(alice_state, AliceState::XmrRefunded));

    let btc_alice_final = alice_btc_wallet.as_ref().balance().await.unwrap();
    let btc_bob_final = bob_btc_wallet.as_ref().balance().await.unwrap();

    // lock_tx_bitcoin_fee is determined by the wallet, it is not necessarily equal
    // to TX_FEE
    let lock_tx_bitcoin_fee = bob_btc_wallet
        .transaction_fee(bob_state4.tx_lock_id())
        .await
        .unwrap();

    assert_eq!(btc_alice_final, alice_btc_starting_balance);

    // Alice or Bob could publish TxCancel. This means Bob could pay tx fees for
    // TxCancel and TxRefund or only TxRefund
    let btc_bob_final_alice_submitted_cancel = btc_bob_final
        == bob_btc_starting_balance
            - lock_tx_bitcoin_fee
            - bitcoin::Amount::from_sat(bitcoin::TX_FEE);

    let btc_bob_final_bob_submitted_cancel = btc_bob_final
        == bob_btc_starting_balance
            - lock_tx_bitcoin_fee
            - bitcoin::Amount::from_sat(2 * bitcoin::TX_FEE);
    assert!(btc_bob_final_alice_submitted_cancel || btc_bob_final_bob_submitted_cancel);

    alice_xmr_wallet.as_ref().0.refresh().await.unwrap();
    let xmr_alice_final = alice_xmr_wallet.as_ref().get_balance().await.unwrap();
    assert_eq!(xmr_alice_final, xmr_to_swap);

    bob_xmr_wallet.as_ref().0.refresh().await.unwrap();
    let xmr_bob_final = bob_xmr_wallet.as_ref().get_balance().await.unwrap();
    assert_eq!(xmr_bob_final, bob_xmr_starting_balance);
}
