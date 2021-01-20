use swap::protocol::{
    alice, bob,
    bob::{swap::is_btc_locked, BobState},
};

pub mod testutils;

/// Bob locks Btc and Alice locks Xmr. Bob does not act; he fails to send Alice
/// the encsig and fail to refund or redeem. Alice punishes.
#[tokio::test]
async fn alice_punishes_if_bob_never_acts_after_fund() {
    testutils::setup_test(|ctx| async move {
        let alice_swap = ctx.new_swap_as_alice().await;
        let bob_swap = ctx.new_swap_as_bob().await;

        let alice = alice::run(alice_swap);
        let alice_handle = tokio::spawn(alice);

        let bob_state = bob::run_until(bob_swap, is_btc_locked).await.unwrap();

        assert!(matches!(bob_state, BobState::BtcLocked {..}));

        let alice_state = alice_handle.await.unwrap();
        ctx.assert_alice_punished(alice_state.unwrap()).await;

        // Restart Bob after Alice punished to ensure Bob transitions to
        // punished and does not run indefinitely
        let bob_swap = ctx.recover_bob_from_db().await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked {..}));

        let bob_state = bob::run(bob_swap).await.unwrap();

        ctx.assert_bob_punished(bob_state).await;
    })
    .await;
}
