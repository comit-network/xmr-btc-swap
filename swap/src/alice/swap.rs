//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::{
    alice::{
        event_loop::EventLoopHandle,
        execution::{
            build_bitcoin_punish_transaction, build_bitcoin_redeem_transaction,
            extract_monero_private_key, lock_xmr, negotiate, publish_bitcoin_punish_transaction,
            publish_bitcoin_redeem_transaction, publish_cancel_transaction,
            wait_for_bitcoin_encrypted_signature, wait_for_bitcoin_refund, wait_for_locked_bitcoin,
        },
    },
    bitcoin::EncryptedSignature,
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
use std::{fmt, sync::Arc};
use tracing::info;
use xmr_btc::{
    alice::{State0, State3},
    bitcoin::{TransactionBlockHeight, TxCancel, TxRefund, WatchForRawTransaction},
    config::Config,
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
        state0: State0,
    },
    Negotiated {
        channel: Option<ResponseChannel<AliceToBob>>,
        amounts: SwapAmounts,
        state3: State3,
    },
    BtcLocked {
        channel: Option<ResponseChannel<AliceToBob>>,
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
        spend_key: monero::PrivateKey,
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

impl fmt::Display for AliceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AliceState::Started { .. } => write!(f, "started"),
            AliceState::Negotiated { .. } => write!(f, "negotiated"),
            AliceState::BtcLocked { .. } => write!(f, "btc_locked"),
            AliceState::XmrLocked { .. } => write!(f, "xmr_locked"),
            AliceState::EncSignLearned { .. } => write!(f, "encsig_sent"),
            AliceState::BtcRedeemed => write!(f, "btc_redeemed"),
            AliceState::BtcCancelled { .. } => write!(f, "btc_cancelled"),
            AliceState::BtcRefunded { .. } => write!(f, "btc_refunded"),
            AliceState::Punished => write!(f, "punished"),
            AliceState::SafelyAborted => write!(f, "safely_aborted"),
            AliceState::BtcPunishable { .. } => write!(f, "btc_punishable"),
            AliceState::XmrRefunded => write!(f, "xmr_refunded"),
            AliceState::WaitingToCancel { .. } => write!(f, "waiting_to_cancel"),
        }
    }
}

pub async fn swap(
    state: AliceState,
    swarm: EventLoopHandle,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
    config: Config,
) -> Result<(AliceState, EventLoopHandle)> {
    run_until(
        state,
        is_complete,
        swarm,
        bitcoin_wallet,
        monero_wallet,
        config,
    )
    .await
}

pub fn is_complete(state: &AliceState) -> bool {
    matches!(
        state,
        AliceState::XmrRefunded
            | AliceState::BtcRedeemed
            | AliceState::Punished
            | AliceState::SafelyAborted
    )
}

pub fn is_xmr_locked(state: &AliceState) -> bool {
    matches!(
        state,
        AliceState::XmrLocked{..}
    )
}

