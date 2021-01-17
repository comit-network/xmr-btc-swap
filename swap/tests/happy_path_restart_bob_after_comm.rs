use rand::rngs::OsRng;
use swap::protocol::{alice, bob, bob::BobState};

pub mod testutils;

#[tokio::test]
async fn given_bob_restarts_after_encsig_is_sent_resume_swap() {
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
            bob::swap::is_encsig_sent,
            bob.event_loop_handle,
            bob.db,
            bob.bitcoin_wallet.clone(),
            bob.monero_wallet.clone(),
            OsRng,
            bob.swap_id,
        )
        .await
        .unwrap();

        assert!(matches!(bob_state, BobState::EncSigSent {..}));

        let bob = bob_harness.recover_bob_from_db().await;
        assert!(matches!(bob.state, BobState::EncSigSent {..}));

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

        bob_harness.assert_redeemed(bob_state).await;

        let alice_state = alice_swap_handle.await.unwrap();
        alice_harness.assert_redeemed(alice_state.unwrap()).await;
    })
    .await;
}
