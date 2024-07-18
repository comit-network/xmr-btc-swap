pub mod harness;

use harness::alice_run_until::is_encsig_learned;
use harness::SlowCancelConfig;
use swap::asb;
use swap::asb::{Finality, FixedRate};
use swap::protocol::alice::AliceState;
use swap::protocol::{alice, bob};

/// Bob locks Btc and Alice locks Xmr. Alice redeems using manual redeem command
/// after learning encsig from Bob
#[tokio::test]
async fn alice_manually_redeems_after_enc_sig_learned() {
    harness::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (bob_swap, _) = ctx.bob_swap().await;
        let bob_swap = tokio::spawn(bob::run(bob_swap));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run_until(
            alice_swap,
            is_encsig_learned,
            FixedRate::default(),
        ));

        let alice_state = alice_swap.await??;
        assert!(matches!(alice_state, AliceState::EncSigLearned { .. }));

        // manual redeem
        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        let (_, alice_state) = asb::redeem(
            alice_swap.swap_id,
            alice_swap.bitcoin_wallet,
            alice_swap.db,
            Finality::Await,
        )
        .await?;
        ctx.assert_alice_redeemed(alice_state).await;

        let bob_state = bob_swap.await??;
        ctx.assert_bob_redeemed(bob_state).await;

        Ok(())
    })
    .await;
}
