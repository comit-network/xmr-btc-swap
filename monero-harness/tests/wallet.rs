use monero_harness::Monero;
use spectral::prelude::*;
use testcontainers::clients::Cli;

#[tokio::test]
async fn fund_transfer_and_check_tx_key() {
    let fund_alice: u64 = 1_000_000_000_000;
    let fund_bob = 0;

    let tc = Cli::default();
    let (monero, _containers) = Monero::new(&tc, Some("test".to_string()), None, vec![
        "alice".to_string(),
        "bob".to_string(),
    ])
    .await
    .unwrap();
    let alice_wallet = monero.wallet("alice").unwrap();
    let bob_wallet = monero.wallet("bob").unwrap();

    let alice_address = alice_wallet.address().await.unwrap().address;

    let transfer = monero.fund(&alice_address, fund_alice).await.unwrap();

    let refreshed = alice_wallet.inner().refresh().await.unwrap();
    assert_that(&refreshed.received_money).is_true();

    let got_alice_balance = alice_wallet.balance().await.unwrap();
    let got_bob_balance = bob_wallet.balance().await.unwrap();

    assert_that(&got_alice_balance).is_equal_to(fund_alice);
    assert_that(&got_bob_balance).is_equal_to(fund_bob);

    let tx_id = transfer.tx_hash;
    let tx_key = transfer.tx_key;

    let res = alice_wallet
        .inner()
        .check_tx_key(&tx_id, &tx_key, &alice_address)
        .await
        .expect("failed to check tx by key");

    assert_that!(res.received).is_equal_to(fund_alice);
}
