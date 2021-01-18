use swap::protocol::{alice, alice::AliceState, bob};

pub mod testutils;

/// Bob locks btc and Alice locks xmr. Alice fails to act so Bob refunds. Alice
/// then also refunds.
#[tokio::test]
async fn given_alice_restarts_after_xmr_is_locked_abort_swap() {
    testutils::setup_test(|test| async move {
        let alice_swap = test.new_swap_as_alice().await;
        let bob_swap = test.new_swap_as_bob().await;

        let bob = bob::run(bob_swap);
        let bob_handle = tokio::spawn(bob);

        let alice_state = alice::run_until(alice_swap, alice::swap::is_xmr_locked)
            .await
            .unwrap();
        assert!(matches!(alice_state, AliceState::XmrLocked {..}));

        // Alice does not act, Bob refunds
        let bob_state = bob_handle.await.unwrap();

        // Once bob has finished Alice is restarted and refunds as well
        let alice_swap = test.recover_alice_from_db().await;
        assert!(matches!(alice_swap.state, AliceState::XmrLocked {..}));

        let alice_state = alice::run(alice_swap).await.unwrap();

        // TODO: The test passes like this, but the assertion should be done after Bob
        // refunded, not at the end because this can cause side-effects!
        //  We have to properly wait for the refund tx's finality inside the assertion,
        // which requires storing the refund_tx_id in the the state!
        test.assert_bob_refunded(bob_state.unwrap()).await;
        test.assert_alice_refunded(alice_state).await;
    })
    .await;
}
