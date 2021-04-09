pub mod harness;

use harness::bob_run_until::is_btc_locked;
use harness::SlowCancelConfig;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

#[tokio::test]
async fn concurrent_bobs_before_xmr_lock_proof_sent() {
    harness::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (bob_swap_1, bob_join_handle_1) = ctx.bob_swap().await;
        let bob_swap_id_1 = bob_swap_1.swap_id;
        let bob_bitcoin_wallet = bob_swap_1.bitcoin_wallet.clone();
        let bob_swap_1 = tokio::spawn(bob::run_until(bob_swap_1, is_btc_locked));

        let alice_swap_1 = ctx.alice_next_swap().await;
        let alice_swap_1 = tokio::spawn(alice::run(alice_swap_1));

        let bob_state_1 = bob_swap_1.await??;
        assert!(matches!(bob_state_1, BobState::BtcLocked(_)));

        // make sure Bob's swap one event loop is gone
        bob_join_handle_1.abort();
        // sync wallet to ensure lock tx for second swap works out
        bob_bitcoin_wallet.sync().await?;

        let (bob_swap_2, bob_join_handle_2) = ctx.bob_swap().await;
        let bob_swap_2 = tokio::spawn(bob::run(bob_swap_2));

        let alice_swap_2 = ctx.alice_next_swap().await;
        let alice_swap_2 = tokio::spawn(alice::run(alice_swap_2));

        // The second (inner) swap should ALWAYS finish successfully in this
        // scenario, but MIGHT receive an "unwanted" transfer proof that is ignored.

        // TODO: The inner swap (bob_swap_2) currently does not succeed properly.
        //  It DOES receive a transfer proof that does not match the swap and prints a
        //  warning - which is expected!  But then it never receives the actual transfer
        //  proof.

        let bob_state_2 = bob_swap_2.await??;
        assert!(matches!(bob_state_2, BobState::XmrRedeemed { .. }));

        let alice_state_2 = alice_swap_2.await??;
        assert!(matches!(alice_state_2, AliceState::BtcRedeemed { .. }));

        let (bob_swap_1, _) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle_2, bob_swap_id_1)
            .await;
        assert!(matches!(bob_state_1, BobState::BtcLocked(_)));

        // The first (paused, outer) swap is expected to refund, because the transfer
        // proof is delivered to the wrong swap, and we currently don't store it in the
        // database for the other swap.

        let bob_state_1 = bob::run(bob_swap_1).await?;
        assert!(matches!(bob_state_1, BobState::BtcRefunded { .. }));

        let alice_state_1 = alice_swap_1.await??;
        assert!(matches!(alice_state_1, AliceState::XmrRefunded { .. }));

        Ok(())
    })
    .await;
}
