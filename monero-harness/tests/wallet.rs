use crate::testutils::init_tracing;
use monero_harness::{Monero, MoneroWalletRpc};
use spectral::prelude::*;
use std::time::Duration;
use testcontainers::clients::Cli;
use tokio::time::sleep;

mod testutils;

#[tokio::test]
async fn fund_transfer_and_check_tx_key() {
    let _guard = init_tracing();

    let fund_alice: u64 = 1_000_000_000_000;
    let fund_bob = 0;
    let send_to_bob = 5_000_000_000;

    let tc = Cli::default();
    let (monero, _containers) = Monero::new(&tc, vec!["alice".to_string(), "bob".to_string()])
        .await
        .unwrap();
    let alice_wallet = monero.wallet("alice").unwrap();
    let bob_wallet = monero.wallet("bob").unwrap();

    // fund alice
    monero
        .init(vec![("alice", fund_alice), ("bob", fund_bob)])
        .await
        .unwrap();

    // check alice balance
    let got_alice_balance = alice_wallet.balance().await.unwrap();
    assert_that(&got_alice_balance).is_equal_to(fund_alice);

    // transfer from alice to bob
    let bob_address = bob_wallet.address().await.unwrap().address;
    let transfer = alice_wallet
        .transfer(&bob_address, send_to_bob)
        .await
        .unwrap();

    wait_for_wallet_to_catch_up(bob_wallet, send_to_bob).await;

    let got_bob_balance = bob_wallet.balance().await.unwrap();
    assert_that(&got_bob_balance).is_equal_to(send_to_bob);

    // check if tx was actually seen
    let tx_id = transfer.tx_hash;
    let tx_key = transfer.tx_key;
    let res = bob_wallet
        .client()
        .check_tx_key(&tx_id, &tx_key, &bob_address)
        .await
        .expect("failed to check tx by key");

    assert_that!(res.received).is_equal_to(send_to_bob);
}

async fn wait_for_wallet_to_catch_up(wallet: &MoneroWalletRpc, expected_balance: u64) {
    let max_retry = 15;
    let mut retry = 0;
    loop {
        retry += 1;
        let balance = wallet.balance().await.unwrap();
        if balance == expected_balance || max_retry == retry {
            break;
        }
        sleep(Duration::from_secs(1)).await;
    }
}
