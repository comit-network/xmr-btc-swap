use monero_harness::{Monero, MoneroWalletRpc};
use std::time::Duration;
use testcontainers::clients::Cli;
use tokio::time::sleep;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::test]
async fn fund_transfer_and_check_tx_key() {
    let _guard = tracing_subscriber::fmt()
        .with_env_filter(
            "info,test=debug,monero_harness=debug,monero_rpc=debug,monero_sys=trace,wallet=trace,monero_cpp=trace",
        )
        .set_default();

    let fund_alice: u64 = 1_000_000_000_000;
    let fund_bob = 0;
    let send_to_bob = 5_000_000_000;

    let tc = Cli::default();
    let (monero, _monerod_container, _wallet_containers) =
        Monero::new(&tc, vec!["alice", "bob"]).await.unwrap();
    let alice_wallet = monero.wallet("alice").unwrap();
    let bob_wallet = monero.wallet("bob").unwrap();

    monero.init_miner().await.unwrap();
    monero.init_wallet("alice", vec![fund_alice]).await.unwrap();
    monero.init_wallet("bob", vec![fund_bob]).await.unwrap();
    monero.start_miner().await.unwrap();

    tracing::info!("Waiting for Alice to catch up");

    wait_for_wallet_to_catch_up(alice_wallet, fund_alice).await;

    // check alice balance
    let got_alice_balance = alice_wallet.balance().await.unwrap();
    assert_eq!(got_alice_balance, fund_alice, "Alice not funded");

    tracing::info!("Transferring funds from Alice to Bob");

    // transfer from alice to bob
    let bob_address = bob_wallet.address().await.unwrap();
    alice_wallet
        .transfer(&bob_address, send_to_bob)
        .await
        .unwrap();

    tracing::info!("Waiting for Bob to catch up");

    wait_for_wallet_to_catch_up(bob_wallet, send_to_bob).await;

    let got_bob_balance = bob_wallet.balance().await.unwrap();
    assert_eq!(send_to_bob, got_bob_balance, "Funds not transferred to Bob");
}

async fn wait_for_wallet_to_catch_up(wallet: &MoneroWalletRpc, expected_balance: u64) {
    for _ in 0..15 {
        tracing::info!(
            current_height = wallet.blockchain_height().await.unwrap(),
            "Waiting for wallet to catch up"
        );
        let balance = wallet.balance().await.unwrap();
        if balance == expected_balance {
            break;
        }
        wallet.refresh().await.unwrap();
        sleep(Duration::from_secs(2)).await;
    }
}
