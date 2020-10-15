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
pub mod transport;

use async_trait::async_trait;
use ecdsa_fun::{adaptor::Adaptor, nonce::Deterministic};
use futures::{
    future::{select, Either},
    Future, FutureExt,
};
use genawaiter::sync::{Gen, GenBoxed};
use sha2::Sha256;

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Action {
    LockBitcoin(bitcoin::TxLock),
    SendBitcoinRedeemEncsig(bitcoin::EncryptedSignature),
    CreateMoneroWalletForOutput {
        spend_key: monero::PrivateKey,
        view_key: monero::PrivateViewKey,
    },
    RefundBitcoin {
        tx_cancel: bitcoin::Transaction,
        tx_refund: bitcoin::Transaction,
    },
}

// TODO: This could be moved to the monero module
#[async_trait]
pub trait ReceiveTransferProof {
    async fn receive_transfer_proof(&mut self) -> monero::TransferProof;
}

#[async_trait]
pub trait MedianTime {
    async fn median_time(&self) -> u32;
}

/// Perform the on-chain protocol to swap monero and bitcoin as Bob.
///
/// This is called post handshake, after all the keys, addresses and most of the
/// signatures have been exchanged.
pub fn action_generator_bob<N, M, B>(
    network: &'static mut N,
    monero_ledger: &'static M,
    bitcoin_ledger: &'static B,
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
) -> GenBoxed<Action, (), ()>
where
    N: ReceiveTransferProof + Send + Sync,
    M: monero::WatchForTransfer + Send + Sync,
    B: MedianTime + bitcoin::WatchForRawTransaction + Send + Sync,
{
    enum SwapFailed {
        TimelockReached,
        InsufficientXMR(monero::InsufficientFunds),
    }

    async fn poll_until(condition_future: impl Future<Output = bool> + Clone) {
        loop {
            if condition_future.clone().await {
                return;
            }
        }
    }

    async fn bitcoin_time_is_gte<B>(bitcoin_client: &B, timestamp: u32) -> bool
    where
        B: MedianTime,
    {
        bitcoin_client.median_time().await >= timestamp
    }

    Gen::new_boxed(|co| async move {
        let swap_result: Result<(), SwapFailed> = async {
            let btc_has_expired = bitcoin_time_is_gte(bitcoin_ledger, refund_timelock).shared();

            if btc_has_expired.clone().await {
                return Err(SwapFailed::TimelockReached);
            }

            co.yield_(Action::LockBitcoin(tx_lock.clone())).await;

            let poll_until_btc_has_expired = poll_until(btc_has_expired).shared();
            futures::pin_mut!(poll_until_btc_has_expired);

            // the source of this could be the database, this layer doesn't care

            let transfer_proof = match select(
                network.receive_transfer_proof(),
                poll_until_btc_has_expired.clone(),
            )
            .await
            {
                Either::Left((proof, _)) => proof,
                Either::Right(_) => return Err(SwapFailed::TimelockReached),
            };

            let S_b_monero = monero::PublicKey::from_private_key(&monero::PrivateKey::from_scalar(
                s_b.into_ed25519(),
            ));
            let S = S_a_monero + S_b_monero;

            match select(
                monero_ledger.watch_for_transfer(
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
                Either::Left((Err(e), _)) => return Err(SwapFailed::InsufficientXMR(e)),
                Either::Right(_) => return Err(SwapFailed::TimelockReached),
                _ => {}
            }

            let tx_redeem = bitcoin::TxRedeem::new(&tx_lock, &redeem_address);
            let tx_redeem_encsig = b.encsign(S_a_bitcoin.clone(), tx_redeem.digest());

            co.yield_(Action::SendBitcoinRedeemEncsig(tx_redeem_encsig.clone()))
                .await;

            let tx_redeem_published = match select(
                bitcoin_ledger.watch_for_raw_transaction(tx_redeem.txid()),
                poll_until_btc_has_expired,
            )
            .await
            {
                Either::Left((tx, _)) => tx,
                Either::Right(_) => return Err(SwapFailed::TimelockReached),
            };

            // NOTE: If any of this fails, Bob will never be able to take the monero.
            // Therefore, there is no way to handle these errors other than aborting
            let tx_redeem_sig = tx_redeem
                .extract_signature_by_key(tx_redeem_published, b.public())
                .expect("redeem transaction must include signature from us");
            let s_a = bitcoin::recover(S_a_bitcoin, tx_redeem_sig, tx_redeem_encsig).expect(
                "alice can only produce our signature by decrypting our encrypted signature",
            );
            let s_a = monero::PrivateKey::from_scalar(monero::Scalar::from_bytes_mod_order(
                s_a.to_bytes(),
            ));

            let s_b = monero::PrivateKey {
                scalar: s_b.into_ed25519(),
            };

            co.yield_(Action::CreateMoneroWalletForOutput {
                spend_key: s_a + s_b,
                view_key: v,
            })
            .await;

            Ok(())
        }
        .await;

        if swap_result.is_err() {
            let tx_cancel =
                bitcoin::TxCancel::new(&tx_lock, refund_timelock, A.clone(), b.public());
            let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &refund_address);

            let signed_tx_cancel = {
                let sig_a = tx_cancel_sig_a.clone();
                let sig_b = b.sign(tx_cancel.digest());

                tx_cancel
                    .clone()
                    .add_signatures(&tx_lock, (A.clone(), sig_a), (b.public(), sig_b))
                    .expect("sig_{a,b} to be valid signatures for tx_cancel")
            };

            let signed_tx_refund = {
                let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

                let sig_a =
                    adaptor.decrypt_signature(&s_b.into_secp256k1(), tx_refund_encsig.clone());
                let sig_b = b.sign(tx_refund.digest());

                tx_refund
                    .add_signatures(&tx_cancel, (A.clone(), sig_a), (b.public(), sig_b))
                    .expect("sig_{a,b} to be valid signatures for tx_refund")
            };

            co.yield_(Action::RefundBitcoin {
                tx_cancel: signed_tx_cancel,
                tx_refund: signed_tx_refund,
            })
            .await;
        }
    })
}
