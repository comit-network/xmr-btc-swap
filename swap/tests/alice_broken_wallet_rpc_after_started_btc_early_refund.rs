pub mod harness;

use harness::bob_run_until::is_btc_locked;
use swap::asb::FixedRate;
use swap::protocol::alice::AliceState;
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};

use crate::harness::SlowCancelConfig;

#[tokio::test]
async fn alice_zero_xmr_refunds_bitcoin() {
    harness::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (bob_swap, bob_handle) = ctx.bob_swap().await;
        let bob_swap = tokio::spawn(bob::run_until(bob_swap, is_btc_locked));

        // Run until the Bitcoin lock transaction is seen
        let alice_swap = ctx.alice_next_swap().await;
        let swap_id = alice_swap.swap_id;
        let alice_swap = tokio::spawn(alice::run_until(
            alice_swap,
            |state| matches!(state, AliceState::BtcLockTransactionSeen { .. }),
            FixedRate::default(),
        ));

        // Wait for both Alice and Bob to reach the Bitcoin locked state
        let alice_state = alice_swap.await??;
        let bob_state = bob_swap.await??;

        assert!(matches!(
            alice_state,
            AliceState::BtcLockTransactionSeen { .. }
        ));
        assert!(matches!(bob_state, BobState::BtcLocked { .. }));

        // Kill the monero-wallet-rpc of Alice
        // This will prevent her from locking her Monero
        // in turn forcing her into an early refund
        ctx.stop_alice_monero_wallet_rpc().await;

        ctx.restart_alice().await;
        let (swap, _) = ctx.stop_and_resume_bob_from_db(bob_handle, swap_id).await;

        let bob_swap = tokio::spawn(bob::run(swap));

        let alice_swap = ctx.alice_next_swap().await;
        let alice_swap = tokio::spawn(alice::run(alice_swap, FixedRate::default()));

        let alice_state = alice_swap.await??;
        let bob_state = bob_swap.await??;

        assert!(matches!(alice_state, AliceState::BtcEarlyRefunded(_)));
        assert!(matches!(bob_state, BobState::BtcEarlyRefunded(_)));

        Ok(())
    })
    .await;
}
