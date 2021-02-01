pub mod testutils;

use swap::protocol::{alice, bob, bob::BobState};
use testutils::{bob_run_until::is_btc_locked, FastCancelConfig};

#[tokio::test]
async fn given_bob_manually_refunds_after_btc_locked_bob_refunds() {
    testutils::setup_test(FastCancelConfig, |mut ctx| async move {
        let (alice_swap, _) = ctx.new_swap_as_alice().await;
        let (bob_swap, bob_join_handle) = ctx.new_swap_as_bob().await;

        let alice_handle = alice::run(alice_swap);
        let alice_swap_handle = tokio::spawn(alice_handle);

        let bob_state = bob::run_until(bob_swap, is_btc_locked).await.unwrap();

        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        let (bob_swap, bob_join_handle) = ctx.stop_and_resume_bob_from_db(bob_join_handle).await;

        // Ensure Bob's timelock is expired
        if let BobState::BtcLocked(state3) = bob_swap.state.clone() {
            state3
                .wait_for_cancel_timelock_to_expire(bob_swap.bitcoin_wallet.as_ref())
                .await
                .unwrap();
        } else {
            panic!("Bob in unexpected state {}", bob_swap.state);
        }

        // Bob manually cancels
        let (_, state) = bob::cancel(
            bob_swap.swap_id,
            bob_swap.state,
            bob_swap.bitcoin_wallet,
            bob_swap.db,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(state, BobState::BtcCancelled { .. }));

        let (bob_swap, _) = ctx.stop_and_resume_bob_from_db(bob_join_handle).await;
        assert!(matches!(bob_swap.state, BobState::BtcCancelled { .. }));

        // Bob manually refunds
        let bob_state = bob::refund(
            bob_swap.swap_id,
            bob_swap.state,
            bob_swap.execution_params,
            bob_swap.bitcoin_wallet,
            bob_swap.db,
        )
        .await
        .unwrap()
        .unwrap();

        ctx.assert_bob_refunded(bob_state).await;

        let alice_state = alice_swap_handle.await.unwrap().unwrap();
        ctx.assert_alice_refunded(alice_state).await;
    })
    .await;
}