// State machine driver for swap execution
#[async_recursion]
pub async fn run_until(
    state: AliceState,
    is_target_state: fn(&AliceState) -> bool,
    mut swarm: EventLoopHandle,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
    config: Config,
) -> Result<(AliceState, EventLoopHandle)> {
    info!("Current state:{}", state);
    if is_target_state(&state) {
        Ok((state, swarm))
    } else {
        match state {
            AliceState::Started { amounts, state0 } => {
                let (channel, state3) = negotiate(state0, amounts, &mut swarm, config).await?;

                run_until(
                    AliceState::Negotiated {
                        channel: Some(channel),
                        amounts,
                        state3,
                    },
                    is_target_state,
                    swarm,
                    bitcoin_wallet,
                    monero_wallet,
                    config,
                )
                .await
            }
            AliceState::Negotiated {
                state3,
                channel,
                amounts,
            } => {
                match channel {
                    Some(channel) => {
                        let _ = wait_for_locked_bitcoin(
                            state3.tx_lock.txid(),
                            bitcoin_wallet.clone(),
                            config,
                        )
                        .await?;

                        run_until(
                            AliceState::BtcLocked {
                                channel: Some(channel),
                                amounts,
                                state3,
                            },
                            is_target_state,
                            swarm,
                            bitcoin_wallet,
                            monero_wallet,
                            config,
                        )
                        .await
                    }
                    None => {
                        tracing::info!("Cannot resume swap from negotiated state, aborting");

                        // Alice did not lock Xmr yet
                        run_until(
                            AliceState::SafelyAborted,
                            is_target_state,
                            swarm,
                            bitcoin_wallet,
                            monero_wallet,
                            config,
                        )
                        .await
                    }
                }
            }
            AliceState::BtcLocked {
                channel,
                amounts,
                state3,
            } => match channel {
                Some(channel) => {
                    lock_xmr(
                        channel,
                        amounts,
                        state3.clone(),
                        &mut swarm,
                        monero_wallet.clone(),
                    )
                    .await?;

                    run_until(
                        AliceState::XmrLocked { state3 },
                        is_target_state,
                        swarm,
                        bitcoin_wallet,
                        monero_wallet,
                        config,
                    )
                    .await
                }
                None => {
                    tracing::info!("Cannot resume swap from BTC locked state, aborting");

                    // Alice did not lock Xmr yet
                    run_until(
                        AliceState::SafelyAborted,
                        is_target_state,
                        swarm,
                        bitcoin_wallet,
                        monero_wallet,
                        config,
                    )
                    .await
                }
            },
            AliceState::XmrLocked { state3 } => {
                // Our Monero is locked, we need to go through the cancellation process if this
                // step fails
                match wait_for_bitcoin_encrypted_signature(
                    &mut swarm,
                    config.monero_max_finality_time,
                )
                .await
                {
                    Ok(encrypted_signature) => {
                        run_until(
                            AliceState::EncSignLearned {
                                state3,
                                encrypted_signature,
                            },
                            is_target_state,
                            swarm,
                            bitcoin_wallet,
                            monero_wallet,
                            config,
                        )
                        .await
                    }
                    Err(_) => {
                        run_until(
                            AliceState::WaitingToCancel { state3 },
                            is_target_state,
                            swarm,
                            bitcoin_wallet,
                            monero_wallet,
                            config,
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
                    state3.B,
                    &state3.redeem_address,
                ) {
                    Ok(tx) => tx,
                    Err(_) => {
                        return run_until(
                            AliceState::WaitingToCancel { state3 },
                            is_target_state,
                            swarm,
                            bitcoin_wallet,
                            monero_wallet,
                            config,
                        )
                        .await;
                    }
                };

                // TODO(Franck): Error handling is delicate here.
                // If Bob sees this transaction he can redeem Monero
                // e.g. If the Bitcoin node is down then the user needs to take action.
                publish_bitcoin_redeem_transaction(
                    signed_tx_redeem,
                    bitcoin_wallet.clone(),
                    config,
                )
                .await?;

                run_until(
                    AliceState::BtcRedeemed,
                    is_target_state,
                    swarm,
                    bitcoin_wallet,
                    monero_wallet,
                    config,
                )
                .await
            }
            AliceState::WaitingToCancel { state3 } => {
                let tx_cancel = publish_cancel_transaction(
                    state3.tx_lock.clone(),
                    state3.a.clone(),
                    state3.B,
                    state3.refund_timelock,
                    state3.tx_cancel_sig_bob.clone(),
                    bitcoin_wallet.clone(),
                )
                .await?;

                run_until(
                    AliceState::BtcCancelled { state3, tx_cancel },
                    is_target_state,
                    swarm,
                    bitcoin_wallet,
                    monero_wallet,
                    config,
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
                        run_until(
                            AliceState::BtcPunishable { tx_refund, state3 },
                            is_target_state,
                            swarm,
                            bitcoin_wallet.clone(),
                            monero_wallet,
                            config,
                        )
                        .await
                    }
                    Some(published_refund_tx) => {
                        let spend_key = extract_monero_private_key(
                            published_refund_tx,
                            tx_refund,
                            state3.s_a,
                            state3.a.clone(),
                            state3.S_b_bitcoin,
                        )?;

                        run_until(
                            AliceState::BtcRefunded { spend_key, state3 },
                            is_target_state,
                            swarm,
                            bitcoin_wallet.clone(),
                            monero_wallet,
                            config,
                        )
                        .await
                    }
                }
            }
            AliceState::BtcRefunded { spend_key, state3 } => {
                let view_key = state3.v;

                monero_wallet
                    .create_and_load_wallet_for_output(spend_key, view_key)
                    .await?;

                Ok((AliceState::XmrRefunded, swarm))
            }
            AliceState::BtcPunishable { tx_refund, state3 } => {
                let signed_tx_punish = build_bitcoin_punish_transaction(
                    &state3.tx_lock,
                    state3.refund_timelock,
                    &state3.punish_address,
                    state3.punish_timelock,
                    state3.tx_punish_sig_bob.clone(),
                    state3.a.clone(),
                    state3.B,
                )?;

                let punish_tx_finalised = publish_bitcoin_punish_transaction(
                    signed_tx_punish,
                    bitcoin_wallet.clone(),
                    config,
                );

                let refund_tx_seen = bitcoin_wallet.watch_for_raw_transaction(tx_refund.txid());

                pin_mut!(punish_tx_finalised);
                pin_mut!(refund_tx_seen);

                match select(punish_tx_finalised, refund_tx_seen).await {
                    Either::Left(_) => {
                        run_until(
                            AliceState::Punished,
                            is_target_state,
                            swarm,
                            bitcoin_wallet.clone(),
                            monero_wallet,
                            config,
                        )
                        .await
                    }
                    Either::Right((published_refund_tx, _)) => {
                        let spend_key = extract_monero_private_key(
                            published_refund_tx,
                            tx_refund,
                            state3.s_a,
                            state3.a.clone(),
                            state3.S_b_bitcoin,
                        )?;

                        run_until(
                            AliceState::BtcRefunded { spend_key, state3 },
                            is_target_state,
                            swarm,
                            bitcoin_wallet.clone(),
                            monero_wallet,
                            config,
                        )
                        .await
                    }
                }
            }
            AliceState::XmrRefunded => Ok((AliceState::XmrRefunded, swarm)),
            AliceState::BtcRedeemed => Ok((AliceState::BtcRedeemed, swarm)),
            AliceState::Punished => Ok((AliceState::Punished, swarm)),
            AliceState::SafelyAborted => Ok((AliceState::SafelyAborted, swarm)),
        }
    }
}
