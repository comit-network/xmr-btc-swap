//! This module shows how a BTC/XMR atomic swap proceeds along the happy path.

use crate::{alice, bitcoin, bob, monero, Message, ReceiveMessage, SendMessage};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use bitcoin_harness::Bitcoind;
use futures::{
    channel::{
        mpsc,
        mpsc::{Receiver, Sender},
    },
    SinkExt, StreamExt,
};
use monero_harness::Monero;
use rand::rngs::OsRng;
use std::convert::TryInto;
use testcontainers::clients::Cli;

const TEN_XMR: u64 = 10_000_000_000_000;

pub async fn init_bitcoind(tc_client: &Cli) -> Bitcoind<'_> {
    let bitcoind = Bitcoind::new(tc_client, "0.19.1").expect("failed to create bitcoind");
    let _ = bitcoind.init(5).await;

    bitcoind
}

/// Create two mock `Transport`s which mimic a peer to peer connection between
/// two parties, allowing them to send and receive `thor::Message`s.
pub fn make_transports() -> (Transport, Transport) {
    let (a_sender, b_receiver) = mpsc::channel(5);
    let (b_sender, a_receiver) = mpsc::channel(5);

    let a_transport = Transport {
        sender: a_sender,
        receiver: a_receiver,
    };

    let b_transport = Transport {
        sender: b_sender,
        receiver: b_receiver,
    };

    (a_transport, b_transport)
}

#[derive(Debug)]
pub struct Transport {
    sender: Sender<Message>,
    receiver: Receiver<Message>,
}

#[async_trait]
impl SendMessage for Transport {
    async fn send_message(&mut self, msg: Message) -> Result<()> {
        self.sender
            .send(msg)
            .await
            .map_err(|_| anyhow!("failed to send message"))
    }
}

#[async_trait]
impl ReceiveMessage for Transport {
    async fn receive_message(&mut self) -> Result<Message> {
        let msg = self
            .receiver
            .next()
            .await
            .ok_or_else(|| anyhow!("failed to receive message"))?;

        Ok(msg)
    }
}

#[tokio::test]
async fn happy_path() {
    let cli = Cli::default();
    let monero = Monero::new(&cli);
    let bitcoind = init_bitcoind(&cli).await;

    // Mocks send/receive message for Alice and Bob.
    let (mut a_trans, mut b_trans) = make_transports();

    // Must be bigger than our hardcoded fee of 10_000
    let btc_amount = bitcoin::Amount::from_sat(10_000_000);
    let xmr_amount = monero::Amount::from_piconero(1_000_000_000_000);

    let fund_alice = TEN_XMR;
    let fund_bob = 0;
    monero.init(fund_alice, fund_bob).await.unwrap();

    let a_xmr_wallet = monero::AliceWallet(&monero);
    let b_xmr_wallet = monero::BobWallet(&monero);

    let a_btc_wallet = bitcoin::Wallet::new("alice", &bitcoind.node_url)
        .await
        .unwrap();
    let b_btc_wallet = bitcoin::make_wallet("bob", &bitcoind, btc_amount)
        .await
        .unwrap();

    let a_initial_btc_balance = a_btc_wallet.balance().await.unwrap();
    let b_initial_btc_balance = b_btc_wallet.balance().await.unwrap();

    let a_initial_xmr_balance = a_xmr_wallet.0.get_balance_alice().await.unwrap();
    let b_initial_xmr_balance = b_xmr_wallet.0.get_balance_bob().await.unwrap();

    let redeem_address = a_btc_wallet.new_address().await.unwrap();
    let punish_address = redeem_address.clone();
    let refund_address = b_btc_wallet.new_address().await.unwrap();

    let refund_timelock = 1;
    let punish_timelock = 1;

    let a_state = alice::State0::new(
        &mut OsRng,
        btc_amount,
        xmr_amount,
        refund_timelock,
        punish_timelock,
        redeem_address,
        punish_address,
    );
    let b_state = bob::State0::new(
        &mut OsRng,
        btc_amount,
        xmr_amount,
        refund_timelock,
        punish_timelock,
        refund_address.clone(),
    );

    let a_msg = Message::Alice0(a_state.next_message(&mut OsRng));
    let b_msg = Message::Bob0(b_state.next_message(&mut OsRng));
    // Calls to send/receive must be ordered otherwise we will block
    // waiting for the message.
    a_trans.send_message(a_msg).await.unwrap();
    let b_recv_msg = b_trans.receive_message().await.unwrap().try_into().unwrap();
    b_trans.send_message(b_msg).await.unwrap();
    let a_recv_msg = a_trans.receive_message().await.unwrap().try_into().unwrap();

    let a_state = a_state.receive(a_recv_msg).unwrap();
    let b_state = b_state.receive(&b_btc_wallet, b_recv_msg).await.unwrap();

    let msg = Message::Bob1(b_state.next_message());
    b_trans.send_message(msg).await.unwrap();
    let a_recv_msg = a_trans.receive_message().await.unwrap().try_into().unwrap();
    let a_state = a_state.receive(a_recv_msg);

    let msg = Message::Alice1(a_state.next_message());
    a_trans.send_message(msg).await.unwrap();
    let b_recv_msg = b_trans.receive_message().await.unwrap().try_into().unwrap();
    let b_state = b_state.receive(b_recv_msg).unwrap();

    let msg = Message::Bob2(b_state.next_message());
    b_trans.send_message(msg).await.unwrap();
    let a_recv_msg = a_trans.receive_message().await.unwrap().try_into().unwrap();
    let a_state = a_state.receive(a_recv_msg).unwrap();

    let b_state = b_state.lock_btc(&b_btc_wallet).await.unwrap();
    let lock_txid = b_state.tx_lock_id();

    let a_state = a_state.watch_for_lock_btc(&a_btc_wallet).await.unwrap();

    let (a_state, lock_tx_monero_fee) = a_state.lock_xmr(&a_xmr_wallet).await.unwrap();

    let msg = Message::Alice2(a_state.next_message());
    a_trans.send_message(msg).await.unwrap();
    let b_recv_msg = b_trans.receive_message().await.unwrap().try_into().unwrap();
    let b_state = b_state
        .watch_for_lock_xmr(&b_xmr_wallet, b_recv_msg)
        .await
        .unwrap();

    let msg = Message::Bob3(b_state.next_message());
    b_trans.send_message(msg).await.unwrap();
    let a_recv_msg = a_trans.receive_message().await.unwrap().try_into().unwrap();
    let a_state = a_state.receive(a_recv_msg);

    a_state.redeem_btc(&a_btc_wallet).await.unwrap();
    let b_state = b_state.watch_for_redeem_btc(&b_btc_wallet).await.unwrap();

    b_state.claim_xmr(&b_xmr_wallet).await.unwrap();

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

    let a_final_xmr_balance = a_xmr_wallet.0.get_balance_alice().await.unwrap();
    b_xmr_wallet
        .0
        .wait_for_bob_wallet_block_height()
        .await
        .unwrap();
    let b_final_xmr_balance = b_xmr_wallet.0.get_balance_bob().await.unwrap();

    assert_eq!(
        a_final_xmr_balance,
        a_initial_xmr_balance - u64::from(xmr_amount) - u64::from(lock_tx_monero_fee)
    );
    assert_eq!(
        b_final_xmr_balance,
        b_initial_xmr_balance + u64::from(xmr_amount)
    );
}
