#![warn(
    unused_extern_crates,
    missing_debug_implementations,
    missing_copy_implementations,
    rust_2018_idioms,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::fallible_impl_from,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::dbg_macro
)]
#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![forbid(unsafe_code)]
#![allow(non_snake_case)]

pub mod alice;
pub mod bitcoin;
pub mod bob;
pub mod monero;

#[cfg(test)]
mod tests {
    use crate::{
        alice, bitcoin,
        bitcoin::{Amount, TX_FEE},
        bob, monero,
    };
    use bitcoin_harness::Bitcoind;
    use monero_harness::Monero;
    use rand::rngs::OsRng;
    use testcontainers::clients::Cli;

    const TEN_XMR: u64 = 10_000_000_000_000;

    pub async fn init_bitcoind(tc_client: &Cli) -> Bitcoind<'_> {
        let bitcoind = Bitcoind::new(tc_client, "0.19.1").expect("failed to create bitcoind");
        let _ = bitcoind.init(5).await;

        bitcoind
    }

    #[tokio::test]
    async fn happy_path() {
        let cli = Cli::default();
        let monero = Monero::new(&cli);
        let bitcoind = init_bitcoind(&cli).await;

        // must be bigger than our hardcoded fee of 10_000
        let btc_amount = bitcoin::Amount::from_sat(10_000_000);
        let xmr_amount = monero::Amount::from_piconero(1_000_000_000_000);

        let fund_alice = TEN_XMR;
        let fund_bob = 0;
        monero.init(fund_alice, fund_bob).await.unwrap();

        let alice_monero_wallet = monero::AliceWallet(&monero);
        let bob_monero_wallet = monero::BobWallet(&monero);

        let alice_btc_wallet = bitcoin::Wallet::new("alice", &bitcoind.node_url)
            .await
            .unwrap();
        let bob_btc_wallet = bitcoin::make_wallet("bob", &bitcoind, btc_amount)
            .await
            .unwrap();

        let alice_initial_btc_balance = alice_btc_wallet.balance().await.unwrap();
        let bob_initial_btc_balance = bob_btc_wallet.balance().await.unwrap();

        let alice_initial_xmr_balance = alice_monero_wallet.0.get_balance_alice().await.unwrap();
        let bob_initial_xmr_balance = bob_monero_wallet.0.get_balance_bob().await.unwrap();

        let redeem_address = alice_btc_wallet.new_address().await.unwrap();
        let punish_address = redeem_address.clone();
        let refund_address = bob_btc_wallet.new_address().await.unwrap();

        let refund_timelock = 1;
        let punish_timelock = 1;

        let alice_state0 = alice::State0::new(
            &mut OsRng,
            btc_amount,
            xmr_amount,
            refund_timelock,
            punish_timelock,
            redeem_address,
            punish_address,
        );
        let bob_state0 = bob::State0::new(
            &mut OsRng,
            btc_amount,
            xmr_amount,
            refund_timelock,
            punish_timelock,
            refund_address.clone(),
        );

        let alice_message0 = alice_state0.next_message(&mut OsRng);
        let bob_message0 = bob_state0.next_message(&mut OsRng);

        let alice_state1 = alice_state0.receive(bob_message0).unwrap();
        let bob_state1 = bob_state0
            .receive(&bob_btc_wallet, alice_message0)
            .await
            .unwrap();

        let bob_message1 = bob_state1.next_message();
        let alice_state2 = alice_state1.receive(bob_message1);
        let alice_message1 = alice_state2.next_message();
        let bob_state2 = bob_state1.receive(alice_message1).unwrap();

        let bob_message2 = bob_state2.next_message();
        let alice_state3 = alice_state2.receive(bob_message2).unwrap();

        let bob_state2b = bob_state2.lock_btc(&bob_btc_wallet).await.unwrap();
        let lock_txid = bob_state2b.tx_lock_id();

        let alice_state4 = alice_state3
            .watch_for_lock_btc(&alice_btc_wallet)
            .await
            .unwrap();

        let (alice_state4b, lock_tx_monero_fee) =
            alice_state4.lock_xmr(&alice_monero_wallet).await.unwrap();

        let alice_message2 = alice_state4b.next_message();

        let bob_state3 = bob_state2b
            .watch_for_lock_xmr(&bob_monero_wallet, alice_message2)
            .await
            .unwrap();

        let bob_message3 = bob_state3.next_message();
        let alice_state5 = alice_state4b.receive(bob_message3);

        alice_state5.redeem_btc(&alice_btc_wallet).await.unwrap();
        let bob_state4 = bob_state3
            .watch_for_redeem_btc(&bob_btc_wallet)
            .await
            .unwrap();

        bob_state4.claim_xmr(&bob_monero_wallet).await.unwrap();

        let alice_final_btc_balance = alice_btc_wallet.balance().await.unwrap();
        let bob_final_btc_balance = bob_btc_wallet.balance().await.unwrap();

        let lock_tx_bitcoin_fee = bob_btc_wallet.transaction_fee(lock_txid).await.unwrap();

        assert_eq!(
            alice_final_btc_balance,
            alice_initial_btc_balance + btc_amount - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
        );
        assert_eq!(
            bob_final_btc_balance,
            bob_initial_btc_balance - btc_amount - lock_tx_bitcoin_fee
        );

        let alice_final_xmr_balance = alice_monero_wallet.0.get_balance_alice().await.unwrap();
        bob_monero_wallet
            .0
            .wait_for_bob_wallet_block_height()
            .await
            .unwrap();
        let bob_final_xmr_balance = bob_monero_wallet.0.get_balance_bob().await.unwrap();

        assert_eq!(
            alice_final_xmr_balance,
            alice_initial_xmr_balance - u64::from(xmr_amount) - u64::from(lock_tx_monero_fee)
        );
        assert_eq!(
            bob_final_xmr_balance,
            bob_initial_xmr_balance + u64::from(xmr_amount)
        );
    }

    #[tokio::test]
    async fn both_refund() {
        let cli = Cli::default();
        let monero = Monero::new(&cli);
        let bitcoind = init_bitcoind(&cli).await;

        // must be bigger than our hardcoded fee of 10_000
        let btc_amount = bitcoin::Amount::from_sat(10_000_000);
        let xmr_amount = monero::Amount::from_piconero(1_000_000_000_000);

        let alice_btc_wallet = bitcoin::Wallet::new("alice", &bitcoind.node_url)
            .await
            .unwrap();
        let bob_btc_wallet = bitcoin::make_wallet("bob", &bitcoind, btc_amount)
            .await
            .unwrap();

        let fund_alice = TEN_XMR;
        let fund_bob = 0;

        monero.init(fund_alice, fund_bob).await.unwrap();
        let alice_monero_wallet = monero::AliceWallet(&monero);
        let bob_monero_wallet = monero::BobWallet(&monero);

        let alice_initial_btc_balance = alice_btc_wallet.balance().await.unwrap();
        let bob_initial_btc_balance = bob_btc_wallet.balance().await.unwrap();

        let bob_initial_xmr_balance = bob_monero_wallet.0.get_balance_bob().await.unwrap();

        let redeem_address = alice_btc_wallet.new_address().await.unwrap();
        let punish_address = redeem_address.clone();
        let refund_address = bob_btc_wallet.new_address().await.unwrap();

        let refund_timelock = 1;
        let punish_timelock = 1;

        let alice_state0 = alice::State0::new(
            &mut OsRng,
            btc_amount,
            xmr_amount,
            refund_timelock,
            punish_timelock,
            redeem_address,
            punish_address,
        );
        let bob_state0 = bob::State0::new(
            &mut OsRng,
            btc_amount,
            xmr_amount,
            refund_timelock,
            punish_timelock,
            refund_address.clone(),
        );

        let alice_message0 = alice_state0.next_message(&mut OsRng);
        let bob_message0 = bob_state0.next_message(&mut OsRng);

        let alice_state1 = alice_state0.receive(bob_message0).unwrap();
        let bob_state1 = bob_state0
            .receive(&bob_btc_wallet, alice_message0)
            .await
            .unwrap();

        let bob_message1 = bob_state1.next_message();
        let alice_state2 = alice_state1.receive(bob_message1);
        let alice_message1 = alice_state2.next_message();
        let bob_state2 = bob_state1.receive(alice_message1).unwrap();

        let bob_message2 = bob_state2.next_message();
        let alice_state3 = alice_state2.receive(bob_message2).unwrap();

        let bob_state2b = bob_state2.lock_btc(&bob_btc_wallet).await.unwrap();

        let alice_state4 = alice_state3
            .watch_for_lock_btc(&alice_btc_wallet)
            .await
            .unwrap();

        let (alice_state4b, _lock_tx_monero_fee) =
            alice_state4.lock_xmr(&alice_monero_wallet).await.unwrap();

        bob_state2b.refund_btc(&bob_btc_wallet).await.unwrap();

        alice_state4b
            .refund_xmr(&alice_btc_wallet, &alice_monero_wallet)
            .await
            .unwrap();

        let alice_final_btc_balance = alice_btc_wallet.balance().await.unwrap();
        let bob_final_btc_balance = bob_btc_wallet.balance().await.unwrap();

        // lock_tx_bitcoin_fee is determined by the wallet, it is not necessarily equal
        // to TX_FEE
        let lock_tx_bitcoin_fee = bob_btc_wallet
            .transaction_fee(bob_state2b.tx_lock_id())
            .await
            .unwrap();

        assert_eq!(alice_final_btc_balance, alice_initial_btc_balance);
        assert_eq!(
            bob_final_btc_balance,
            // The 2 * TX_FEE corresponds to tx_refund and tx_cancel.
            bob_initial_btc_balance - Amount::from_sat(2 * TX_FEE) - lock_tx_bitcoin_fee
        );

        alice_monero_wallet
            .0
            .wait_for_alice_wallet_block_height()
            .await
            .unwrap();
        let alice_final_xmr_balance = alice_monero_wallet.0.get_balance_alice().await.unwrap();
        let bob_final_xmr_balance = bob_monero_wallet.0.get_balance_bob().await.unwrap();

        // Because we create a new wallet when claiming Monero, we can only assert on
        // this new wallet owning all of `xmr_amount` after refund
        assert_eq!(alice_final_xmr_balance, u64::from(xmr_amount));
        assert_eq!(bob_final_xmr_balance, bob_initial_xmr_balance);
    }

    #[tokio::test]
    async fn alice_punishes() {
        let cli = Cli::default();
        let bitcoind = init_bitcoind(&cli).await;

        // must be bigger than our hardcoded fee of 10_000
        let btc_amount = bitcoin::Amount::from_sat(10_000_000);
        let xmr_amount = monero::Amount::from_piconero(1_000_000_000_000);

        let alice_btc_wallet = bitcoin::Wallet::new("alice", &bitcoind.node_url)
            .await
            .unwrap();
        let bob_btc_wallet = bitcoin::make_wallet("bob", &bitcoind, btc_amount)
            .await
            .unwrap();

        let alice_initial_btc_balance = alice_btc_wallet.balance().await.unwrap();
        let bob_initial_btc_balance = bob_btc_wallet.balance().await.unwrap();

        let redeem_address = alice_btc_wallet.new_address().await.unwrap();
        let punish_address = redeem_address.clone();
        let refund_address = bob_btc_wallet.new_address().await.unwrap();

        let refund_timelock = 1;
        let punish_timelock = 1;

        let alice_state0 = alice::State0::new(
            &mut OsRng,
            btc_amount,
            xmr_amount,
            refund_timelock,
            punish_timelock,
            redeem_address,
            punish_address,
        );
        let bob_state0 = bob::State0::new(
            &mut OsRng,
            btc_amount,
            xmr_amount,
            refund_timelock,
            punish_timelock,
            refund_address.clone(),
        );

        let alice_message0 = alice_state0.next_message(&mut OsRng);
        let bob_message0 = bob_state0.next_message(&mut OsRng);

        let alice_state1 = alice_state0.receive(bob_message0).unwrap();
        let bob_state1 = bob_state0
            .receive(&bob_btc_wallet, alice_message0)
            .await
            .unwrap();

        let bob_message1 = bob_state1.next_message();
        let alice_state2 = alice_state1.receive(bob_message1);
        let alice_message1 = alice_state2.next_message();
        let bob_state2 = bob_state1.receive(alice_message1).unwrap();

        let bob_message2 = bob_state2.next_message();
        let alice_state3 = alice_state2.receive(bob_message2).unwrap();

        let bob_state2b = bob_state2.lock_btc(&bob_btc_wallet).await.unwrap();

        let alice_state4 = alice_state3
            .watch_for_lock_btc(&alice_btc_wallet)
            .await
            .unwrap();

        alice_state4.punish(&alice_btc_wallet).await.unwrap();

        let alice_final_btc_balance = alice_btc_wallet.balance().await.unwrap();
        let bob_final_btc_balance = bob_btc_wallet.balance().await.unwrap();

        // lock_tx_bitcoin_fee is determined by the wallet, it is not necessarily equal
        // to TX_FEE
        let lock_tx_bitcoin_fee = bob_btc_wallet
            .transaction_fee(bob_state2b.tx_lock_id())
            .await
            .unwrap();

        assert_eq!(
            alice_final_btc_balance,
            alice_initial_btc_balance + btc_amount - Amount::from_sat(2 * TX_FEE)
        );
        assert_eq!(
            bob_final_btc_balance,
            bob_initial_btc_balance - btc_amount - lock_tx_bitcoin_fee
        );
    }
}
