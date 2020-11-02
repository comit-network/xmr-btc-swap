use monero_harness::Monero;
use spectral::prelude::*;
use testcontainers::clients::Cli;

#[tokio::test]
async fn fund_transfer_and_check_tx_key() {
    let fund_alice: u64 = 1_000_000_000_000;
    let fund_bob = 0;

    let tc = Cli::default();
    let (monerod, _monerod_container) = Monero::new_monerod(&tc).unwrap();

    let (miner_wallet, _wallet_container) = Monero::new_wallet(&tc, "miner").await.unwrap();
    let (alice_wallet, _alice_wallet_container) = Monero::new_wallet(&tc, "alice").await.unwrap();
    let (bob_wallet, _bob_wallet_container) = Monero::new_wallet(&tc, "bob").await.unwrap();

    let address = miner_wallet
        .wallet_rpc_client()
        .get_address(0)
        .await
        .unwrap()
        .address;

    monerod.start_miner(&address).await.unwrap();

    let block_height = monerod
        .monerod_rpc_client()
        .get_block_count()
        .await
        .unwrap();

    miner_wallet
        .wait_for_wallet_height(block_height)
        .await
        .unwrap();

    let alice_address = alice_wallet
        .wallet_rpc_client()
        .get_address(0)
        .await
        .unwrap()
        .address;

    let transfer = miner_wallet
        .transfer(&alice_address, fund_alice)
        .await
        .unwrap();

    monerod
        .monerod_rpc_client()
        .generate_blocks(10, &address)
        .await
        .unwrap();

    let refreshed = alice_wallet.wallet_rpc_client().refresh().await.unwrap();
    assert_that(&refreshed.received_money).is_true();

    let got_alice_balance = alice_wallet
        .wallet_rpc_client()
        .get_balance(0)
        .await
        .unwrap();

    let got_bob_balance = bob_wallet.wallet_rpc_client().get_balance(0).await.unwrap();

    assert_that(&got_alice_balance).is_equal_to(fund_alice);
    assert_that(&got_bob_balance).is_equal_to(fund_bob);

    let tx_id = transfer.tx_hash;
    let tx_key = transfer.tx_key;

    let res = alice_wallet
        .wallet_rpc_client()
        .check_tx_key(&tx_id, &tx_key, &alice_address)
        .await
        .expect("failed to check tx by key");

    assert_that!(res.received).is_equal_to(fund_alice);
}
