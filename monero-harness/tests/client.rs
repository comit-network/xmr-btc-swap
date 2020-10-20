use monero_harness::Monero;
use spectral::prelude::*;
use testcontainers::clients::Cli;

const ALICE_FUND_AMOUNT: u64 = 1_000_000_000_000;
const BOB_FUND_AMOUNT: u64 = 0;

#[tokio::test]
async fn init_accounts_for_alice_and_bob() {
    let tc = Cli::default();
    let (monero, _container) = Monero::new(&tc);
    monero
        .init(ALICE_FUND_AMOUNT, BOB_FUND_AMOUNT)
        .await
        .unwrap();

    let got_balance_alice = monero
        .alice_wallet_rpc_client()
        .get_balance(0)
        .await
        .expect("failed to get alice's balance");

    let got_balance_bob = monero
        .bob_wallet_rpc_client()
        .get_balance(0)
        .await
        .expect("failed to get bob's balance");

    assert_that!(got_balance_alice).is_equal_to(ALICE_FUND_AMOUNT);
    assert_that!(got_balance_bob).is_equal_to(BOB_FUND_AMOUNT);
}
