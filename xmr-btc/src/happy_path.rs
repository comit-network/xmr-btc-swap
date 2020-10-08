//! This module shows how a BTC/XMR atomic swap proceeds along the happy path.

use crate::{alice, bitcoin, bob, monero};
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

    let a_monero_wallet = monero::AliceWallet(&monero);
    let b_monero_wallet = monero::BobWallet(&monero);

    let a_btc_wallet = bitcoin::Wallet::new("alice", &bitcoind.node_url)
        .await
        .unwrap();
    let b_btc_wallet = bitcoin::make_wallet("bob", &bitcoind, btc_amount)
        .await
        .unwrap();

    let a_initial_btc_balance = a_btc_wallet.balance().await.unwrap();
    let b_initial_btc_balance = b_btc_wallet.balance().await.unwrap();

    let a_initial_xmr_balance = a_monero_wallet.0.get_balance_alice().await.unwrap();
    let b_initial_xmr_balance = b_monero_wallet.0.get_balance_bob().await.unwrap();

    let redeem_address = a_btc_wallet.new_address().await.unwrap();
    let punish_address = redeem_address.clone();
    let refund_address = b_btc_wallet.new_address().await.unwrap();

    let refund_timelock = 1;
    let punish_timelock = 1;

    let a_state0 = alice::State0::new(
        &mut OsRng,
        btc_amount,
        xmr_amount,
        refund_timelock,
        punish_timelock,
        redeem_address,
        punish_address,
    );
    let b_state0 = bob::State0::new(
        &mut OsRng,
        btc_amount,
        xmr_amount,
        refund_timelock,
        punish_timelock,
        refund_address.clone(),
    );

    let a_message0 = a_state0.next_message(&mut OsRng);
    let b_message0 = b_state0.next_message(&mut OsRng);

    let a_state1 = a_state0.receive(b_message0).unwrap();
    let b_state1 = b_state0.receive(&b_btc_wallet, a_message0).await.unwrap();

    let b_message1 = b_state1.next_message();
    let a_state2 = a_state1.receive(b_message1);
    let a_message1 = a_state2.next_message();
    let b_state2 = b_state1.receive(a_message1).unwrap();

    let b_message2 = b_state2.next_message();
    let a_state3 = a_state2.receive(b_message2).unwrap();

    let b_state2b = b_state2.lock_btc(&b_btc_wallet).await.unwrap();
    let lock_txid = b_state2b.tx_lock_id();

    let a_state4 = a_state3.watch_for_lock_btc(&a_btc_wallet).await.unwrap();

    let (a_state4b, lock_tx_monero_fee) = a_state4.lock_xmr(&a_monero_wallet).await.unwrap();

    let a_message2 = a_state4b.next_message();

    let b_state3 = b_state2b
        .watch_for_lock_xmr(&b_monero_wallet, a_message2)
        .await
        .unwrap();

    let b_message3 = b_state3.next_message();
    let a_state5 = a_state4b.receive(b_message3);

    a_state5.redeem_btc(&a_btc_wallet).await.unwrap();
    let b_state4 = b_state3.watch_for_redeem_btc(&b_btc_wallet).await.unwrap();

    b_state4.claim_xmr(&b_monero_wallet).await.unwrap();

    let a_final_btc_balance = a_btc_wallet.balance().await.unwrap();
    let b_final_btc_balance = b_btc_wallet.balance().await.unwrap();

    let lock_tx_bitcoin_fee = b_btc_wallet.transaction_fee(lock_txid).await.unwrap();

    assert_eq!(
        a_final_btc_balance,
        a_initial_btc_balance + btc_amount - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
    );
    assert_eq!(
        b_final_btc_balance,
        b_initial_btc_balance - btc_amount - lock_tx_bitcoin_fee
    );

    let a_final_xmr_balance = a_monero_wallet.0.get_balance_alice().await.unwrap();
    b_monero_wallet
        .0
        .wait_for_bob_wallet_block_height()
        .await
        .unwrap();
    let b_final_xmr_balance = b_monero_wallet.0.get_balance_bob().await.unwrap();

    assert_eq!(
        a_final_xmr_balance,
        a_initial_xmr_balance - u64::from(xmr_amount) - u64::from(lock_tx_monero_fee)
    );
    assert_eq!(
        b_final_xmr_balance,
        b_initial_xmr_balance + u64::from(xmr_amount)
    );
}
