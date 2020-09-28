use monero_harness::Monero;
use spectral::prelude::*;
use testcontainers::clients::Cli;

const ALICE_FUND_AMOUNT: u64 = 1_000_000_000_000;
const BOB_FUND_AMOUNT: u64 = 0;

fn init_cli() -> Cli {
    Cli::default()
}

async fn init_monero(tc: &'_ Cli) -> Monero<'_> {
    let monero = Monero::new(tc);
    let _ = monero.init(ALICE_FUND_AMOUNT, BOB_FUND_AMOUNT).await;

    monero
}

#[tokio::test]
async fn init_accounts_for_alice_and_bob() {
    let cli = init_cli();
    let monero = init_monero(&cli).await;

    let got_balance_alice = monero
        .get_balance_alice()
        .await
        .expect("failed to get alice's balance");

    let got_balance_bob = monero
        .get_balance_bob()
        .await
        .expect("failed to get bob's balance");

    assert_that!(got_balance_alice).is_equal_to(ALICE_FUND_AMOUNT);
    assert_that!(got_balance_bob).is_equal_to(BOB_FUND_AMOUNT);
}
