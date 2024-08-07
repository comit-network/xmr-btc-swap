pub mod harness;

use harness::alice_run_until::is_xmr_lock_transaction_sent;
use harness::bob_run_until::is_btc_locked;
use harness::FastCancelConfig;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};
use swap::{asb, cli};

#[tokio::test]
async fn given_alice_and_bob_manually_cancel_and_refund_after_funds_locked_both_refund() {
    harness::setup_test(FastCancelConfig, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_btc_locked));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run_until(
            alice_swap,
            is_xmr_lock_transaction_sent,
            FixedRate::default(),
        ));

        let bob_state = bob_swap.await??;
        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        let alice_state = alice_swap.await??;
        assert!(matches!(
            alice_state,
            AliceState::XmrLockTransactionSent { .. }
        ));

        let (bob_swap, bob_join_handle) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;

        // Ensure cancel timelock is expired
        if let BobState::BtcLocked { state3, .. } = bob_swap.state.clone() {
            bob_swap
                .bitcoin_wallet
                .subscribe_to(state3.tx_lock)
                .await
                .wait_until_confirmed_with(state3.cancel_timelock)
                .await?;
        } else {
            panic!("Bob in unexpected state {}", bob_swap.state);
        }

        // Bob manually cancels and refunds
        bob_join_handle.abort();
        let bob_state =
            cli::cancel_and_refund(bob_swap.id, bob_swap.bitcoin_wallet, bob_swap.db).await?;

        ctx.assert_bob_refunded(bob_state).await;

        // manually refund Alice's swap
        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        let alice_state = asb::refund(
            alice_swap.swap_id,
            alice_swap.bitcoin_wallet,
            alice_swap.monero_wallet,
            alice_swap.db,
        )
        .await?;

        ctx.assert_alice_refunded(alice_state).await;

        Ok(())
    })
    .await
}
