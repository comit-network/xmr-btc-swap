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
