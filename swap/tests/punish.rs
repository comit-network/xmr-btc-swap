use rand::rngs::OsRng;
use swap::protocol::{alice, bob, bob::BobState};

pub mod testutils;

/// Bob locks Btc and Alice locks Xmr. Bob does not act; he fails to send Alice
/// the encsig and fail to refund or redeem. Alice punishes.
#[tokio::test]
async fn alice_punishes_if_bob_never_acts_after_fund() {
    testutils::test(|alice_harness, bob_harness| async move {
        let alice = alice_harness.new_alice().await;
        let bob = bob_harness.new_bob().await;

        let alice_swap = alice::swap(
            alice.state,
            alice.event_loop_handle,
            alice.bitcoin_wallet.clone(),
            alice.monero_wallet.clone(),
            alice.config,
            alice.swap_id,
            alice.db,
        );
        let alice_swap_handle = tokio::spawn(alice_swap);

        let bob_state = bob::run_until(
            bob.state,
            bob::swap::is_btc_locked,
            bob.event_loop_handle,
            bob.db,
            bob.bitcoin_wallet.clone(),
            bob.monero_wallet.clone(),
            OsRng,
            bob.swap_id,
        )
        .await
        .unwrap();

        assert!(matches!(bob_state, BobState::BtcLocked {..}));

        let alice_state = alice_swap_handle.await.unwrap();
        alice_harness.assert_punished(alice_state.unwrap()).await;

        // Restart Bob after Alice punished to ensure Bob transitions to
        // punished and does not run indefinitely
        let bob = bob_harness.recover_bob_from_db().await;
        assert!(matches!(bob.state, BobState::BtcLocked {..}));

        // TODO: make lock-tx-id available in final states
        let lock_tx_id = if let BobState::BtcLocked(state3) = bob_state {
            state3.tx_lock_id()
        } else {
            panic!("Bob in unexpected state");
        };

        let bob_state = bob::swap(
            bob.state,
            bob.event_loop_handle,
            bob.db,
            bob.bitcoin_wallet.clone(),
            bob.monero_wallet.clone(),
            OsRng,
            bob.swap_id,
        )
        .await
        .unwrap();

        bob_harness.assert_punished(bob_state, lock_tx_id).await;
    })
    .await;
}
