pub mod harness;

use harness::alice_run_until::is_xmr_lock_transaction_sent;
use harness::SlowCancelConfig;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::{alice, bob};

#[tokio::test]
async fn given_alice_restarts_after_xmr_is_locked_resume_swap() {
    harness::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (bob_swap, _) = ctx.bob_swap().await;
        let bob_swap = tokio::spawn(bob::run(bob_swap));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run_until(
            alice_swap,
            is_xmr_lock_transaction_sent,
            FixedRate::default(),
        ));

        let alice_state = alice_swap.await??;

        assert!(matches!(
            alice_state,
            AliceState::XmrLockTransactionSent { .. }
        ));

        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        assert!(matches!(
            alice_swap.state,
            AliceState::XmrLockTransactionSent { .. }
        ));

        let alice_state = alice::run(alice_swap, FixedRate::default()).await?;
        ctx.assert_alice_redeemed(alice_state).await;

        let bob_state = bob_swap.await??;
        ctx.assert_bob_redeemed(bob_state).await;

        Ok(())
    })
    .await;
}
