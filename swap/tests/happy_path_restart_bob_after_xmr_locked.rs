pub mod harness;

use harness::bob_run_until::is_xmr_locked;
use harness::SlowCancelConfig;
use swap::asb::FixedRate;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

#[tokio::test]
async fn given_bob_restarts_after_xmr_is_locked_resume_swap() {
    harness::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_xmr_locked));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let bob_state = bob_swap.await??;

        assert!(matches!(bob_state, BobState::XmrLocked { .. }));

        let (bob_swap, _) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;
        assert!(matches!(bob_swap.state, BobState::XmrLocked { .. }));

        let bob_state = bob::run(bob_swap).await?;

        ctx.assert_bob_redeemed(bob_state).await;

        let alice_state = alice_swap.await??;
        ctx.assert_alice_redeemed(alice_state).await;

        Ok(())
    })
    .await;
}
