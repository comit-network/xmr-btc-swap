pub mod testutils;

use swap::protocol::{alice, alice::AliceState, bob};
use testutils::alice_run_until::is_xmr_locked;

/// Bob locks btc and Alice locks xmr. Alice fails to act so Bob refunds. Alice
/// then also refunds.
#[tokio::test]
async fn given_alice_restarts_after_xmr_is_locked_refund_swap() {
    testutils::setup_test(|mut ctx| async move {
        let (alice_swap, alice_join_handle) = ctx.new_swap_as_alice().await;
        let (bob_swap, _) = ctx.new_swap_as_bob().await;

        let bob = bob::run(bob_swap);
        let bob_handle = tokio::spawn(bob);

        let alice_state = alice::run_until(alice_swap, is_xmr_locked).await.unwrap();
        assert!(matches!(alice_state,
        AliceState::XmrLocked {..}));

        // Alice does not act, Bob refunds
        let bob_state = bob_handle.await.unwrap();
        ctx.assert_bob_refunded(bob_state.unwrap()).await;

        // Once bob has finished Alice is restarted and refunds as well
        let alice_swap = ctx.stop_and_resume_alice_from_db(alice_join_handle).await;
        assert!(matches!(alice_swap.state, AliceState::XmrLocked {..}));

        let alice_state = alice::run(alice_swap).await.unwrap();

        ctx.assert_alice_refunded(alice_state).await;
    })
    .await;
}
