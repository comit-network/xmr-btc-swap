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

#[macro_use]
mod utils {

    macro_rules! impl_try_from_parent_enum {
        ($type:ident, $parent:ident) => {
            impl TryFrom<$parent> for $type {
                type Error = anyhow::Error;
                fn try_from(from: $parent) -> Result<Self> {
                    if let $parent::$type(inner) = from {
                        Ok(inner)
                    } else {
                        Err(anyhow::anyhow!(
                            "Failed to convert parent state to child state"
                        ))
                    }
                }
            }
        };
    }

    macro_rules! impl_from_child_enum {
        ($type:ident, $parent:ident) => {
            impl From<$type> for $parent {
                fn from(from: $type) -> Self {
                    $parent::$type(from)
                }
            }
        };
    }
}

pub mod alice;
pub mod bitcoin;
pub mod bob;
pub mod monero;
pub mod serde;
pub mod transport;

use async_trait::async_trait;
use ecdsa_fun::{adaptor::Adaptor, nonce::Deterministic};
use futures::{
    future::{select, Either},
    Future, FutureExt,
};
use genawaiter::sync::{Gen, GenBoxed};
use sha2::Sha256;
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;
use tracing::error;

// TODO: Replace this with something configurable, such as an function argument.
/// Time that Bob has to publish the Bitcoin lock transaction before both
/// parties will abort, in seconds.
const SECS_TO_ACT_BOB: u64 = 60;

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum BobAction {
    LockBitcoin(bitcoin::TxLock),
    SendBitcoinRedeemEncsig(bitcoin::EncryptedSignature),
    CreateMoneroWalletForOutput {
        spend_key: monero::PrivateKey,
        view_key: monero::PrivateViewKey,
    },
    CancelBitcoin(bitcoin::Transaction),
    RefundBitcoin(bitcoin::Transaction),
}

// TODO: This could be moved to the monero module
#[async_trait]
pub trait ReceiveTransferProof {
    async fn receive_transfer_proof(&mut self) -> monero::TransferProof;
}

#[async_trait]
pub trait BlockHeight {
    async fn block_height(&self) -> u32;
}

#[async_trait]
pub trait TransactionBlockHeight {
    async fn transaction_block_height(&self, txid: bitcoin::Txid) -> u32;
}

