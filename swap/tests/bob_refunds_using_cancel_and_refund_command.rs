pub mod testutils;

use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};
use testutils::bob_run_until::is_btc_locked;
use testutils::FastCancelConfig;

#[tokio::test]
async fn given_bob_manually_refunds_after_btc_locked_bob_refunds() {
    testutils::setup_test(FastCancelConfig, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_btc_locked));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap));

        let bob_state = bob_swap.await??;
        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        let (bob_swap, bob_join_handle) = ctx.stop_and_resume_bob_from_db(bob_join_handle).await;

        // Ensure Bob's timelock is expired
        if let BobState::BtcLocked(state3) = bob_swap.state.clone() {
            bob_swap
                .bitcoin_wallet
                .subscribe_to(state3.tx_lock)
                .await
                .wait_until_confirmed_with(state3.cancel_timelock)
                .await?;
        } else {
            panic!("Bob in unexpected state {}", bob_swap.state);
        }

        // Bob manually cancels
        bob_join_handle.abort();
        let (_, state) = bob::cancel(
            bob_swap.swap_id,
            bob_swap.state,
            bob_swap.bitcoin_wallet,
            bob_swap.db,
            false,
        )
        .await??;
        assert!(matches!(state, BobState::BtcCancelled { .. }));

        let (bob_swap, bob_join_handle) = ctx.stop_and_resume_bob_from_db(bob_join_handle).await;
        assert!(matches!(bob_swap.state, BobState::BtcCancelled { .. }));

        // Bob manually refunds
        bob_join_handle.abort();
        let bob_state = bob::refund(
            bob_swap.swap_id,
            bob_swap.state,
            bob_swap.bitcoin_wallet,
            bob_swap.db,
            false,
        )
        .await??;

        ctx.assert_bob_refunded(bob_state).await;

        let alice_state = alice_swap.await??;
        ctx.assert_alice_refunded(alice_state).await;

        Ok(())
    })
    .await
}
