use crate::{
    bitcoin,
    bitcoin::timelocks::ExpiredTimelocks,
    database::{Database, Swap},
    execution_params::ExecutionParams,
    monero,
    protocol::{
        bob::{self, event_loop::EventLoopHandle, state::*, SwapRequest},
        SwapAmounts,
    },
};
use anyhow::{bail, Result};
use async_recursion::async_recursion;
use std::sync::Arc;
use tokio::select;
use tracing::info;
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

#[tracing::instrument(name = "swap", skip(swap,is_target_state), fields(id = %swap.swap_id))]
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
) -> Result<BobState> {
    info!("Current state: {}", state);
    if is_target_state(&state) {
        Ok(state)
    } else {
        match state {
            BobState::Started { state0, amounts } => {
                event_loop_handle.dial().await?;

                let state2 = negotiate(
                    state0,
                    amounts,
                    &mut event_loop_handle,
                    bitcoin_wallet.clone(),
                )
                .await?;

                let state = BobState::Negotiated(state2);
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
                )
                .await
            }
            BobState::Negotiated(state2) => {
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
                    // TODO: This can be optimized further by extracting the block height when
                    //  tx-lock was included. However, scanning a few more blocks won't do any harm
                    //  and is simpler.
                    let monero_wallet_restore_blockheight =
                        monero_wallet.inner.block_height().await?;

                    select! {
                        transfer_proof = transfer_proof_watcher => {
                            let transfer_proof = transfer_proof?;
                            BobState::XmrLockProofReceived {
                                state: state3,
                                lock_transfer_proof: transfer_proof.tx_lock_proof,
                                monero_wallet_restore_blockheight
                            }
                        },
                        _ = cancel_timelock_expires => {
                            let state4 = state3.state4();
                            BobState::CancelTimelockExpired(state4)
                        }
                    }
                } else {
                    let state4 = state3.state4();
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
                        monero_wallet_restore_blockheight.height,
                    );
                    let cancel_timelock_expires =
                        state.wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref());

                    select! {
                        state4 = xmr_lock_watcher => {
                            BobState::XmrLocked(state4?)
                        },
                        _ = cancel_timelock_expires => {
                            let state4 = state.state4();
                            BobState::CancelTimelockExpired(state4)
                        }
                    }
                } else {
                    let state4 = state.state4();
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
                )
                .await
            }
            BobState::BtcRedeemed(state) => {
                // Bob redeems XMR using revealed s_a
                state.claim_xmr(monero_wallet.as_ref()).await?;

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

pub async fn negotiate(
    state0: crate::protocol::bob::state::State0,
    amounts: SwapAmounts,
    event_loop_handle: &mut EventLoopHandle,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
) -> Result<bob::state::State2> {
    tracing::trace!("Starting negotiate");
    event_loop_handle
        .send_swap_request(SwapRequest {
            btc_amount: amounts.btc,
        })
        .await?;

    // TODO: Use this once Bob's CLI is modified to only pass xmr amount in
    // argument.
    let _swap_response = event_loop_handle.recv_swap_response().await?;

    event_loop_handle
        .send_message0(state0.next_message())
        .await?;
    let msg0 = event_loop_handle.recv_message0().await?;
    let state1 = state0.receive(bitcoin_wallet.as_ref(), msg0).await?;

    event_loop_handle
        .send_message1(state1.next_message())
        .await?;
    let msg1 = event_loop_handle.recv_message1().await?;
    let state2 = state1.receive(msg1)?;

    event_loop_handle
        .send_message2(state2.next_message())
        .await?;

    Ok(state2)
}
