use monero_harness::Monero;
use monero_rpc::monerod::MonerodRpc as _;
use spectral::prelude::*;
use std::time::Duration;
use testcontainers::clients::Cli;
use tokio::time;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::test]
async fn init_miner_and_mine_to_miner_address() {
    let _guard = tracing_subscriber::fmt()
        .with_env_filter("warn,test=debug,monero_harness=debug,monero_rpc=debug")
        .set_default();

    let tc = Cli::default();
    let (monero, _monerod_container, _wallet_containers) = Monero::new(&tc, vec![]).await.unwrap();

    monero.init_and_start_miner().await.unwrap();

    let monerod = monero.monerod();
    let miner_wallet = monero.wallet("miner").unwrap();

    let got_miner_balance = miner_wallet.balance().await.unwrap();
    assert_that!(got_miner_balance).is_greater_than(0);

    time::sleep(Duration::from_millis(1010)).await;

    // after a bit more than 1 sec another block should have been mined
    let block_height = monerod.client().get_block_count().await.unwrap().count;

    assert_that(&block_height).is_greater_than(70);
}
