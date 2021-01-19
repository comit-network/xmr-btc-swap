use swap::protocol::{alice, alice::AliceState, bob};

pub mod testutils;

/// Bob locks btc and Alice locks xmr. Alice fails to act so Bob refunds. Alice
/// is forced to refund even though she learned the secret and would be able to
/// redeem had the timelock not expired.
#[tokio::test]
async fn given_alice_restarts_after_enc_sig_learned_and_bob_already_cancelled_refund_swap() {
    testutils::setup_test(|mut ctx| async move {
        let alice_swap = ctx.new_swap_as_alice().await;
        let bob_swap = ctx.new_swap_as_bob().await;

        let bob = bob::run(bob_swap);
        let bob_handle = tokio::spawn(bob);

        let alice_state = alice::run_until(alice_swap, alice::swap::is_encsig_learned)
            .await
            .unwrap();
        assert!(matches!(alice_state, AliceState::EncSigLearned {..}));

        // Wait for Bob to refund, because Alice does not act
        let bob_state = bob_handle.await.unwrap();
        ctx.assert_bob_refunded(bob_state.unwrap()).await;

        // Once bob has finished Alice is restarted and refunds as well
        let alice_swap = ctx.recover_alice_from_db().await;
        assert!(matches!(alice_swap.state, AliceState::EncSigLearned {..}));

        let alice_state = alice::run(alice_swap).await.unwrap();

        ctx.assert_alice_refunded(alice_state).await;
    })
    .await;
}
