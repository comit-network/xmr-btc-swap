use rand::rngs::OsRng;
use swap::protocol::{alice, alice::AliceState, bob};

pub mod testutils;

/// Bob locks btc and Alice locks xmr. Alice fails to act so Bob refunds. Alice
/// then also refunds.
#[tokio::test]
async fn given_alice_restarts_after_xmr_is_locked_abort_swap() {
    testutils::test(|alice_harness, bob_harness| async move {
        let alice = alice_harness.new_alice().await;
        let bob = bob_harness.new_bob().await;

        let bob_swap = bob::swap(
            bob.state,
            bob.event_loop_handle,
            bob.db,
            bob.bitcoin_wallet.clone(),
            bob.monero_wallet.clone(),
            OsRng,
            bob.swap_id,
        );
        let bob_swap_handle = tokio::spawn(bob_swap);

        let alice_state = alice::run_until(
            alice.state,
            alice::swap::is_xmr_locked,
            alice.event_loop_handle,
            alice.bitcoin_wallet.clone(),
            alice.monero_wallet.clone(),
            alice.config,
            alice.swap_id,
            alice.db,
        )
        .await
        .unwrap();
        assert!(matches!(alice_state, AliceState::XmrLocked {..}));

        // Alice does not act, Bob refunds
        let bob_state = bob_swap_handle.await.unwrap();

        // Once bob has finished Alice is restarted and refunds as well
        let alice = alice_harness.recover_alice_from_db().await;
        assert!(matches!(alice.state, AliceState::XmrLocked {..}));

        let alice_state = alice::swap(
            alice.state,
            alice.event_loop_handle,
            alice.bitcoin_wallet.clone(),
            alice.monero_wallet.clone(),
            alice.config,
            alice.swap_id,
            alice.db,
        )
        .await
        .unwrap();

        // TODO: The test passes like this, but the assertion should be done after Bob
        // refunded, not at the end because this can cause side-effects!
        //  We have to properly wait for the refund tx's finality inside the assertion,
        // which requires storing the refund_tx_id in the the state!
        bob_harness.assert_refunded(bob_state.unwrap()).await;
        alice_harness.assert_refunded(alice_state).await;
    })
    .await;
}
