pub mod harness;

use harness::alice_run_until::is_xmr_lock_transaction_sent;
use harness::bob_run_until::is_btc_locked;
use harness::FastPunishConfig;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

/// Bob locks Btc and Alice locks Xmr. Bob does not act; he fails to send Alice
/// the encsig and fail to refund or redeem. Alice cancels and punishes.
#[tokio::test]
async fn alice_punishes_after_restart_if_bob_dead() {
    harness::setup_test(FastPunishConfig, |mut ctx| async move {
        let (bob_swap, bob_join_handle) = ctx.bob_swap().await;
        let bob_swap_id = bob_swap.id;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_btc_locked));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_bitcoin_wallet = alice_swap.bitcoin_wallet.clone();

        let alice_swap = tokio::spawn(alice::run_until(
            alice_swap,
            is_xmr_lock_transaction_sent,
            FixedRate::default(),
        ));

        let bob_state = bob_swap.await??;
        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        let alice_state = alice_swap.await??;

        // Ensure cancel timelock is expired (we can only ensure that, because the
        // cancel transaction is not published at this point)
        if let AliceState::XmrLockTransactionSent { state3, .. } = alice_state {
            alice_bitcoin_wallet
                .subscribe_to(state3.tx_lock)
                .await
                .wait_until_confirmed_with(state3.cancel_timelock)
                .await?;
        } else {
            panic!("Alice in unexpected state {}", alice_state);
        }

        ctx.restart_alice().await;
        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let alice_state = alice_swap.await??;
        ctx.assert_alice_punished(alice_state).await;

        // Restart Bob after Alice punished to ensure Bob transitions to
        // punished and does not run indefinitely
        let (bob_swap, _) = ctx
            .stop_and_resume_bob_from_db(bob_join_handle, bob_swap_id)
            .await;
        assert!(matches!(bob_swap.state, BobState::BtcLocked { .. }));

        let bob_state = bob::run(bob_swap).await?;

        ctx.assert_bob_punished(bob_state).await;

        Ok(())
    })
    .await;
}
