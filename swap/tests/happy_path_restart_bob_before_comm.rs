use swap::protocol::{
    alice, bob,
    bob::{swap::is_xmr_locked, BobState},
};

pub mod testutils;

#[tokio::test]
async fn given_bob_restarts_after_xmr_is_locked_resume_swap() {
    testutils::setup_test(|test| async move {
        let alice_swap = test.new_swap_as_alice().await;
        let bob_swap = test.new_swap_as_bob().await;

        let alice_handle = alice::run(alice_swap);
        let alice_swap_handle = tokio::spawn(alice_handle);

        let bob_state = bob::run_until(bob_swap, is_xmr_locked).await.unwrap();

        assert!(matches!(bob_state, BobState::XmrLocked {..}));

        let bob_swap = test.recover_bob_from_db().await;
        assert!(matches!(bob_swap.state, BobState::XmrLocked {..}));

        let bob_state = bob::run(bob_swap).await.unwrap();

        test.assert_bob_redeemed(bob_state).await;

        let alice_state = alice_swap_handle.await.unwrap();
        test.assert_alice_redeemed(alice_state.unwrap()).await;
    })
    .await;
}
