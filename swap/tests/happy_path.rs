use rand::rngs::OsRng;
use swap::protocol::{alice, bob};
use tokio::join;

pub mod testutils;

/// Run the following tests with RUST_MIN_STACK=10000000

#[tokio::test]
async fn happy_path() {
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

        let bob_swap = bob::swap(
            bob.state,
            bob.event_loop_handle,
            bob.db,
            bob.bitcoin_wallet.clone(),
            bob.monero_wallet.clone(),
            OsRng,
            bob.swap_id,
        );
        let (alice_state, bob_state) = join!(alice_swap, bob_swap);

        alice_harness.assert_redeemed(alice_state.unwrap()).await;
        bob_harness.assert_redeemed(bob_state.unwrap()).await;
    })
    .await;
}
