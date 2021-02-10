pub mod testutils;

use swap::protocol::bob;
use testutils::SlowCancelConfig;

/// Run the following tests with RUST_MIN_STACK=10000000

#[tokio::test]
async fn happy_path() {
    testutils::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (bob_swap, _) = ctx.new_swap_as_bob().await;

        let bob_state = bob::run(bob_swap).await;

        ctx.assert_alice_redeemed().await;
        ctx.assert_bob_redeemed(bob_state.unwrap()).await;
    })
    .await;
}
