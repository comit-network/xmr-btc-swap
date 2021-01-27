pub mod testutils;

use swap::{
    config::GetConfig,
    protocol::{alice, bob, bob::BobState},
};
use testutils::{bob_run_until::is_encsig_sent, SlowCancelConfig};

#[tokio::test]
async fn given_bob_restarts_after_encsig_is_sent_resume_swap() {
    testutils::setup_test(SlowCancelConfig::get_config(), |mut ctx| async move {
        let (alice_swap, _) = ctx.new_swap_as_alice().await;
        let (bob_swap, bob_join_handle) = ctx.new_swap_as_bob().await;

        let alice = alice::run(alice_swap);
        let alice_handle = tokio::spawn(alice);

        let bob_state = bob::run_until(bob_swap, is_encsig_sent).await.unwrap();

        assert!(matches!(bob_state, BobState::EncSigSent {..}));

        let bob_swap = ctx.stop_and_resume_bob_from_db(bob_join_handle).await;
        assert!(matches!(bob_swap.state, BobState::EncSigSent {..}));

        let bob_state = bob::run(bob_swap).await.unwrap();

        ctx.assert_bob_redeemed(bob_state).await;

        let alice_state = alice_handle.await.unwrap();
        ctx.assert_alice_redeemed(alice_state.unwrap()).await;
    })
    .await;
}