/// Perform the on-chain protocol to swap monero and bitcoin as Bob.
///
/// This is called post handshake, after all the keys, addresses and most of the
/// signatures have been exchanged.
pub fn action_generator_bob<N, M, B>(
    mut network: N,
    monero_client: Arc<M>,
    bitcoin_client: Arc<B>,
    // TODO: Replace this with a new, slimmer struct?
    bob::State2 {
        A,
        b,
        s_b,
        S_a_monero,
        S_a_bitcoin,
        v,
        xmr,
        refund_timelock,
        redeem_address,
        refund_address,
        tx_lock,
        tx_cancel_sig_a,
        tx_refund_encsig,
        ..
    }: bob::State2,
) -> GenBoxed<BobAction, (), ()>
where
    N: ReceiveTransferProof + Send + Sync + 'static,
    M: monero::WatchForTransfer + Send + Sync + 'static,
    B: BlockHeight
        + TransactionBlockHeight
        + bitcoin::WatchForRawTransaction
        + Send
        + Sync
        + 'static,
{
    #[derive(Debug)]
    enum SwapFailed {
        BeforeBtcLock,
        AfterBtcLock(Reason),
        AfterBtcRedeem(Reason),
    }

    /// Reason why the swap has failed.
    #[derive(Debug)]
    enum Reason {
        /// The refund timelock has been reached.
        BtcExpired,
        /// Alice did not lock up enough monero in the shared output.
        InsufficientXmr(monero::InsufficientFunds),
        /// Could not find Bob's signature on the redeem transaction witness
        /// stack.
        BtcRedeemSignature,
        /// Could not recover secret `s_a` from Bob's redeem transaction
        /// signature.
        SecretRecovery,
    }

    async fn poll_until(condition_future: impl Future<Output = bool> + Clone) {
        loop {
            if condition_future.clone().await {
                return;
            }

            tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
        }
    }

    async fn bitcoin_block_height_is_gte<B>(bitcoin_client: &B, n_blocks: u32) -> bool
    where
        B: BlockHeight,
    {
        bitcoin_client.block_height().await >= n_blocks
    }

    Gen::new_boxed(|co| async move {
        let swap_result: Result<(), SwapFailed> = async {
            co.yield_(BobAction::LockBitcoin(tx_lock.clone())).await;

            timeout(
                Duration::from_secs(SECS_TO_ACT_BOB),
                bitcoin_client.watch_for_raw_transaction(tx_lock.txid()),
            )
            .await
            .map(|tx| tx.txid())
            .map_err(|_| SwapFailed::BeforeBtcLock)?;

            let tx_lock_height = bitcoin_client
                .transaction_block_height(tx_lock.txid())
                .await;
            let btc_has_expired = bitcoin_block_height_is_gte(
                bitcoin_client.as_ref(),
                tx_lock_height + refund_timelock,
            )
            .shared();
            let poll_until_btc_has_expired = poll_until(btc_has_expired).shared();
            futures::pin_mut!(poll_until_btc_has_expired);

            let transfer_proof = match select(
                network.receive_transfer_proof(),
                poll_until_btc_has_expired.clone(),
            )
            .await
            {
                Either::Left((proof, _)) => proof,
                Either::Right(_) => return Err(SwapFailed::AfterBtcLock(Reason::BtcExpired)),
            };

            let S_b_monero = monero::PublicKey::from_private_key(&monero::PrivateKey::from_scalar(
                s_b.into_ed25519(),
            ));
            let S = S_a_monero + S_b_monero;

            match select(
                monero_client.watch_for_transfer(
                    S,
                    v.public(),
                    transfer_proof,
                    xmr,
                    monero::MIN_CONFIRMATIONS,
                ),
                poll_until_btc_has_expired.clone(),
            )
            .await
            {
                Either::Left((Err(e), _)) => {
                    return Err(SwapFailed::AfterBtcLock(Reason::InsufficientXmr(e)))
                }
                Either::Right(_) => return Err(SwapFailed::AfterBtcLock(Reason::BtcExpired)),
                _ => {}
            }

            let tx_redeem = bitcoin::TxRedeem::new(&tx_lock, &redeem_address);
            let tx_redeem_encsig = b.encsign(S_a_bitcoin.clone(), tx_redeem.digest());

            co.yield_(BobAction::SendBitcoinRedeemEncsig(tx_redeem_encsig.clone()))
                .await;

            let tx_redeem_published = match select(
                bitcoin_client.watch_for_raw_transaction(tx_redeem.txid()),
                poll_until_btc_has_expired,
            )
            .await
            {
                Either::Left((tx, _)) => tx,
                Either::Right(_) => return Err(SwapFailed::AfterBtcLock(Reason::BtcExpired)),
            };

            let tx_redeem_sig = tx_redeem
                .extract_signature_by_key(tx_redeem_published, b.public())
                .map_err(|_| SwapFailed::AfterBtcRedeem(Reason::BtcRedeemSignature))?;
            let s_a = bitcoin::recover(S_a_bitcoin, tx_redeem_sig, tx_redeem_encsig)
                .map_err(|_| SwapFailed::AfterBtcRedeem(Reason::SecretRecovery))?;
            let s_a = monero::PrivateKey::from_scalar(monero::Scalar::from_bytes_mod_order(
                s_a.to_bytes(),
            ));

            let s_b = monero::PrivateKey {
                scalar: s_b.into_ed25519(),
            };

            co.yield_(BobAction::CreateMoneroWalletForOutput {
                spend_key: s_a + s_b,
                view_key: v,
            })
            .await;

            Ok(())
        }
        .await;

        if let Err(err @ SwapFailed::AfterBtcLock(_)) = swap_result {
            error!("Swap failed, reason: {:?}", err);

            let tx_cancel =
                bitcoin::TxCancel::new(&tx_lock, refund_timelock, A.clone(), b.public());
            let tx_cancel_txid = tx_cancel.txid();
            let signed_tx_cancel = {
                let sig_a = tx_cancel_sig_a.clone();
                let sig_b = b.sign(tx_cancel.digest());

                tx_cancel
                    .clone()
                    .add_signatures(&tx_lock, (A.clone(), sig_a), (b.public(), sig_b))
                    .expect("sig_{a,b} to be valid signatures for tx_cancel")
            };

            co.yield_(BobAction::CancelBitcoin(signed_tx_cancel)).await;

            let _ = bitcoin_client
                .watch_for_raw_transaction(tx_cancel_txid)
                .await;

            let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &refund_address);
            let tx_refund_txid = tx_refund.txid();
            let signed_tx_refund = {
                let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

                let sig_a =
                    adaptor.decrypt_signature(&s_b.into_secp256k1(), tx_refund_encsig.clone());
                let sig_b = b.sign(tx_refund.digest());

                tx_refund
                    .add_signatures(&tx_cancel, (A.clone(), sig_a), (b.public(), sig_b))
                    .expect("sig_{a,b} to be valid signatures for tx_refund")
            };

            co.yield_(BobAction::RefundBitcoin(signed_tx_refund)).await;

            let _ = bitcoin_client
                .watch_for_raw_transaction(tx_refund_txid)
                .await;
        }
    })
}

