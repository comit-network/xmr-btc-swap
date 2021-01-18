use swap::protocol::{alice, alice::AliceState, bob};

pub mod testutils;

#[tokio::test]
async fn given_alice_restarts_after_encsig_is_learned_resume_swap() {
    testutils::init(|test| async move {
        let alice_swap = test.new_swap_as_alice().await;
        let bob_swap = test.new_swap_as_bob().await;

        let bob = bob::run(bob_swap);
        let bob_handle = tokio::spawn(bob);

        let alice_state = alice::run_until(alice_swap, alice::swap::is_encsig_learned)
            .await
            .unwrap();
        assert!(matches!(alice_state, AliceState::EncSigLearned {..}));

        let alice_swap = test.recover_alice_from_db().await;
        assert!(matches!(alice_swap.state, AliceState::EncSigLearned {..}));

        let alice_state = alice::run(alice_swap).await.unwrap();

        test.assert_alice_redeemed(alice_state).await;

        let bob_state = bob_handle.await.unwrap();
        test.assert_bob_redeemed(bob_state.unwrap()).await
    })
    .await;
}
