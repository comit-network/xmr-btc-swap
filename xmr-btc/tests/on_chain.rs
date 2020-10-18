pub mod harness;

use anyhow::Result;
use async_trait::async_trait;
use futures::{
    channel::mpsc::{Receiver, Sender},
    SinkExt, StreamExt,
};
use genawaiter::GeneratorState;
use harness::wallet::{bitcoin, monero};
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
struct Network<RecvMsg> {
    // TODO: It is weird to use mpsc's in a situation where only one message is expected, but the
    // ownership rules of Rust are making this painful
    pub receiver: Receiver<RecvMsg>,
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
    network: &'static mut AliceNetwork,
    // FIXME: It would be more intuitive to have a single network/transport struct instead of
    // splitting into two, but Rust ownership rules make this tedious
    mut sender: Sender<TransferProof>,
    monero_wallet: &'static monero::AliceWallet<'static>,
    bitcoin_wallet: &'static bitcoin::Wallet,
    state: alice::State3,
) -> Result<()> {
    let mut action_generator =
        action_generator_alice(network, monero_wallet, bitcoin_wallet, state);

    loop {
        match action_generator.async_resume().await {
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
    network: &'static mut BobNetwork,
    mut sender: Sender<EncryptedSignature>,
    monero_wallet: &'static monero::BobWallet<'static>,
    bitcoin_wallet: &'static bitcoin::Wallet,
    state: bob::State2,
) -> Result<()> {
    let mut action_generator = action_generator_bob(network, monero_wallet, bitcoin_wallet, state);

    loop {
        match action_generator.async_resume().await {
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

#[test]
fn on_chain_happy_path() {}
