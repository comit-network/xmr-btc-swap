pub mod harness;

use bob::cancel::Error;
use harness::bob_run_until::is_btc_locked;
use harness::SlowCancelConfig;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

#[tokio::test]
async fn given_bob_manually_cancels_when_timelock_not_expired_errors() {
    harness::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_btc_locked));

        let alice_swap = ctx.alice_next_swap().await;
        let _ = tokio::spawn(alice::run(alice_swap));

        let bob_state = bob_swap.await??;
        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        let (bob_swap, bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));

        // Bob tries but fails to manually cancel
        let result = bob::cancel(bob_swap.id, bob_swap.bitcoin_wallet, bob_swap.db, false)
            .await?
            .err()
            .unwrap();

        assert!(matches!(result, Error::CancelTimelockNotExpiredYet));

        let (bob_swap, bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));

        // Bob tries but fails to manually refund
        bob::refund(bob_swap.id, bob_swap.bitcoin_wallet, bob_swap.db, false)
            .await?
            .err()
            .unwrap();

        let (bob_swap, _) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));

        Ok(())
    })
    .await;
}