#[derive(Debug)]
pub enum AliceAction {
    // This action also includes proving to Bob that this has happened, given that our current
    // protocol requires a transfer proof to verify that the coins have been locked on Monero
    LockXmr {
        amount: monero::Amount,
        public_spend_key: monero::PublicKey,
        public_view_key: monero::PublicViewKey,
    },
    RedeemBtc(bitcoin::Transaction),
    CreateMoneroWalletForOutput {
        spend_key: monero::PrivateKey,
        view_key: monero::PrivateViewKey,
    },
    CancelBtc(bitcoin::Transaction),
    PunishBtc(bitcoin::Transaction),
}

// TODO: This could be moved to the bitcoin module
#[async_trait]
pub trait ReceiveBitcoinRedeemEncsig {
    async fn receive_bitcoin_redeem_encsig(&mut self) -> bitcoin::EncryptedSignature;
}

/// Perform the on-chain protocol to swap monero and bitcoin as Alice.
///
/// This is called post handshake, after all the keys, addresses and most of the
/// signatures have been exchanged.
pub fn action_generator_alice<N, B>(
    mut network: N,
    bitcoin_client: Arc<B>,
    // TODO: Replace this with a new, slimmer struct?
    alice::State3 {
        a,
        B,
        s_a,
        S_b_monero,
        S_b_bitcoin,
        v,
        xmr,
        refund_timelock,
        punish_timelock,
        refund_address,
        redeem_address,
        punish_address,
        tx_lock,
        tx_punish_sig_bob,
        tx_cancel_sig_bob,
        ..
    }: alice::State3,
) -> GenBoxed<AliceAction, (), ()>
where
    N: ReceiveBitcoinRedeemEncsig + Send + Sync + 'static,
    B: BlockHeight
        + TransactionBlockHeight
        + bitcoin::WatchForRawTransaction
        + Send
        + Sync
        + 'static,
{
    #[derive(Debug)]
    enum SwapFailed {
        BeforeBtcLock,
        AfterXmrLock(Reason),
    }

    /// Reason why the swap has failed.
    #[derive(Debug)]
    enum Reason {
        /// The refund timelock has been reached.
        BtcExpired,
    }

    enum RefundFailed {
        BtcPunishable {
            tx_cancel_was_published: bool,
        },
        /// Could not find Alice's signature on the refund transaction witness
        /// stack.
        BtcRefundSignature,
        /// Could not recover secret `s_b` from Alice's refund transaction
        /// signature.
        SecretRecovery,
    }

    async fn poll_until(condition_future: impl Future<Output = bool> + Clone) {
        loop {
            if condition_future.clone().await {
                return;
            }

            tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
        }
    }

    async fn bitcoin_block_height_is_gte<B>(bitcoin_client: &B, n_blocks: u32) -> bool
    where
        B: BlockHeight,
    {
        bitcoin_client.block_height().await >= n_blocks
    }

    Gen::new_boxed(|co| async move {
        let swap_result: Result<(), SwapFailed> = async {
            timeout(
                Duration::from_secs(SECS_TO_ACT_BOB),
                bitcoin_client.watch_for_raw_transaction(tx_lock.txid()),
            )
            .await
            .map_err(|_| SwapFailed::BeforeBtcLock)?;

            let tx_lock_height = bitcoin_client
                .transaction_block_height(tx_lock.txid())
                .await;
            let btc_has_expired = bitcoin_block_height_is_gte(
                bitcoin_client.as_ref(),
                tx_lock_height + refund_timelock,
            )
            .shared();
            let poll_until_btc_has_expired = poll_until(btc_has_expired).shared();
            futures::pin_mut!(poll_until_btc_has_expired);

            let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey {
                scalar: s_a.into_ed25519(),
            });

            co.yield_(AliceAction::LockXmr {
                amount: xmr,
                public_spend_key: S_a + S_b_monero,
                public_view_key: v.public(),
            })
            .await;

            // TODO: Watch for LockXmr using watch-only wallet. Doing so will prevent Alice
            // from cancelling/refunding unnecessarily.

            let tx_redeem_encsig = match select(
                network.receive_bitcoin_redeem_encsig(),
                poll_until_btc_has_expired.clone(),
            )
            .await
            {
                Either::Left((encsig, _)) => encsig,
                Either::Right(_) => return Err(SwapFailed::AfterXmrLock(Reason::BtcExpired)),
            };

            let (signed_tx_redeem, tx_redeem_txid) = {
                let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

                let tx_redeem = bitcoin::TxRedeem::new(&tx_lock, &redeem_address);

                let sig_a = a.sign(tx_redeem.digest());
                let sig_b =
                    adaptor.decrypt_signature(&s_a.into_secp256k1(), tx_redeem_encsig.clone());

                let tx = tx_redeem
                    .add_signatures(&tx_lock, (a.public(), sig_a), (B.clone(), sig_b))
                    .expect("sig_{a,b} to be valid signatures for tx_redeem");
                let txid = tx.txid();

                (tx, txid)
            };

            co.yield_(AliceAction::RedeemBtc(signed_tx_redeem)).await;

            match select(
                bitcoin_client.watch_for_raw_transaction(tx_redeem_txid),
                poll_until_btc_has_expired,
            )
            .await
            {
                Either::Left(_) => {}
                Either::Right(_) => return Err(SwapFailed::AfterXmrLock(Reason::BtcExpired)),
            };

            Ok(())
        }
        .await;

        if let Err(SwapFailed::AfterXmrLock(Reason::BtcExpired)) = swap_result {
            let refund_result: Result<(), RefundFailed> = async {
                let bob_can_be_punished =
                    bitcoin_block_height_is_gte(bitcoin_client.as_ref(), punish_timelock).shared();
                let poll_until_bob_can_be_punished = poll_until(bob_can_be_punished).shared();
                futures::pin_mut!(poll_until_bob_can_be_punished);

                let tx_cancel =
                    bitcoin::TxCancel::new(&tx_lock, refund_timelock, a.public(), B.clone());
                match select(
                    bitcoin_client.watch_for_raw_transaction(tx_cancel.txid()),
                    poll_until_bob_can_be_punished.clone(),
                )
                .await
                {
                    Either::Left(_) => {}
                    Either::Right(_) => {
                        return Err(RefundFailed::BtcPunishable {
                            tx_cancel_was_published: false,
                        })
                    }
                };

                let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &refund_address);
                let tx_refund_published = match select(
                    bitcoin_client.watch_for_raw_transaction(tx_refund.txid()),
                    poll_until_bob_can_be_punished,
                )
                .await
                {
                    Either::Left((tx, _)) => tx,
                    Either::Right(_) => {
                        return Err(RefundFailed::BtcPunishable {
                            tx_cancel_was_published: true,
                        })
                    }
                };

                let s_a = monero::PrivateKey {
                    scalar: s_a.into_ed25519(),
                };

                let tx_refund_sig = tx_refund
                    .extract_signature_by_key(tx_refund_published, B.clone())
                    .map_err(|_| RefundFailed::BtcRefundSignature)?;
                let tx_refund_encsig = a.encsign(S_b_bitcoin.clone(), tx_refund.digest());

                let s_b = bitcoin::recover(S_b_bitcoin, tx_refund_sig, tx_refund_encsig)
                    .map_err(|_| RefundFailed::SecretRecovery)?;
                let s_b = monero::PrivateKey::from_scalar(monero::Scalar::from_bytes_mod_order(
                    s_b.to_bytes(),
                ));

                co.yield_(AliceAction::CreateMoneroWalletForOutput {
                    spend_key: s_a + s_b,
                    view_key: v,
                })
                .await;

                Ok(())
            }
            .await;

            // LIMITATION: When approaching the punish scenario, Bob could theoretically
            // wake up in between Alice's publication of tx cancel and beat Alice's punish
            // transaction with his refund transaction. Alice would then need to carry on
            // with the refund on Monero. Doing so may be too verbose with the current,
            // linear approach. A different design may be required
            if let Err(RefundFailed::BtcPunishable {
                tx_cancel_was_published,
            }) = refund_result
            {
                let tx_cancel =
                    bitcoin::TxCancel::new(&tx_lock, refund_timelock, a.public(), B.clone());

                if !tx_cancel_was_published {
                    let tx_cancel_txid = tx_cancel.txid();
                    let signed_tx_cancel = {
                        let sig_a = a.sign(tx_cancel.digest());
                        let sig_b = tx_cancel_sig_bob;

                        tx_cancel
                            .clone()
                            .add_signatures(&tx_lock, (a.public(), sig_a), (B.clone(), sig_b))
                            .expect("sig_{a,b} to be valid signatures for tx_cancel")
                    };

                    co.yield_(AliceAction::CancelBtc(signed_tx_cancel)).await;

                    let _ = bitcoin_client
                        .watch_for_raw_transaction(tx_cancel_txid)
                        .await;
                }

                let tx_punish =
                    bitcoin::TxPunish::new(&tx_cancel, &punish_address, punish_timelock);
                let tx_punish_txid = tx_punish.txid();
                let signed_tx_punish = {
                    let sig_a = a.sign(tx_punish.digest());
                    let sig_b = tx_punish_sig_bob;

                    tx_punish
                        .add_signatures(&tx_cancel, (a.public(), sig_a), (B, sig_b))
                        .expect("sig_{a,b} to be valid signatures for tx_cancel")
                };

                co.yield_(AliceAction::PunishBtc(signed_tx_punish)).await;

                let _ = bitcoin_client
                    .watch_for_raw_transaction(tx_punish_txid)
                    .await;
            }
        }
    })
}
