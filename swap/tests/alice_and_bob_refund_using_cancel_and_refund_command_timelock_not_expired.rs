pub mod harness;

use harness::alice_run_until::is_xmr_lock_transaction_sent;
use harness::bob_run_until::is_btc_locked;
use harness::SlowCancelConfig;
use swap::protocol::alice::event_loop::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

#[tokio::test]
async fn given_alice_and_bob_manually_cancel_when_timelock_not_expired_errors() {
    harness::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_btc_locked));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run_until(
            alice_swap,
            is_xmr_lock_transaction_sent,
            FixedRate::default(),
        ));

        let bob_state = bob_swap.await??;
        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        let (bob_swap, bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, swap_id)
            .await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));

        let alice_state = alice_swap.await??;
        assert!(matches!(
            alice_state,
            AliceState::XmrLockTransactionSent { .. }
        ));

        // Bob tries but fails to manually cancel
        let result = bob::cancel(bob_swap.id, bob_swap.bitcoin_wallet, bob_swap.db, false)
            .await?
            .unwrap_err();
        assert!(matches!(
            result,
            bob::cancel::Error::CancelTimelockNotExpiredYet
        ));

        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        assert!(matches!(
            alice_swap.state,
            AliceState::XmrLockTransactionSent { .. }
        ));

        // Alice tries but fails manual cancel
        let result = alice::cancel(
            alice_swap.swap_id,
            alice_swap.bitcoin_wallet,
            alice_swap.db,
            false,
        )
        .await?
        .unwrap_err();
        assert!(matches!(
            result,
            alice::cancel::Error::CancelTimelockNotExpiredYet
        ));

        let (bob_swap, bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, swap_id)
            .await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));

        // Bob tries but fails to manually refund
        let result = bob::refund(bob_swap.id, bob_swap.bitcoin_wallet, bob_swap.db, false)
            .await?
            .unwrap_err();
        assert!(matches!(result, bob::refund::SwapNotCancelledYet(_)));

        let (bob_swap, _) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, swap_id)
            .await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));

        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        assert!(matches!(
            alice_swap.state,
            AliceState::XmrLockTransactionSent { .. }
        ));

        // Alice tries but fails manual cancel
        let result = alice::refund(
            alice_swap.swap_id,
            alice_swap.bitcoin_wallet,
            alice_swap.monero_wallet,
            alice_swap.db,
            false,
        )
        .await?
        .unwrap_err();
        assert!(matches!(result, alice::refund::Error::SwapNotCancelled));

        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        assert!(matches!(
            alice_swap.state,
            AliceState::XmrLockTransactionSent { .. }
        ));

        Ok(())
    })
    .await;
}
