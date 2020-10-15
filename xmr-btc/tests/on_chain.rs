pub mod harness;

use anyhow::Result;
use genawaiter::GeneratorState;
use harness::wallet::{bitcoin, monero};
use xmr_btc::{
    action_generator_bob,
    bitcoin::{BroadcastSignedTransaction, SignTxLock},
    bob,
    monero::CreateWalletForOutput,
    Action, ReceiveTransferProof,
};

struct Network;

impl ReceiveTransferProof for Network {
    fn receive_transfer_proof(&self) -> xmr_btc::monero::TransferProof {
        todo!("use libp2p")
    }
}

async fn swap_as_bob(
    network: &'static Network,
    monero_wallet: &'static monero::BobWallet<'static>,
    bitcoin_wallet: &'static bitcoin::Wallet,
    state: bob::State2,
) -> Result<()> {
    let mut action_generator = action_generator_bob(network, monero_wallet, bitcoin_wallet, state);

    loop {
        match action_generator.async_resume().await {
            GeneratorState::Yielded(Action::LockBitcoin(tx_lock)) => {
                let signed_tx_lock = bitcoin_wallet.sign_tx_lock(tx_lock).await?;
                let _ = bitcoin_wallet
                    .broadcast_signed_transaction(signed_tx_lock)
                    .await?;
            }
            GeneratorState::Yielded(Action::SendBitcoinRedeemEncsig(_tx_redeem_encsig)) => {
                todo!("use libp2p")
            }
            GeneratorState::Yielded(Action::CreateMoneroWalletForOutput {
                spend_key,
                view_key,
            }) => {
                monero_wallet
                    .create_and_load_wallet_for_output(spend_key, view_key)
                    .await?;
            }
            GeneratorState::Yielded(Action::RefundBitcoin {
                tx_cancel,
                tx_refund,
            }) => {
                let _ = bitcoin_wallet
                    .broadcast_signed_transaction(tx_cancel)
                    .await?;

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
