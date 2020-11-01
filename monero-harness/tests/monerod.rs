use monero_harness::Monero;
use spectral::prelude::*;
use std::time::Duration;
use testcontainers::clients::Cli;
use tokio::time;

#[tokio::test]
async fn init_miner_and_mine_to_miner_address() {
    let tc = Cli::default();
    let (monerod, _monerod_container) = Monero::new_monerod(&tc).unwrap();

    let (miner_wallet, _wallet_container) = Monero::new_wallet(&tc, "miner").await.unwrap();

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

    let got_miner_balance = miner_wallet
        .wallet_rpc_client()
        .get_balance(0)
        .await
        .unwrap();
    assert_that!(got_miner_balance).is_greater_than(0);

    time::delay_for(Duration::from_millis(1010)).await;

    // after a bit more than 1 sec another block should have been mined
    let block_height = monerod
        .monerod_rpc_client()
        .get_block_count()
        .await
        .unwrap();

    assert_that(&block_height).is_greater_than(70);
}
