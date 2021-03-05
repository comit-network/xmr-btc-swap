use crate::bitcoin::ExpiredTimelocks;
use crate::database::{Database, Swap};
use crate::execution_params::ExecutionParams;
use crate::monero::InsufficientFunds;
use crate::protocol::bob;
use crate::protocol::bob::event_loop::EventLoopHandle;
use crate::protocol::bob::state::*;
use crate::{bitcoin, monero};
use anyhow::{bail, Result};
use async_recursion::async_recursion;
use rand::rngs::OsRng;
use std::sync::Arc;
use tokio::select;
use tracing::{trace, warn};
use uuid::Uuid;

pub fn is_complete(state: &BobState) -> bool {
    matches!(
        state,
        BobState::BtcRefunded(..)
            | BobState::XmrRedeemed { .. }
            | BobState::BtcPunished { .. }
            | BobState::SafelyAborted
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn run(swap: bob::Swap) -> Result<BobState> {
    run_until(swap, is_complete).await
}

pub async fn run_until(
    swap: bob::Swap,
    is_target_state: fn(&BobState) -> bool,
) -> Result<BobState> {
    run_until_internal(
        swap.state,
        is_target_state,
        swap.event_loop_handle,
        swap.db,
        swap.bitcoin_wallet,
        swap.monero_wallet,
        swap.swap_id,
        swap.execution_params,
        swap.receive_monero_address,
    )
    .await
}

// State machine driver for swap execution
#[allow(clippy::too_many_arguments)]
#[async_recursion]
async fn run_until_internal(
    state: BobState,
    is_target_state: fn(&BobState) -> bool,
    mut event_loop_handle: EventLoopHandle,
    db: Database,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    swap_id: Uuid,
    execution_params: ExecutionParams,
    receive_monero_address: monero::Address,
) -> Result<BobState> {
    trace!("Current state: {}", state);
    if is_target_state(&state) {
        Ok(state)
    } else {
        match state {
            BobState::Started { btc_amount } => {
                let bitcoin_refund_address = bitcoin_wallet.new_address().await?;

                event_loop_handle.dial().await?;

                let state2 = request_price_and_setup(
                    btc_amount,
                    &mut event_loop_handle,
                    execution_params,
                    bitcoin_refund_address,
                )
                .await?;

                let state = BobState::ExecutionSetupDone(state2);
                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, Swap::Bob(db_state)).await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    swap_id,
                    execution_params,
                    receive_monero_address,
                )
                .await
            }
            BobState::ExecutionSetupDone(state2) => {
                // Do not lock Bitcoin if not connected to Alice.
                event_loop_handle.dial().await?;
                // Alice and Bob have exchanged info
                let state3 = state2.lock_btc(bitcoin_wallet.as_ref()).await?;

                let state = BobState::BtcLocked(state3);
                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, Swap::Bob(db_state)).await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    swap_id,
                    execution_params,
                    receive_monero_address,
                )
                .await
            }
            // Bob has locked Btc
            // Watch for Alice to Lock Xmr or for cancel timelock to elapse
            BobState::BtcLocked(state3) => {
                let state = if let ExpiredTimelocks::None =
                    state3.current_epoch(bitcoin_wallet.as_ref()).await?
                {
                    event_loop_handle.dial().await?;

                    let transfer_proof_watcher = event_loop_handle.recv_transfer_proof();
                    let cancel_timelock_expires =
                        state3.wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref());

                    // Record the current monero wallet block height so we don't have to scan from
                    // block 0 once we create the redeem wallet.
                    let monero_wallet_restore_blockheight = monero_wallet.block_height().await?;

                    tracing::info!("Waiting for Alice to lock Monero");

                    select! {
                        transfer_proof = transfer_proof_watcher => {
                            let transfer_proof = transfer_proof?.tx_lock_proof;

                            tracing::info!(txid = %transfer_proof.tx_hash(), "Alice locked Monero");

                            BobState::XmrLockProofReceived {
                                state: state3,
                                lock_transfer_proof: transfer_proof,
                                monero_wallet_restore_blockheight
                            }
                        },
                        _ = cancel_timelock_expires => {
                            tracing::info!("Alice took too long to lock Monero, cancelling the swap");

                            let state4 = state3.cancel();
                            BobState::CancelTimelockExpired(state4)
                        }
                    }
                } else {
                    let state4 = state3.cancel();
                    BobState::CancelTimelockExpired(state4)
                };
                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, Swap::Bob(db_state)).await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    swap_id,
                    execution_params,
                    receive_monero_address,
                )
                .await
            }
            BobState::XmrLockProofReceived {
                state,
                lock_transfer_proof,
                monero_wallet_restore_blockheight,
            } => {
                let state = if let ExpiredTimelocks::None =
                    state.current_epoch(bitcoin_wallet.as_ref()).await?
                {
                    event_loop_handle.dial().await?;

                    let xmr_lock_watcher = state.clone().watch_for_lock_xmr(
                        monero_wallet.as_ref(),
                        lock_transfer_proof,
                        monero_wallet_restore_blockheight,
                    );
                    let cancel_timelock_expires =
                        state.wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref());

                    select! {
                        state4 = xmr_lock_watcher => {
                            match state4? {
                                Ok(state4) => BobState::XmrLocked(state4),
                                Err(InsufficientFunds {..}) => {
                                     warn!("The other party has locked insufficient Monero funds! Waiting for refund...");
                                     state.wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref()).await?;
                                     let state4 = state.cancel();
                                     BobState::CancelTimelockExpired(state4)
                                },
                            }
                        },
                        _ = cancel_timelock_expires => {
                            let state4 = state.cancel();
                            BobState::CancelTimelockExpired(state4)
                        }
                    }
                } else {
                    let state4 = state.cancel();
                    BobState::CancelTimelockExpired(state4)
                };

                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, Swap::Bob(db_state)).await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    swap_id,
                    execution_params,
                    receive_monero_address,
                )
                .await
            }
            BobState::XmrLocked(state) => {
                let state = if let ExpiredTimelocks::None =
                    state.expired_timelock(bitcoin_wallet.as_ref()).await?
                {
                    event_loop_handle.dial().await?;
                    // Alice has locked Xmr
                    // Bob sends Alice his key
                    let tx_redeem_encsig = state.tx_redeem_encsig();

                    let state4_clone = state.clone();

                    let enc_sig_sent_watcher =
                        event_loop_handle.send_encrypted_signature(tx_redeem_encsig);
                    let bitcoin_wallet = bitcoin_wallet.clone();
                    let cancel_timelock_expires =
                        state4_clone.wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref());

                    select! {
                        _ = enc_sig_sent_watcher => {
                            BobState::EncSigSent(state)
                        },
                        _ = cancel_timelock_expires => {
                            BobState::CancelTimelockExpired(state)
                        }
                    }
                } else {
                    BobState::CancelTimelockExpired(state)
                };
                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, Swap::Bob(db_state)).await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    swap_id,
                    execution_params,
                    receive_monero_address,
                )
                .await
            }
            BobState::EncSigSent(state) => {
                let state = if let ExpiredTimelocks::None =
                    state.expired_timelock(bitcoin_wallet.as_ref()).await?
                {
                    let state_clone = state.clone();
                    let redeem_watcher = state_clone.watch_for_redeem_btc(bitcoin_wallet.as_ref());
                    let cancel_timelock_expires =
                        state_clone.wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref());

                    select! {
                        state5 = redeem_watcher => {
                            BobState::BtcRedeemed(state5?)
                        },
                        _ = cancel_timelock_expires => {
                            BobState::CancelTimelockExpired(state)
                        }
                    }
                } else {
                    BobState::CancelTimelockExpired(state)
                };

                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, Swap::Bob(db_state)).await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet.clone(),
                    monero_wallet,
                    swap_id,
                    execution_params,
                    receive_monero_address,
                )
                .await
            }
            BobState::BtcRedeemed(state) => {
                // Bob redeems XMR using revealed s_a
                state.claim_xmr(monero_wallet.as_ref()).await?;

                // Ensure that the generated wallet is synced so we have a proper balance
                monero_wallet.refresh().await?;
                // Sweep (transfer all funds) to the given address
                let tx_hashes = monero_wallet.sweep_all(receive_monero_address).await?;

                for tx_hash in tx_hashes {
                    tracing::info!("Sent XMR to {} in tx {}", receive_monero_address, tx_hash.0);
                }

                let state = BobState::XmrRedeemed {
                    tx_lock_id: state.tx_lock_id(),
                };
                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, Swap::Bob(db_state)).await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    swap_id,
                    execution_params,
                    receive_monero_address,
                )
                .await
            }
            BobState::CancelTimelockExpired(state4) => {
                if state4
                    .check_for_tx_cancel(bitcoin_wallet.as_ref())
                    .await
                    .is_err()
                {
                    state4.submit_tx_cancel(bitcoin_wallet.as_ref()).await?;
                }

                let state = BobState::BtcCancelled(state4);
                db.insert_latest_state(swap_id, Swap::Bob(state.clone().into()))
                    .await?;

                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    swap_id,
                    execution_params,
                    receive_monero_address,
                )
                .await
            }
            BobState::BtcCancelled(state) => {
                // Bob has cancelled the swap
                let state = match state.expired_timelock(bitcoin_wallet.as_ref()).await? {
                    ExpiredTimelocks::None => {
                        bail!("Internal error: canceled state reached before cancel timelock was expired");
                    }
                    ExpiredTimelocks::Cancel => {
                        state
                            .refund_btc(bitcoin_wallet.as_ref(), execution_params)
                            .await?;
                        BobState::BtcRefunded(state)
                    }
                    ExpiredTimelocks::Punish => BobState::BtcPunished {
                        tx_lock_id: state.tx_lock_id(),
                    },
                };

                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, Swap::Bob(db_state)).await?;
                run_until_internal(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    swap_id,
                    execution_params,
                    receive_monero_address,
                )
                .await
            }
            BobState::BtcRefunded(state4) => Ok(BobState::BtcRefunded(state4)),
            BobState::BtcPunished { tx_lock_id } => Ok(BobState::BtcPunished { tx_lock_id }),
            BobState::SafelyAborted => Ok(BobState::SafelyAborted),
            BobState::XmrRedeemed { tx_lock_id } => Ok(BobState::XmrRedeemed { tx_lock_id }),
        }
    }
}

pub async fn request_price_and_setup(
    btc: bitcoin::Amount,
    event_loop_handle: &mut EventLoopHandle,
    execution_params: ExecutionParams,
    bitcoin_refund_address: bitcoin::Address,
) -> Result<bob::state::State2> {
    let xmr = event_loop_handle.request_spot_price(btc).await?;

    tracing::info!("Spot price for {} is {}", btc, xmr);

    let state0 = State0::new(
        &mut OsRng,
        btc,
        xmr,
        execution_params.bitcoin_cancel_timelock,
        execution_params.bitcoin_punish_timelock,
        bitcoin_refund_address,
        execution_params.monero_finality_confirmations,
    );

    let state2 = event_loop_handle.execution_setup(state0).await?;

    Ok(state2)
}
