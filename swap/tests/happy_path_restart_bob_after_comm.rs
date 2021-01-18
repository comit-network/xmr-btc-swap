use swap::protocol::{alice, bob, bob::BobState};

pub mod testutils;

#[tokio::test]
async fn given_bob_restarts_after_encsig_is_sent_resume_swap() {
    testutils::setup_test(|test| async move {
        let alice_swap = test.new_swap_as_alice().await;
        let bob_swap = test.new_swap_as_bob().await;

        let alice = alice::run(alice_swap);
        let alice_handle = tokio::spawn(alice);

        let bob_state = bob::run_until(bob_swap, bob::swap::is_encsig_sent)
            .await
            .unwrap();

        assert!(matches!(bob_state, BobState::EncSigSent {..}));

        let bob_swap = test.recover_bob_from_db().await;
        assert!(matches!(bob_swap.state, BobState::EncSigSent {..}));

        let bob_state = bob::run(bob_swap).await.unwrap();

        test.assert_bob_redeemed(bob_state).await;

        let alice_state = alice_handle.await.unwrap();
        test.assert_alice_redeemed(alice_state.unwrap()).await;
    })
    .await;
}
