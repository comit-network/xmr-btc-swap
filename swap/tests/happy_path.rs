use rand::rngs::OsRng;
use swap::{
    bitcoin,
    protocol::{alice, alice::AliceState, bob, bob::BobState},
};
use tokio::join;

pub mod testutils;

/// Run the following tests with RUST_MIN_STACK=10000000

#[tokio::test]
async fn happy_path() {
    testutils::test(|alice, bob, swap_amounts| async move {
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

        let btc_alice_final = alice.bitcoin_wallet.as_ref().balance().await.unwrap();
        let btc_bob_final = bob.bitcoin_wallet.as_ref().balance().await.unwrap();

        let xmr_alice_final = alice.monero_wallet.as_ref().get_balance().await.unwrap();

        bob.monero_wallet.as_ref().inner.refresh().await.unwrap();
        let xmr_bob_final = bob.monero_wallet.as_ref().get_balance().await.unwrap();

        assert!(matches!(alice_state.unwrap(), AliceState::BtcRedeemed));
        assert!(matches!(bob_state.unwrap(), BobState::XmrRedeemed));

        assert_eq!(
            btc_alice_final,
            alice.btc_starting_balance + swap_amounts.btc
                - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
        );
        assert!(btc_bob_final <= bob.btc_starting_balance - swap_amounts.btc);

        assert!(xmr_alice_final <= alice.xmr_starting_balance - swap_amounts.xmr);
        assert_eq!(xmr_bob_final, bob.xmr_starting_balance + swap_amounts.xmr);
    })
    .await;
}
