use monero_harness::Monero;
use spectral::prelude::*;
use testcontainers::clients::Cli;

#[tokio::test]
async fn fund_transfer_and_check_tx_key() {
    let fund_alice: u64 = 1_000_000_000_000;
    let fund_bob = 0;
    let send_to_bob = 5_000_000_000;

    let tc = Cli::default();
    let (monero, _containers) = Monero::new(&tc, Some("test_".to_string()), vec![
        "alice".to_string(),
        "bob".to_string(),
    ])
    .await
    .unwrap();
    let alice_wallet = monero.wallet("alice").unwrap();
    let bob_wallet = monero.wallet("bob").unwrap();

    // fund alice
    monero.init(fund_alice, fund_bob).await.unwrap();

    // check alice balance
    alice_wallet.inner().refresh().await.unwrap();
    let got_alice_balance = alice_wallet.balance().await.unwrap();
    assert_that(&got_alice_balance).is_equal_to(fund_alice);

    // transfer from alice to bob
    let bob_address = bob_wallet.address().await.unwrap().address;
    let transfer = monero
        .transfer_from_alice(&bob_address, send_to_bob)
        .await
        .unwrap();

    bob_wallet.inner().refresh().await.unwrap();
    let got_bob_balance = bob_wallet.balance().await.unwrap();
    assert_that(&got_bob_balance).is_equal_to(send_to_bob);

    // check if tx was actually seen
    let tx_id = transfer.tx_hash;
    let tx_key = transfer.tx_key;
    let res = bob_wallet
        .inner()
        .check_tx_key(&tx_id, &tx_key, &bob_address)
        .await
        .expect("failed to check tx by key");

    assert_that!(res.received).is_equal_to(send_to_bob);
}
