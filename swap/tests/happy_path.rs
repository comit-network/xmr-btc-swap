use crate::testutils::Test;
use swap::{bitcoin, monero};
use tokio::join;

pub mod testutils;

/// Run the following tests with RUST_MIN_STACK=10000000

#[tokio::test]
async fn happy_path() {
    let mut test = Test::new(
        bitcoin::Amount::from_sat(1_000_000),
        monero::Amount::from_piconero(1_000_000_000_000),
    )
    .await;

    join!(test.alice.swap(), test.bob.swap());

    test.alice.assert_btc_redeemed();
    test.bob.assert_btc_redeemed();
}
