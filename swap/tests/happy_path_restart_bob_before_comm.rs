pub mod testutils;

use swap::protocol::bob;
use swap::protocol::bob::BobState;
use testutils::bob_run_until::is_xmr_locked;
use testutils::SlowCancelConfig;

#[tokio::test]
async fn given_bob_restarts_after_xmr_is_locked_resume_swap() {
    testutils::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.new_swap_as_bob().await;

        let bob_state = bob::run_until(bob_swap, is_xmr_locked).await.unwrap();

        assert!(matches!(bob_state, BobState::XmrLocked { .. }));

        let (bob_swap, _) = ctx.stop_and_resume_bob_from_db(bob_join_handle).await;
        assert!(matches!(bob_swap.state, BobState::XmrLocked { .. }));

        let bob_state = bob::run(bob_swap).await.unwrap();

        ctx.assert_bob_redeemed(bob_state).await;

        ctx.assert_alice_redeemed().await;
    })
    .await;
}
