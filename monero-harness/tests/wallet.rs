use monero_harness::Monero;
use spectral::prelude::*;
use testcontainers::clients::Cli;

#[tokio::test]
async fn wallet_and_accounts() {
    let tc = Cli::default();
    let (monero, _monerod_container) = Monero::new_monerod(&tc).unwrap();
    let (wallet, _wallet_container) = Monero::new_wallet(&tc, "wallet").unwrap();
    // let cli = monero.miner_wallet_rpc_client();
    //
    // println!("creating wallet ...");
    //
    // let _ = cli
    //     .create_wallet("wallet")
    //     .await
    //     .expect("failed to create wallet");
    //
    // let got = cli.get_balance(0).await.expect("failed to get balance");
    // let want = 0;
    //
    // assert_that!(got).is_equal_to(want);
}

#[tokio::test]
async fn create_account_and_retrieve_it() {
    let tc = Cli::default();
    let (monero, _container) = Monero::new_monerod(&tc).unwrap();
    // let cli = monero.miner_wallet_rpc_client();
    //
    // let label = "Iron Man"; // This is intentionally _not_ Alice or Bob.
    //
    // let _ = cli
    //     .create_wallet("wallet")
    //     .await
    //     .expect("failed to create wallet");
    //
    // let _ = cli
    //     .create_account(label)
    //     .await
    //     .expect("failed to create account");
    //
    // let mut found: bool = false;
    // let accounts = cli
    //     .get_accounts("") // Empty filter.
    //     .await
    //     .expect("failed to get accounts");
    // for account in accounts.subaddress_accounts {
    //     if account.label == label {
    //         found = true;
    //     }
    // }
    // assert!(found);
}

#[tokio::test]
async fn transfer_and_check_tx_key() {
    let fund_alice: u64 = 1_000_000_000_000;
    let fund_bob = 0;

    let tc = Cli::default();
    let (monero, _container) = Monero::new_monerod(&tc).unwrap();
    // let _ = monero.init(fund_alice, fund_bob).await;
    //
    // let address_bob = monero
    //     .bob_wallet_rpc_client()
    //     .get_address(0)
    //     .await
    //     .expect("failed to get Bob's address")
    //     .address;
    //
    // let transfer_amount = 100;
    // let transfer = monero
    //     .alice_wallet_rpc_client()
    //     .transfer(0, transfer_amount, &address_bob)
    //     .await
    //     .expect("transfer failed");
    //
    // let tx_id = transfer.tx_hash;
    // let tx_key = transfer.tx_key;
    //
    // let cli = monero.miner_wallet_rpc_client();
    // let res = cli
    //     .check_tx_key(&tx_id, &tx_key, &address_bob)
    //     .await
    //     .expect("failed to check tx by key");
    //
    // assert_that!(res.received).is_equal_to(transfer_amount);
}
