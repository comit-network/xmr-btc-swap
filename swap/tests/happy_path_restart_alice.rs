use rand::rngs::OsRng;
use swap::protocol::{alice, alice::AliceState, bob, bob::BobState};

pub mod testutils;

#[tokio::test]
async fn given_alice_restarts_after_encsig_is_learned_resume_swap() {
    testutils::test(|alice_harness, bob, swap_amounts| async move {
        let alice = alice_harness.new_alice().await;

        let bob_swap_fut = bob::swap(
            bob.state,
            bob.event_loop_handle,
            bob.db,
            bob.bitcoin_wallet.clone(),
            bob.monero_wallet.clone(),
            OsRng,
            bob.swap_id,
        );
        let bob_swap_handle = tokio::spawn(bob_swap_fut);

        let alice_state = alice::run_until(
            alice.state,
            alice::swap::is_encsig_learned,
            alice.event_loop_handle,
            alice.bitcoin_wallet.clone(),
            alice.monero_wallet.clone(),
            alice.config,
            alice.swap_id,
            alice.db,
        )
        .await
        .unwrap();
        assert!(matches!(alice_state, AliceState::EncSigLearned {..}));

        let alice = alice_harness.recover_alice_from_db().await;
        assert!(matches!(alice.state, AliceState::EncSigLearned {..}));

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

        alice_harness.assert_redeemed(alice_state).await;

        let bob_state = bob_swap_handle.await.unwrap();
        let btc_bob_final = bob.bitcoin_wallet.as_ref().balance().await.unwrap();
        bob.monero_wallet.as_ref().inner.refresh().await.unwrap();
        let xmr_bob_final = bob.monero_wallet.as_ref().get_balance().await.unwrap();
        assert!(matches!(bob_state.unwrap(), BobState::XmrRedeemed));
        assert!(btc_bob_final <= bob.btc_starting_balance - swap_amounts.btc);
        assert_eq!(xmr_bob_final, bob.xmr_starting_balance + swap_amounts.xmr);
    })
    .await;
}
