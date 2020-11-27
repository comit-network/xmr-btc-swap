//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::{
    alice::{
        execution::{lock_xmr, negotiate},
        OutEvent, Swarm,
    },
    bitcoin,
    bitcoin::{EncryptedSignature, TX_LOCK_MINE_TIMEOUT},
    monero,
    network::request_response::AliceToBob,
    SwapAmounts,
};
use anyhow::{anyhow, Context, Result};
use async_recursion::async_recursion;

use ecdsa_fun::{adaptor::Adaptor, nonce::Deterministic};
use futures::{
    future::{select, Either},
    pin_mut,
};

use libp2p::request_response::ResponseChannel;
use rand::{CryptoRng, RngCore};
use sha2::Sha256;
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;

use xmr_btc::{
    alice::State3,
    bitcoin::{
        poll_until_block_height_is_gte, BroadcastSignedTransaction, GetRawTransaction,
        TransactionBlockHeight, TxCancel, TxRefund, WaitForTransactionFinality,
        WatchForRawTransaction,
    },
    cross_curve_dleq,
    monero::CreateWalletForOutput,
};

trait Rng: RngCore + CryptoRng + Send {}

impl<T> Rng for T where T: RngCore + CryptoRng + Send {}

// The same data structure is used for swap execution and recovery.
// This allows for a seamless transition from a failed swap to recovery.
pub enum AliceState {
    Started {
        amounts: SwapAmounts,
        a: bitcoin::SecretKey,
        s_a: cross_curve_dleq::Scalar,
        v_a: monero::PrivateViewKey,
    },
    Negotiated {
        channel: ResponseChannel<AliceToBob>,
        amounts: SwapAmounts,
        state3: State3,
    },
    BtcLocked {
        channel: ResponseChannel<AliceToBob>,
        amounts: SwapAmounts,
        state3: State3,
    },
    XmrLocked {
        state3: State3,
    },
    EncSignLearned {
        state3: State3,
        encrypted_signature: EncryptedSignature,
    },
    BtcRedeemed,
    BtcCancelled {
        state3: State3,
        tx_cancel: TxCancel,
    },
    BtcRefunded {
        tx_refund: TxRefund,
        published_refund_tx: ::bitcoin::Transaction,
        state3: State3,
    },
    BtcPunishable {
        tx_refund: TxRefund,
        state3: State3,
    },
    BtcPunished {
        tx_refund: TxRefund,
        punished_tx_id: bitcoin::Txid,
        state3: State3,
    },
    XmrRefunded,
    WaitingToCancel {
        state3: State3,
    },
    Punished,
    SafelyAborted,
}

