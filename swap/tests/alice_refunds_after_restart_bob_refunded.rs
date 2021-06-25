pub mod harness;

use harness::alice_run_until::is_xmr_lock_transaction_sent;
use harness::FastCancelConfig;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::{alice, bob};

/// Bob locks Btc and Alice locks Xmr. Alice does not act so Bob refunds.
/// Eventually Alice comes back online and refunds as well.
#[tokio::test]
async fn alice_refunds_after_restart_if_bob_already_refunded() {
    harness::setup_test(FastCancelConfig, |mut ctx| async move {
        let (bob_swap, _) = ctx.bob_swap().await;
        let bob_swap = tokio::spawn(bob::run(bob_swap));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run_until(
            alice_swap,
            is_xmr_lock_transaction_sent,
            FixedRate::default(),
        ));

        let bob_state = bob_swap.await??;
        ctx.assert_bob_refunded(bob_state).await;

        let alice_state = alice_swap.await??;
        assert!(matches!(
            alice_state,
            AliceState::XmrLockTransactionSent { .. }
        ));

        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let alice_state = alice_swap.await??;
        ctx.assert_alice_refunded(alice_state).await;

        Ok(())
    })
    .await;
}
