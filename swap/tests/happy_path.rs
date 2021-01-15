use rand::rngs::OsRng;
use swap::protocol::{alice, bob, bob::BobState};
use tokio::join;

pub mod testutils;

/// Run the following tests with RUST_MIN_STACK=10000000

#[tokio::test]
async fn happy_path() {
    testutils::test(|alice_harness, bob, swap_amounts| async move {
        let alice = alice_harness.new_alice().await;
        let alice_swap_fut = alice::swap(
            alice.state,
            alice.event_loop_handle,
            alice.bitcoin_wallet.clone(),
            alice.monero_wallet.clone(),
            alice.config,
            alice.swap_id,
            alice.db,
        );
        let bob_swap_fut = bob::swap(
            bob.state,
            bob.event_loop_handle,
            bob.db,
            bob.bitcoin_wallet.clone(),
            bob.monero_wallet.clone(),
            OsRng,
            bob.swap_id,
        );
        let (alice_state, bob_state) = join!(alice_swap_fut, bob_swap_fut);

        alice_harness.assert_redeemed(alice_state.unwrap()).await;

        let btc_bob_final = bob.bitcoin_wallet.as_ref().balance().await.unwrap();

        bob.monero_wallet.as_ref().inner.refresh().await.unwrap();

        let xmr_bob_final = bob.monero_wallet.as_ref().get_balance().await.unwrap();
        assert!(matches!(bob_state.unwrap(), BobState::XmrRedeemed));
        assert!(btc_bob_final <= bob.btc_starting_balance - swap_amounts.btc);
        assert_eq!(xmr_bob_final, bob.xmr_starting_balance + swap_amounts.xmr);
    })
    .await;
}
