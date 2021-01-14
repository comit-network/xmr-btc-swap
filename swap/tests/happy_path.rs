use crate::testutils::Test;
use swap::{bitcoin, monero};
use tokio::join;

pub mod testutils;

/// Run the following tests with RUST_MIN_STACK=10000000

#[tokio::test]
async fn happy_path() {
    testutils::test(|alice, bob| async move {
        join!(alice.swap(), bob.swap());

        alice.assert_btc_redeemed();
        bob.assert_btc_redeemed();
    }).await;
}
