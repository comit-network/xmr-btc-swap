pub mod harness;

use harness::SlowCancelConfig;
use swap::protocol::bob;

#[tokio::test]
async fn ensure_same_swap_id_for_alice_and_bob() {
    harness::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (bob_swap, _) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        tokio::spawn(bob::run(bob_swap));

        // once Bob's swap is spawned we can retrieve Alice's swap and assert on the
        // swap ID
        let alice_swap = ctx.alice_next_swap().await;
        assert_eq!(alice_swap.swap_id, bob_swap_id);

        Ok(())
    })
    .await;
}
