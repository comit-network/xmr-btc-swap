use monero_harness::Monero;
use spectral::prelude::*;
use std::time::Duration;
use testcontainers::clients::Cli;
use tokio::time;

#[tokio::test]
async fn init_miner_and_mine_to_miner_address() {
    let tc = Cli::default();
    let (monero, _monerod_container) = Monero::new(&tc, None, None, vec![]).await.unwrap();

    let monerod = monero.monerod();
    let miner_wallet = monero.wallet("miner").unwrap();

    let got_miner_balance = miner_wallet.balance().await.unwrap();
    assert_that!(got_miner_balance).is_greater_than(0);

    time::delay_for(Duration::from_millis(1010)).await;

    // after a bit more than 1 sec another block should have been mined
    let block_height = monerod.inner().get_block_count().await.unwrap();

    assert_that(&block_height).is_greater_than(70);
}
