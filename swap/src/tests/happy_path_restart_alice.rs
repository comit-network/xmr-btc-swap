use crate::{
    protocol::{alice, alice::AliceState, bob},
    tests::{alice_run_until::is_encsig_learned, setup_test},
};

#[tokio::test]
async fn given_alice_restarts_after_encsig_is_learned_resume_swap() {
    setup_test(|mut ctx| async move {
        let alice_swap = ctx.new_swap_as_alice().await;
        let bob_swap = ctx.new_swap_as_bob().await;

        let bob = bob::run(bob_swap);
        let bob_handle = tokio::spawn(bob);

        let alice_state = alice::run_until(alice_swap, is_encsig_learned)
            .await
            .unwrap();
        assert!(matches!(alice_state, AliceState::EncSigLearned {..}));

        let alice_swap = ctx.recover_alice_from_db().await;
        assert!(matches!(alice_swap.state, AliceState::EncSigLearned {..}));

        let alice_state = alice::run(alice_swap).await.unwrap();

        ctx.assert_alice_redeemed(alice_state).await;

        let bob_state = bob_handle.await.unwrap();
        ctx.assert_bob_redeemed(bob_state.unwrap()).await
    })
    .await;
}
