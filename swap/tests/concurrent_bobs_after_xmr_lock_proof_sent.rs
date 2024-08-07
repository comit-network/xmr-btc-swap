pub mod harness;

use harness::bob_run_until::is_xmr_locked;
use harness::SlowCancelConfig;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

#[tokio::test]
async fn concurrent_bobs_after_xmr_lock_proof_sent() {
    harness::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (bob_swap_1, bob_join_handle_1) = ctx.bob_swap().await;

        let swap_id = bob_swap_1.id;

        let bob_swap_1 = tokio::spawn(bob::run_until(bob_swap_1, is_xmr_locked));

        let alice_swap_1 = ctx.alice_next_swap().await;
        let alice_swap_1 = tokio::spawn(alice::run(alice_swap_1, FixedRate::default()));

        let bob_state_1 = bob_swap_1.await??;
        assert!(matches!(bob_state_1, BobState::XmrLocked { .. }));

        // make sure bob_swap_1's event loop is gone
        bob_join_handle_1.abort();

        let (bob_swap_2, bob_join_handle_2) = ctx.bob_swap().await;
        let bob_swap_2 = tokio::spawn(bob::run(bob_swap_2));

        let alice_swap_2 = ctx.alice_next_swap().await;
        let alice_swap_2 = tokio::spawn(alice::run(alice_swap_2, FixedRate::default()));

        // The 2nd swap should ALWAYS finish successfully in this
        // scenario

        let bob_state_2 = bob_swap_2.await??;
        assert!(matches!(bob_state_2, BobState::XmrRedeemed { .. }));

        let alice_state_2 = alice_swap_2.await??;
        assert!(matches!(alice_state_2, AliceState::BtcRedeemed { .. }));

        let (bob_swap_1, _) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle_2, swap_id)
            .await;
        assert!(matches!(bob_swap_1.state, BobState::XmrLocked { .. }));

        // The 1st (paused) swap ALWAYS finishes successfully in this
        // scenario, because it is ensured that Bob already received the
        // transfer proof.

        let bob_state_1 = bob::run(bob_swap_1).await?;
        assert!(matches!(bob_state_1, BobState::XmrRedeemed { .. }));

        let alice_state_1 = alice_swap_1.await??;
        assert!(matches!(alice_state_1, AliceState::BtcRedeemed { .. }));

        Ok(())
    })
    .await;
}
