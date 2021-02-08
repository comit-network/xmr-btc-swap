//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::{
    bitcoin,
    bitcoin::{
        timelocks::ExpiredTimelocks, TransactionBlockHeight, WaitForTransactionFinality,
        WatchForRawTransaction,
    },
    database,
    database::Database,
    execution_params::ExecutionParams,
    monero,
    monero::CreateWalletForOutput,
    protocol::{
        alice,
        alice::{
            event_loop::EventLoopHandle,
            steps::{
                build_bitcoin_punish_transaction, build_bitcoin_redeem_transaction,
                extract_monero_private_key, lock_xmr, publish_bitcoin_punish_transaction,
                publish_bitcoin_redeem_transaction, publish_cancel_transaction,
                wait_for_bitcoin_encrypted_signature, wait_for_bitcoin_refund,
                wait_for_locked_bitcoin,
            },
            AliceState,
        },
    },
};
use anyhow::{bail, Result};
use async_recursion::async_recursion;
use futures::{
    future::{select, Either},
    pin_mut,
};
use rand::{CryptoRng, RngCore};
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

trait Rng: RngCore + CryptoRng + Send {}

impl<T> Rng for T where T: RngCore + CryptoRng + Send {}

pub fn is_complete(state: &AliceState) -> bool {
    matches!(
        state,
        AliceState::XmrRefunded
            | AliceState::BtcRedeemed
            | AliceState::BtcPunished
            | AliceState::SafelyAborted
    )
}

pub async fn run(swap: alice::Swap) -> Result<AliceState> {
    run_until(swap, is_complete).await
}

#[tracing::instrument(name = "swap", skip(swap,is_target_state), fields(id = %swap.swap_id))]
pub async fn run_until(
    swap: alice::Swap,
    is_target_state: fn(&AliceState) -> bool,
) -> Result<AliceState> {
    run_until_internal(
        swap.state,
        is_target_state,
        swap.event_loop_handle,
        swap.bitcoin_wallet,
        swap.monero_wallet,
        swap.execution_params,
        swap.swap_id,
        swap.db,
    )
    .await
}