// State machine driver for swap execution
#[async_recursion]
pub async fn swap(
    state: AliceState,
    mut swarm: Swarm,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
) -> Result<AliceState> {
    match state {
        AliceState::Started {
            amounts,
            a,
            s_a,
            v_a,
        } => {
            let (channel, amounts, state3) =
                negotiate(amounts, a, s_a, v_a, &mut swarm, bitcoin_wallet.clone()).await?;

            swap(
                AliceState::Negotiated {
                    channel,
                    amounts,
                    state3,
                },
                swarm,
                bitcoin_wallet,
                monero_wallet,
            )
            .await
        }
        AliceState::Negotiated {
            state3,
            channel,
            amounts,
        } => {
            timeout(
                Duration::from_secs(TX_LOCK_MINE_TIMEOUT),
                bitcoin_wallet.wait_for_transaction_finality(state3.tx_lock.txid()),
            )
            .await
            .context("Timed out, Bob did not lock Bitcoin in time")?;

            swap(
                AliceState::BtcLocked {
                    channel,
                    amounts,
                    state3,
                },
                swarm,
                bitcoin_wallet,
                monero_wallet,
            )
            .await
        }
        AliceState::BtcLocked {
            channel,
            amounts,
            state3,
        } => {
            lock_xmr(
                channel,
                amounts,
                state3.clone(),
                &mut swarm,
                monero_wallet.clone(),
            )
            .await?;

            swap(
                AliceState::XmrLocked { state3 },
                swarm,
                bitcoin_wallet,
                monero_wallet,
            )
            .await
        }
        AliceState::XmrLocked { state3 } => {
            let encsig = timeout(
                // Give a set arbitrary time to Bob to send us `tx_redeem_encsign`
                Duration::from_secs(TX_LOCK_MINE_TIMEOUT),
                async {
                    match swarm.next().await {
                        OutEvent::Message3(msg) => Ok(msg.tx_redeem_encsig),
                        other => Err(anyhow!(
                            "Expected Bob's Bitcoin redeem encsig, got: {:?}",
                            other
                        )),
                    }
                },
            )
            .await
            .context("Timed out, Bob did not send redeem encsign in time");

            match encsig {
                Err(_timeout_error) => {
                    // TODO(Franck): Insert in DB

                    swap(
                        AliceState::WaitingToCancel { state3 },
                        swarm,
                        bitcoin_wallet,
                        monero_wallet,
                    )
                    .await
                }
                Ok(Err(_unexpected_msg_error)) => {
                    // TODO(Franck): Insert in DB

                    swap(
                        AliceState::WaitingToCancel { state3 },
                        swarm,
                        bitcoin_wallet,
                        monero_wallet,
                    )
                    .await
                }
                Ok(Ok(encrypted_signature)) => {
                    // TODO(Franck): Insert in DB

                    swap(
                        AliceState::EncSignLearned {
                            state3,
                            encrypted_signature,
                        },
                        swarm,
                        bitcoin_wallet,
                        monero_wallet,
                    )
                    .await
                }
            }
        }
        AliceState::EncSignLearned {
            state3,
            encrypted_signature,
        } => {
            let (signed_tx_redeem, _tx_redeem_txid) = {
                let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

                let tx_redeem = bitcoin::TxRedeem::new(&state3.tx_lock, &state3.redeem_address);

                bitcoin::verify_encsig(
                    state3.B.clone(),
                    state3.s_a.into_secp256k1().into(),
                    &tx_redeem.digest(),
                    &encrypted_signature,
                )
                .context("Invalid encrypted signature received")?;

                let sig_a = state3.a.sign(tx_redeem.digest());
                let sig_b = adaptor
                    .decrypt_signature(&state3.s_a.into_secp256k1(), encrypted_signature.clone());

                let tx = tx_redeem
                    .add_signatures(
                        &state3.tx_lock,
                        (state3.a.public(), sig_a),
                        (state3.B.clone(), sig_b),
                    )
                    .expect("sig_{a,b} to be valid signatures for tx_redeem");
                let txid = tx.txid();

                (tx, txid)
            };

            // TODO(Franck): Insert in db

            let _ = bitcoin_wallet
                .broadcast_signed_transaction(signed_tx_redeem)
                .await?;

            // TODO(Franck) Wait for confirmations

            swap(
                AliceState::BtcRedeemed,
                swarm,
                bitcoin_wallet,
                monero_wallet,
            )
            .await
        }
        AliceState::WaitingToCancel { state3 } => {
            let tx_lock_height = bitcoin_wallet
                .transaction_block_height(state3.tx_lock.txid())
                .await;
            poll_until_block_height_is_gte(
                bitcoin_wallet.as_ref(),
                tx_lock_height + state3.refund_timelock,
            )
            .await;

            let tx_cancel = bitcoin::TxCancel::new(
                &state3.tx_lock,
                state3.refund_timelock,
                state3.a.public(),
                state3.B.clone(),
            );

            if let Err(_e) = bitcoin_wallet.get_raw_transaction(tx_cancel.txid()).await {
                let sig_a = state3.a.sign(tx_cancel.digest());
                let sig_b = state3.tx_cancel_sig_bob.clone();

                let tx_cancel = tx_cancel
                    .clone()
                    .add_signatures(
                        &state3.tx_lock,
                        (state3.a.public(), sig_a),
                        (state3.B.clone(), sig_b),
                    )
                    .expect("sig_{a,b} to be valid signatures for tx_cancel");

                bitcoin_wallet
                    .broadcast_signed_transaction(tx_cancel)
                    .await?;
            }

            swap(
                AliceState::BtcCancelled { state3, tx_cancel },
                swarm,
                bitcoin_wallet,
                monero_wallet,
            )
            .await
        }
        AliceState::BtcCancelled { state3, tx_cancel } => {
            let tx_cancel_height = bitcoin_wallet
                .transaction_block_height(tx_cancel.txid())
                .await;

            let reached_t2 = poll_until_block_height_is_gte(
                bitcoin_wallet.as_ref(),
                tx_cancel_height + state3.punish_timelock,
            );

            let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &state3.refund_address);
            let seen_refund_tx = bitcoin_wallet.watch_for_raw_transaction(tx_refund.txid());

            pin_mut!(reached_t2);
            pin_mut!(seen_refund_tx);

            match select(reached_t2, seen_refund_tx).await {
                Either::Left(_) => {
                    swap(
                        AliceState::BtcPunishable { tx_refund, state3 },
                        swarm,
                        bitcoin_wallet.clone(),
                        monero_wallet,
                    )
                    .await
                }
                Either::Right((published_refund_tx, _)) => {
                    swap(
                        AliceState::BtcRefunded {
                            tx_refund,
                            published_refund_tx,
                            state3,
                        },
                        swarm,
                        bitcoin_wallet.clone(),
                        monero_wallet,
                    )
                    .await
                }
            }
        }
        AliceState::BtcRefunded {
            tx_refund,
            published_refund_tx,
            state3,
        } => {
            let s_a = monero::PrivateKey {
                scalar: state3.s_a.into_ed25519(),
            };

            let tx_refund_sig = tx_refund
                .extract_signature_by_key(published_refund_tx, state3.a.public())
                .context("Failed to extract signature from Bitcoin refund tx")?;
            let tx_refund_encsig = state3
                .a
                .encsign(state3.S_b_bitcoin.clone(), tx_refund.digest());

            let s_b = bitcoin::recover(state3.S_b_bitcoin, tx_refund_sig, tx_refund_encsig)
                .context("Failed to recover Monero secret key from Bitcoin signature")?;
            let s_b = monero::private_key_from_secp256k1_scalar(s_b.into());

            let spend_key = s_a + s_b;
            let view_key = state3.v;

            monero_wallet
                .create_and_load_wallet_for_output(spend_key, view_key)
                .await?;

            Ok(AliceState::XmrRefunded)
        }
        AliceState::BtcPunishable { tx_refund, state3 } => {
            let tx_cancel = bitcoin::TxCancel::new(
                &state3.tx_lock,
                state3.refund_timelock,
                state3.a.public(),
                state3.B.clone(),
            );
            let tx_punish =
                bitcoin::TxPunish::new(&tx_cancel, &state3.punish_address, state3.punish_timelock);
            let punished_tx_id = tx_punish.txid();

            let sig_a = state3.a.sign(tx_punish.digest());
            let sig_b = state3.tx_punish_sig_bob.clone();

            let signed_tx_punish = tx_punish
                .add_signatures(
                    &tx_cancel,
                    (state3.a.public(), sig_a),
                    (state3.B.clone(), sig_b),
                )
                .expect("sig_{a,b} to be valid signatures for tx_cancel");

            let _ = bitcoin_wallet
                .broadcast_signed_transaction(signed_tx_punish)
                .await?;

            swap(
                AliceState::BtcPunished {
                    tx_refund,
                    punished_tx_id,
                    state3,
                },
                swarm,
                bitcoin_wallet.clone(),
                monero_wallet,
            )
            .await
        }
        AliceState::BtcPunished {
            punished_tx_id,
            tx_refund,
            state3,
        } => {
            let punish_tx_finalised = bitcoin_wallet.wait_for_transaction_finality(punished_tx_id);

            let refund_tx_seen = bitcoin_wallet.watch_for_raw_transaction(tx_refund.txid());

            pin_mut!(punish_tx_finalised);
            pin_mut!(refund_tx_seen);

            match select(punish_tx_finalised, refund_tx_seen).await {
                Either::Left(_) => {
                    swap(
                        AliceState::Punished,
                        swarm,
                        bitcoin_wallet.clone(),
                        monero_wallet,
                    )
                    .await
                }
                Either::Right((published_refund_tx, _)) => {
                    swap(
                        AliceState::BtcRefunded {
                            tx_refund,
                            published_refund_tx,
                            state3,
                        },
                        swarm,
                        bitcoin_wallet.clone(),
                        monero_wallet,
                    )
                    .await
                }
            }
        }

        AliceState::XmrRefunded => Ok(AliceState::XmrRefunded),
        AliceState::BtcRedeemed => Ok(AliceState::BtcRedeemed),
        AliceState::Punished => Ok(AliceState::Punished),
        AliceState::SafelyAborted => Ok(AliceState::SafelyAborted),
    }
}
