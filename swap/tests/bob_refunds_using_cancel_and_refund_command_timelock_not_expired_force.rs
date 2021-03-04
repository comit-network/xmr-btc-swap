pub mod testutils;

use swap::protocol::bob;
use swap::protocol::bob::BobState;
use testutils::bob_run_until::is_btc_locked;
use testutils::SlowCancelConfig;

#[tokio::test]
async fn given_bob_manually_forces_cancel_when_timelock_not_expired_errors() {
    testutils::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.new_swap_as_bob().await;

        let bob_state = bob::run_until(bob_swap, is_btc_locked).await.unwrap();
        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        let (bob_swap, bob_join_handle) = ctx.stop_and_resume_bob_from_db(bob_join_handle).await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));

        // Bob forces a cancel that will fail
        let is_error = bob::cancel(
            bob_swap.swap_id,
            bob_swap.state,
            bob_swap.bitcoin_wallet,
            bob_swap.db,
            true,
        )
        .await
        .is_err();

        assert!(is_error);

        let (bob_swap, bob_join_handle) = ctx.stop_and_resume_bob_from_db(bob_join_handle).await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));

        // Bob forces a refund that will fail
        let is_error = bob::refund(
            bob_swap.swap_id,
            bob_swap.state,
            bob_swap.execution_params,
            bob_swap.bitcoin_wallet,
            bob_swap.db,
            true,
        )
        .await
        .is_err();

        assert!(is_error);
        let (bob_swap, _) = ctx.stop_and_resume_bob_from_db(bob_join_handle).await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));
    })
    .await;
}
