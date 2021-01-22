pub mod testutils;

use swap::protocol::{alice, alice::AliceState, bob};
use testutils::alice_run_until::is_encsig_learned;

#[tokio::test]
async fn given_alice_restarts_after_encsig_is_learned_resume_swap() {
    testutils::setup_test(|mut ctx| async move {
        let (alice_swap, alice_join_handle) = ctx.new_swap_as_alice().await;
        let (bob_swap, _) = ctx.new_swap_as_bob().await;

        let bob = bob::run(bob_swap);
        let bob_handle = tokio::spawn(bob);

        let alice_state = alice::run_until(alice_swap, is_encsig_learned)
            .await
            .unwrap();
        assert!(matches!(alice_state, AliceState::EncSigLearned {..}));

        let alice_swap = ctx.stop_and_resume_alice_from_db(alice_join_handle).await;
        assert!(matches!(alice_swap.state, AliceState::EncSigLearned {..}));

        let alice_state = alice::run(alice_swap).await.unwrap();

        ctx.assert_alice_redeemed(alice_state).await;

        let bob_state = bob_handle.await.unwrap();
        ctx.assert_bob_redeemed(bob_state.unwrap()).await
    })
    .await;
}
