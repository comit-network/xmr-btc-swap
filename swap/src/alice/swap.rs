//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::{
    alice::{
        execution::{
            build_bitcoin_punish_transaction, build_bitcoin_redeem_transaction,
            extract_monero_private_key, lock_xmr, negotiate, publish_bitcoin_punish_transaction,
            publish_bitcoin_redeem_transaction, publish_cancel_transaction,
            wait_for_bitcoin_encrypted_signature, wait_for_bitcoin_refund, wait_for_locked_bitcoin,
        },
        Swarm,
    },
    bitcoin,
    bitcoin::EncryptedSignature,
    monero,
    network::request_response::AliceToBob,
    SwapAmounts,
};
use anyhow::Result;
use async_recursion::async_recursion;
use futures::{
    future::{select, Either},
    pin_mut,
};
use libp2p::request_response::ResponseChannel;
use rand::{CryptoRng, RngCore};
use std::sync::Arc;
use xmr_btc::{
    alice::State3,
    bitcoin::{TransactionBlockHeight, TxCancel, TxRefund, WatchForRawTransaction},
    cross_curve_dleq,
    monero::CreateWalletForOutput,
};

trait Rng: RngCore + CryptoRng + Send {}

impl<T> Rng for T where T: RngCore + CryptoRng + Send {}

// The same data structure is used for swap execution and recovery.
// This allows for a seamless transition from a failed swap to recovery.
#[allow(clippy::large_enum_variant)]
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
            let (channel, state3) =
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
            let _ = wait_for_locked_bitcoin(state3.tx_lock.txid(), bitcoin_wallet.clone()).await?;

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
            // Our Monero is locked, we need to go through the cancellation process if this
            // step fails
            match wait_for_bitcoin_encrypted_signature(&mut swarm).await {
                Ok(encrypted_signature) => {
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
                Err(_) => {
                    swap(
                        AliceState::WaitingToCancel { state3 },
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
            let signed_tx_redeem = match build_bitcoin_redeem_transaction(
                encrypted_signature,
                &state3.tx_lock,
                state3.a.clone(),
                state3.s_a,
                state3.B.clone(),
                &state3.redeem_address,
            ) {
                Ok(tx) => tx,
                Err(_) => {
                    return swap(
                        AliceState::WaitingToCancel { state3 },
                        swarm,
                        bitcoin_wallet,
                        monero_wallet,
                    )
                    .await;
                }
            };

            // TODO(Franck): Error handling is delicate here.
            // If Bob sees this transaction he can redeem Monero
            // e.g. If the Bitcoin node is down then the user needs to take action.
            publish_bitcoin_redeem_transaction(signed_tx_redeem, bitcoin_wallet.clone()).await?;

            swap(
                AliceState::BtcRedeemed,
                swarm,
                bitcoin_wallet,
                monero_wallet,
            )
            .await
        }
        AliceState::WaitingToCancel { state3 } => {
            let tx_cancel = publish_cancel_transaction(
                state3.tx_lock.clone(),
                state3.a.clone(),
                state3.B.clone(),
                state3.refund_timelock,
                state3.tx_cancel_sig_bob.clone(),
                bitcoin_wallet.clone(),
            )
            .await?;

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

            let (tx_refund, published_refund_tx) = wait_for_bitcoin_refund(
                &tx_cancel,
                tx_cancel_height,
                state3.punish_timelock,
                &state3.refund_address,
                bitcoin_wallet.clone(),
            )
            .await?;

            // TODO(Franck): Review error handling
            match published_refund_tx {
                None => {
                    swap(
                        AliceState::BtcPunishable { tx_refund, state3 },
                        swarm,
                        bitcoin_wallet.clone(),
                        monero_wallet,
                    )
                    .await
                }
                Some(published_refund_tx) => {
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
            let spend_key = extract_monero_private_key(
                published_refund_tx,
                tx_refund,
                state3.s_a,
                state3.a.clone(),
                state3.S_b_bitcoin,
            )?;
            let view_key = state3.v;

            monero_wallet
                .create_and_load_wallet_for_output(spend_key, view_key)
                .await?;

            Ok(AliceState::XmrRefunded)
        }
        AliceState::BtcPunishable { tx_refund, state3 } => {
            let signed_tx_punish = build_bitcoin_punish_transaction(
                &state3.tx_lock,
                state3.refund_timelock,
                &state3.punish_address,
                state3.punish_timelock,
                state3.tx_punish_sig_bob.clone(),
                state3.a.clone(),
                state3.B.clone(),
            )?;

            let punish_tx_finalised =
                publish_bitcoin_punish_transaction(signed_tx_punish, bitcoin_wallet.clone());

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
