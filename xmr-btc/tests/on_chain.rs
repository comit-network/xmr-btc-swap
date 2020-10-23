pub mod harness;

use std::{convert::TryInto, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use futures::{
    channel::mpsc::{channel, Receiver, Sender},
    future::try_join,
    SinkExt, StreamExt,
};
use genawaiter::GeneratorState;
use harness::{
    init_bitcoind, init_test,
    node::{run_alice_until, run_bob_until},
};
use monero_harness::Monero;
use rand::rngs::OsRng;
use testcontainers::clients::Cli;
use tracing::info;
use xmr_btc::{
    action_generator_alice, action_generator_bob, alice,
    bitcoin::{BroadcastSignedTransaction, EncryptedSignature, SignTxLock},
    bob,
    monero::{CreateWalletForOutput, Transfer, TransferProof},
    AliceAction, BobAction, ReceiveBitcoinRedeemEncsig, ReceiveTransferProof,
};

type AliceNetwork = Network<EncryptedSignature>;
type BobNetwork = Network<TransferProof>;

#[derive(Debug)]
struct Network<M> {
    // TODO: It is weird to use mpsc's in a situation where only one message is expected, but the
    // ownership rules of Rust are making this painful
    pub receiver: Receiver<M>,
}

impl<M> Network<M> {
    pub fn new() -> (Network<M>, Sender<M>) {
        let (sender, receiver) = channel(1);

        (Self { receiver }, sender)
    }
}

#[async_trait]
impl ReceiveTransferProof for BobNetwork {
    async fn receive_transfer_proof(&mut self) -> TransferProof {
        self.receiver.next().await.unwrap()
    }
}

#[async_trait]
impl ReceiveBitcoinRedeemEncsig for AliceNetwork {
    async fn receive_bitcoin_redeem_encsig(&mut self) -> EncryptedSignature {
        self.receiver.next().await.unwrap()
    }
}

async fn swap_as_alice(
    network: AliceNetwork,
    // FIXME: It would be more intuitive to have a single network/transport struct instead of
    // splitting into two, but Rust ownership rules make this tedious
    mut sender: Sender<TransferProof>,
    monero_wallet: &harness::wallet::monero::Wallet,
    bitcoin_wallet: Arc<harness::wallet::bitcoin::Wallet>,
    state: alice::State3,
) -> Result<()> {
    let mut action_generator = action_generator_alice(network, bitcoin_wallet.clone(), state);

    loop {
        let state = action_generator.async_resume().await;

        info!("resumed execution of generator, got: {:?}", state);

        match state {
            GeneratorState::Yielded(AliceAction::LockXmr {
                amount,
                public_spend_key,
                public_view_key,
            }) => {
                let (transfer_proof, _) = monero_wallet
                    .transfer(public_spend_key, public_view_key, amount)
                    .await?;

                sender.send(transfer_proof).await.unwrap();
            }
            GeneratorState::Yielded(AliceAction::RedeemBtc(tx))
            | GeneratorState::Yielded(AliceAction::CancelBtc(tx))
            | GeneratorState::Yielded(AliceAction::PunishBtc(tx)) => {
                let _ = bitcoin_wallet.broadcast_signed_transaction(tx).await?;
            }
            GeneratorState::Yielded(AliceAction::CreateMoneroWalletForOutput {
                spend_key,
                view_key,
            }) => {
                monero_wallet
                    .create_and_load_wallet_for_output(spend_key, view_key)
                    .await?;
            }
            GeneratorState::Complete(()) => return Ok(()),
        }
    }
}

async fn swap_as_bob(
    network: BobNetwork,
    mut sender: Sender<EncryptedSignature>,
    monero_wallet: Arc<harness::wallet::monero::Wallet>,
    bitcoin_wallet: Arc<harness::wallet::bitcoin::Wallet>,
    state: bob::State2,
) -> Result<()> {
    let mut action_generator = action_generator_bob(
        network,
        monero_wallet.clone(),
        bitcoin_wallet.clone(),
        state,
    );

    loop {
        let state = action_generator.async_resume().await;

        info!("resumed execution of generator, got: {:?}", state);

        match state {
            GeneratorState::Yielded(BobAction::LockBitcoin(tx_lock)) => {
                let signed_tx_lock = bitcoin_wallet.sign_tx_lock(tx_lock).await?;
                let _ = bitcoin_wallet
                    .broadcast_signed_transaction(signed_tx_lock)
                    .await?;
            }
            GeneratorState::Yielded(BobAction::SendBitcoinRedeemEncsig(tx_redeem_encsig)) => {
                sender.send(tx_redeem_encsig).await.unwrap();
            }
            GeneratorState::Yielded(BobAction::CreateMoneroWalletForOutput {
                spend_key,
                view_key,
            }) => {
                monero_wallet
                    .create_and_load_wallet_for_output(spend_key, view_key)
                    .await?;
            }
            GeneratorState::Yielded(BobAction::CancelBitcoin(tx_cancel)) => {
                let _ = bitcoin_wallet
                    .broadcast_signed_transaction(tx_cancel)
                    .await?;
            }
            GeneratorState::Yielded(BobAction::RefundBitcoin(tx_refund)) => {
                let _ = bitcoin_wallet
                    .broadcast_signed_transaction(tx_refund)
                    .await?;
            }
            GeneratorState::Complete(()) => return Ok(()),
        }
    }
}

// NOTE: For some reason running these tests overflows the stack. In order to
// mitigate this run them with:
//
//     RUST_MIN_STACK=100000000 cargo test

#[tokio::test]
async fn on_chain_happy_path() {
    let cli = Cli::default();
    let (monero, _container) = Monero::new(&cli).unwrap();
    let bitcoind = init_bitcoind(&cli).await;

    let (alice_state0, bob_state0, mut alice_node, mut bob_node, initial_balances, swap_amounts) =
        init_test(&monero, &bitcoind, Some(100), Some(100)).await;

    // run the handshake as part of the setup
    let (alice_state, bob_state) = try_join(
        run_alice_until(
            &mut alice_node,
            alice_state0.into(),
            harness::alice::is_state3,
            &mut OsRng,
        ),
        run_bob_until(
            &mut bob_node,
            bob_state0.into(),
            harness::bob::is_state2,
            &mut OsRng,
        ),
    )
    .await
    .unwrap();
    let alice: alice::State3 = alice_state.try_into().unwrap();
    let bob: bob::State2 = bob_state.try_into().unwrap();
    let tx_lock_txid = bob.tx_lock.txid();

    let alice_bitcoin_wallet = Arc::new(alice_node.bitcoin_wallet);
    let bob_bitcoin_wallet = Arc::new(bob_node.bitcoin_wallet);
    let alice_monero_wallet = Arc::new(alice_node.monero_wallet);
    let bob_monero_wallet = Arc::new(bob_node.monero_wallet);

    let (alice_network, bob_sender) = Network::<EncryptedSignature>::new();
    let (bob_network, alice_sender) = Network::<TransferProof>::new();

    try_join(
        swap_as_alice(
            alice_network,
            alice_sender,
            &alice_monero_wallet.clone(),
            alice_bitcoin_wallet.clone(),
            alice,
        ),
        swap_as_bob(
            bob_network,
            bob_sender,
            bob_monero_wallet.clone(),
            bob_bitcoin_wallet.clone(),
            bob,
        ),
    )
    .await
    .unwrap();

    let alice_final_btc_balance = alice_bitcoin_wallet.balance().await.unwrap();
    let bob_final_btc_balance = bob_bitcoin_wallet.balance().await.unwrap();

    let lock_tx_bitcoin_fee = bob_bitcoin_wallet
        .transaction_fee(tx_lock_txid)
        .await
        .unwrap();

    let alice_final_xmr_balance = alice_monero_wallet.get_balance().await.unwrap();

    monero.wait_for_bob_wallet_block_height().await.unwrap();
    let bob_final_xmr_balance = bob_monero_wallet.get_balance().await.unwrap();

    assert_eq!(
        alice_final_btc_balance,
        initial_balances.alice_btc + swap_amounts.btc
            - bitcoin::Amount::from_sat(xmr_btc::bitcoin::TX_FEE)
    );
    assert_eq!(
        bob_final_btc_balance,
        initial_balances.bob_btc - swap_amounts.btc - lock_tx_bitcoin_fee
    );

    // Getting the Monero LockTx fee is tricky in a clean way, I think checking this
    // condition is sufficient
    assert!(alice_final_xmr_balance <= initial_balances.alice_xmr - swap_amounts.xmr,);
    assert_eq!(
        bob_final_xmr_balance,
        initial_balances.bob_xmr + swap_amounts.xmr
    );
}
