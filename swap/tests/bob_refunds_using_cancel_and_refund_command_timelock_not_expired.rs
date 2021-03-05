pub mod testutils;

use bob::cancel::Error;
use swap::protocol::bob;
use swap::protocol::bob::BobState;
use testutils::bob_run_until::is_btc_locked;
use testutils::SlowCancelConfig;

#[tokio::test]
async fn given_bob_manually_cancels_when_timelock_not_expired_errors() {
    testutils::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.new_swap_as_bob().await;

        let bob_state = bob::run_until(bob_swap, is_btc_locked).await.unwrap();
        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        let (bob_swap, bob_join_handle) = ctx.stop_and_resume_bob_from_db(bob_join_handle).await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));

        // Bob tries but fails to manually cancel
        let result = bob::cancel(
            bob_swap.swap_id,
            bob_swap.state,
            bob_swap.bitcoin_wallet,
            bob_swap.db,
            false,
        )
        .await
        .unwrap()
        .err()
        .unwrap();

        assert!(matches!(result, Error::CancelTimelockNotExpiredYet));

        let (bob_swap, bob_join_handle) = ctx.stop_and_resume_bob_from_db(bob_join_handle).await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));

        // Bob tries but fails to manually refund
        bob::refund(
            bob_swap.swap_id,
            bob_swap.state,
            bob_swap.execution_params,
            bob_swap.bitcoin_wallet,
            bob_swap.db,
            false,
        )
        .await
        .unwrap()
        .err()
        .unwrap();

        let (bob_swap, _) = ctx.stop_and_resume_bob_from_db(bob_join_handle).await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));
    })
    .await;
}