// State machine driver for swap execution
#[async_recursion]
#[allow(clippy::too_many_arguments)]
async fn run_until_internal(
    state: AliceState,
    is_target_state: fn(&AliceState) -> bool,
    mut event_loop_handle: EventLoopHandle,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    execution_params: ExecutionParams,
    swap_id: Uuid,
    db: Database,
) -> Result<AliceState> {
    info!("Current state:{}", state);
    if is_target_state(&state) {
        Ok(state)
    } else {
        match state {
            AliceState::Started { amounts, state0 } => {
                let state = AliceState::Negotiated {
                    bob_peer_id: todo!(),
                    amounts,
                    state3: todo!(),
                };

                let db_state = (&state).into();
                db.insert_latest_state(swap_id, database::Swap::Alice(db_state))
                    .await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    bitcoin_wallet,
                    monero_wallet,
                    execution_params,
                    swap_id,
                    db,
                )
                .await
            }
            AliceState::Negotiated {
                state3,
                bob_peer_id,
                amounts,
            } => {
                let _ = wait_for_locked_bitcoin(
                    state3.tx_lock.txid(),
                    bitcoin_wallet.clone(),
                    execution_params,
                )
                .await?;

                let state = AliceState::BtcLocked {
                    bob_peer_id,
                    amounts,
                    state3,
                };

                let db_state = (&state).into();
                db.insert_latest_state(swap_id, database::Swap::Alice(db_state))
                    .await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    bitcoin_wallet,
                    monero_wallet,
                    execution_params,
                    swap_id,
                    db,
                )
                .await
            }
            AliceState::BtcLocked {
                bob_peer_id,
                amounts,
                state3,
            } => {
                lock_xmr(
                    bob_peer_id,
                    amounts,
                    *state3.clone(),
                    &mut event_loop_handle,
                    monero_wallet.clone(),
                    execution_params,
                )
                .await?;

                let state = AliceState::XmrLocked { state3 };

                let db_state = (&state).into();
                db.insert_latest_state(swap_id, database::Swap::Alice(db_state))
                    .await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    bitcoin_wallet,
                    monero_wallet,
                    execution_params,
                    swap_id,
                    db,
                )
                .await
            }
            AliceState::XmrLocked { state3 } => {
                let state = match state3.expired_timelocks(bitcoin_wallet.as_ref()).await? {
                    ExpiredTimelocks::None => {
                        let wait_for_enc_sig =
                            wait_for_bitcoin_encrypted_signature(&mut event_loop_handle);
                        let state3_clone = state3.clone();
                        let cancel_timelock_expires = state3_clone
                            .wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref());

                        pin_mut!(wait_for_enc_sig);
                        pin_mut!(cancel_timelock_expires);

                        match select(cancel_timelock_expires, wait_for_enc_sig).await {
                            Either::Left(_) => AliceState::CancelTimelockExpired { state3 },
                            Either::Right((enc_sig, _)) => AliceState::EncSigLearned {
                                state3,
                                encrypted_signature: Box::new(enc_sig?),
                            },
                        }
                    }
                    _ => AliceState::CancelTimelockExpired { state3 },
                };

                let db_state = (&state).into();
                db.insert_latest_state(swap_id, database::Swap::Alice(db_state))
                    .await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    bitcoin_wallet.clone(),
                    monero_wallet,
                    execution_params,
                    swap_id,
                    db,
                )
                .await
            }
            AliceState::EncSigLearned {
                state3,
                encrypted_signature,
            } => {
                let state = match state3.expired_timelocks(bitcoin_wallet.as_ref()).await? {
                    ExpiredTimelocks::None => {
                        match build_bitcoin_redeem_transaction(
                            *encrypted_signature,
                            &state3.tx_lock,
                            state3.a.clone(),
                            state3.s_a,
                            state3.B,
                            &state3.redeem_address,
                        ) {
                            Ok(tx) => {
                                match publish_bitcoin_redeem_transaction(tx, bitcoin_wallet.clone())
                                    .await
                                {
                                    Ok(txid) => {
                                        let publishded_redeem_tx = bitcoin_wallet
                                            .wait_for_transaction_finality(txid, execution_params)
                                            .await;

                                        match publishded_redeem_tx {
                                            Ok(_) => AliceState::BtcRedeemed,
                                            Err(e) => {
                                                bail!("Waiting for Bitcoin transaction finality failed with {}! The redeem transaction was published, but it is not ensured that the transaction was included! You're screwed.", e)
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("Publishing the redeem transaction failed with {}, attempting to wait for cancellation now. If you restart the application before the timelock is expired publishing the redeem transaction will be retried.", e);
                                        state3
                                            .wait_for_cancel_timelock_to_expire(
                                                bitcoin_wallet.as_ref(),
                                            )
                                            .await?;

                                        AliceState::CancelTimelockExpired { state3 }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Constructing the redeem transaction failed with {}, attempting to wait for cancellation now.", e);
                                state3
                                    .wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref())
                                    .await?;

                                AliceState::CancelTimelockExpired { state3 }
                            }
                        }
                    }
                    _ => AliceState::CancelTimelockExpired { state3 },
                };

                let db_state = (&state).into();
                db.insert_latest_state(swap_id, database::Swap::Alice(db_state))
                    .await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    bitcoin_wallet,
                    monero_wallet,
                    execution_params,
                    swap_id,
                    db,
                )
                .await
            }
            AliceState::CancelTimelockExpired { state3 } => {
                let tx_cancel = publish_cancel_transaction(
                    state3.tx_lock.clone(),
                    state3.a.clone(),
                    state3.B,
                    state3.cancel_timelock,
                    state3.tx_cancel_sig_bob.clone(),
                    bitcoin_wallet.clone(),
                )
                .await?;

                let state = AliceState::BtcCancelled {
                    state3,
                    tx_cancel: Box::new(tx_cancel),
                };
                let db_state = (&state).into();
                db.insert_latest_state(swap_id, database::Swap::Alice(db_state))
                    .await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    bitcoin_wallet,
                    monero_wallet,
                    execution_params,
                    swap_id,
                    db,
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
                        let state = AliceState::BtcPunishable { tx_refund, state3 };
                        let db_state = (&state).into();
                        db.insert_latest_state(swap_id, database::Swap::Alice(db_state))
                            .await?;

                        run_until_internal(
                            state,
                            is_target_state,
                            event_loop_handle,
                            bitcoin_wallet.clone(),
                            monero_wallet,
                            execution_params,
                            swap_id,
                            db,
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

                        let state = AliceState::BtcRefunded { spend_key, state3 };
                        let db_state = (&state).into();
                        db.insert_latest_state(swap_id, database::Swap::Alice(db_state))
                            .await?;
                        run_until_internal(
                            state,
                            is_target_state,
                            event_loop_handle,
                            bitcoin_wallet.clone(),
                            monero_wallet,
                            execution_params,
                            swap_id,
                            db,
                        )
                        .await
                    }
                }
            }
            AliceState::BtcRefunded { spend_key, state3 } => {
                let view_key = state3.v;

                monero_wallet
                    .create_and_load_wallet_for_output(spend_key, view_key, None)
                    .await?;

                let state = AliceState::XmrRefunded;
                let db_state = (&state).into();
                db.insert_latest_state(swap_id, database::Swap::Alice(db_state))
                    .await?;
                Ok(state)
            }
            AliceState::BtcPunishable { tx_refund, state3 } => {
                let signed_tx_punish = build_bitcoin_punish_transaction(
                    &state3.tx_lock,
                    state3.cancel_timelock,
                    &state3.punish_address,
                    state3.punish_timelock,
                    state3.tx_punish_sig_bob.clone(),
                    state3.a.clone(),
                    state3.B,
                )?;

                let punish_tx_finalised = publish_bitcoin_punish_transaction(
                    signed_tx_punish,
                    bitcoin_wallet.clone(),
                    execution_params,
                );

                let refund_tx_seen = bitcoin_wallet.watch_for_raw_transaction(tx_refund.txid());

                pin_mut!(punish_tx_finalised);
                pin_mut!(refund_tx_seen);

                match select(refund_tx_seen, punish_tx_finalised).await {
                    Either::Left((published_refund_tx, _)) => {
                        let spend_key = extract_monero_private_key(
                            published_refund_tx,
                            tx_refund,
                            state3.s_a,
                            state3.a.clone(),
                            state3.S_b_bitcoin,
                        )?;
                        let state = AliceState::BtcRefunded { spend_key, state3 };
                        let db_state = (&state).into();
                        db.insert_latest_state(swap_id, database::Swap::Alice(db_state))
                            .await?;
                        run_until_internal(
                            state,
                            is_target_state,
                            event_loop_handle,
                            bitcoin_wallet.clone(),
                            monero_wallet,
                            execution_params,
                            swap_id,
                            db,
                        )
                        .await
                    }
                    Either::Right(_) => {
                        let state = AliceState::BtcPunished;
                        let db_state = (&state).into();
                        db.insert_latest_state(swap_id, database::Swap::Alice(db_state))
                            .await?;
                        run_until_internal(
                            state,
                            is_target_state,
                            event_loop_handle,
                            bitcoin_wallet.clone(),
                            monero_wallet,
                            execution_params,
                            swap_id,
                            db,
                        )
                        .await
                    }
                }
            }
            AliceState::XmrRefunded => Ok(AliceState::XmrRefunded),
            AliceState::BtcRedeemed => Ok(AliceState::BtcRedeemed),
            AliceState::BtcPunished => Ok(AliceState::BtcPunished),
            AliceState::SafelyAborted => Ok(AliceState::SafelyAborted),
        }
    }
}
