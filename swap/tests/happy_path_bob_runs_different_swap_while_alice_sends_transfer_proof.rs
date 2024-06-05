use futures::{select, FutureExt};
use harness::{
    alice_run_until::{is_btc_locked, is_transfer_proof_sent},
    bob_run_until, SlowCancelConfig,
};
use swap::{
    asb::FixedRate,
    protocol::{alice, bob},
};
use tokio::{join, task};

pub mod harness;

#[tokio::test]
async fn given_bob_is_running_a_different_swap_while_alice_sends_transfer_proof_swap_completes() {
    harness::setup_test(SlowCancelConfig, |mut ctx| async move {
        // Start a swap with bob, wait until btc is locked
        println!("Starting swap with bob, waiting until btc is locked");
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;

        let bob_swap = tokio::spawn(bob::run_until(bob_swap, bob_run_until::is_btc_locked));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run_until(
            alice_swap,
            is_btc_locked,
            FixedRate::default(),
        ));

        let (bob_state, alice_state) = join!(bob_swap, alice_swap);

        let _ = bob_state??;
        let _ = alice_state??;

        // Start bob but with a different swap. Alice will send the transfer proof while bob is running the new swap
        println!("Starting a different swap with bob while alice sends transfer proof");
        let (bob_swap2, bob_join_handle2) = ctx.bob_swap().await;
        let bob_state2 = bob::run(bob_swap2);

        ctx.restart_alice_resume_only(true).await;
        let alice_swap = ctx.alice_next_swap().await;
        let alice_state =
            alice::run_until(alice_swap, is_transfer_proof_sent, FixedRate::default()).fuse();
        let mut alice_state = Box::pin(alice_state);

        let mut bob_handle = task::spawn(bob_state2).fuse();

        // TODO: Fix this
        let result = select! {
            alice_state = alice_state => {
                drop(bob_handle); // Explicitly drop the handle to cancel bob_state2
                ()
            },
            // This should ideally never be reached.
            _ = &mut bob_handle => {
                ()
            },
        };
        
        // Resume the original swap for bob and alice
        println!("Resuming the original swap for bob and alice");
        let (bob_swap, _) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle2, bob_swap_id)
            .await;
        let bob_state = bob::run(bob_swap);

        ctx.restart_alice_resume_only(true).await;
        let alice_swap = ctx.alice_next_swap().await;
        let alice_state = alice::run(alice_swap, FixedRate::default());

        let (bob_state, alice_state) = join!(bob_state, alice_state);

        ctx.assert_bob_redeemed(bob_state?).await;
        ctx.assert_alice_redeemed(alice_state?).await;

        Ok(())
    })
    .await;
}
