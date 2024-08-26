use crate::bitcoin::{ExpiredTimelocks, TxCancel, TxRefund};
use crate::cli::api::tauri_bindings::{TauriEmitter, TauriHandle, TauriSwapProgressEvent};
use crate::cli::EventLoopHandle;
use crate::network::cooperative_xmr_redeem_after_punish::Response::{Fullfilled, Rejected};
use crate::network::swap_setup::bob::NewSwap;
use crate::protocol::bob::state::*;
use crate::protocol::{bob, Database};
use crate::{bitcoin, monero};
use anyhow::{bail, Context, Result};
use std::sync::Arc;
use tokio::select;
use uuid::Uuid;

pub fn is_complete(state: &BobState) -> bool {
    matches!(
        state,
        BobState::BtcRefunded(..) | BobState::XmrRedeemed { .. } | BobState::SafelyAborted
    )
}

// Identifies states that should be run at most once before exiting.
// This is used to prevent infinite retry loops while still allowing manual resumption.
//
// Currently, this applies to the BtcPunished state:
// - We want to attempt recovery via cooperative XMR redeem once.
// - If unsuccessful, we exit to avoid an infinite retry loop.
// - The swap can still be manually resumed later and retried if desired.
pub fn is_run_at_most_once(state: &BobState) -> bool {
    matches!(state, BobState::BtcPunished { .. })
}

#[allow(clippy::too_many_arguments)]
pub async fn run(swap: bob::Swap) -> Result<BobState> {
    run_until(swap, is_complete).await
}

pub async fn run_until(
    mut swap: bob::Swap,
    is_target_state: fn(&BobState) -> bool,
) -> Result<BobState> {
    let mut current_state = swap.state.clone();

    while !is_target_state(&current_state) {
        let next_state = next_state(
            swap.id,
            current_state.clone(),
            &mut swap.event_loop_handle,
            swap.db.clone(),
            swap.bitcoin_wallet.as_ref(),
            swap.monero_wallet.as_ref(),
            swap.monero_receive_address,
            swap.event_emitter.clone(),
        )
        .await?;

        swap.db
            .insert_latest_state(swap.id, next_state.clone().into())
            .await?;

        if is_run_at_most_once(&current_state) && next_state == current_state {
            break;
        }

        current_state = next_state;
    }

    Ok(current_state)
}

#[allow(clippy::too_many_arguments)]
async fn next_state(
    swap_id: Uuid,
    state: BobState,
    event_loop_handle: &mut EventLoopHandle,
    db: Arc<dyn Database + Send + Sync>,
    bitcoin_wallet: &bitcoin::Wallet,
    monero_wallet: &monero::Wallet,
    monero_receive_address: monero::Address,
    event_emitter: Option<TauriHandle>,
) -> Result<BobState> {
    tracing::debug!(%state, "Advancing state");

    Ok(match state {
        BobState::Started {
            btc_amount,
            change_address,
        } => {
            let tx_refund_fee = bitcoin_wallet
                .estimate_fee(TxRefund::weight(), btc_amount)
                .await?;
            let tx_cancel_fee = bitcoin_wallet
                .estimate_fee(TxCancel::weight(), btc_amount)
                .await?;

            let state2 = event_loop_handle
                .setup_swap(NewSwap {
                    swap_id,
                    btc: btc_amount,
                    tx_refund_fee,
                    tx_cancel_fee,
                    bitcoin_refund_address: change_address,
                })
                .await?;

            tracing::info!(%swap_id, "Starting new swap");

            BobState::SwapSetupCompleted(state2)
        }
        BobState::SwapSetupCompleted(state2) => {
            // Record the current monero wallet block height so we don't have to scan from
            // block 0 once we create the redeem wallet.
            // This has to be done **before** the Bitcoin is locked in order to ensure that
            // if Bob goes offline the recorded wallet-height is correct.
            // If we only record this later, it can happen that Bob publishes the Bitcoin
            // transaction, goes offline, while offline Alice publishes Monero.
            // If the Monero transaction gets confirmed before Bob comes online again then
            // Bob would record a wallet-height that is past the lock transaction height,
            // which can lead to the wallet not detect the transaction.
            let monero_wallet_restore_blockheight = monero_wallet.block_height().await?;

            // Alice and Bob have exchanged info
            let (state3, tx_lock) = state2.lock_btc().await?;
            let signed_tx = bitcoin_wallet
                .sign_and_finalize(tx_lock.clone().into())
                .await
                .context("Failed to sign Bitcoin lock transaction")?;

            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::Started {
                    btc_lock_amount: tx_lock.lock_amount(),
                    // TODO: Replace this with the actual fee
                    btc_tx_lock_fee: bitcoin::Amount::ZERO,
                },
            );

            let (..) = bitcoin_wallet.broadcast(signed_tx, "lock").await?;

            BobState::BtcLocked {
                state3,
                monero_wallet_restore_blockheight,
            }
        }
        // Bob has locked Btc
        // Watch for Alice to Lock Xmr or for cancel timelock to elapse
        BobState::BtcLocked {
            state3,
            monero_wallet_restore_blockheight,
        } => {
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcLockTxInMempool {
                    btc_lock_txid: state3.tx_lock_id(),
                    // TODO: Replace this with the actual confirmations
                    btc_lock_confirmations: 0,
                },
            );

            let tx_lock_status = bitcoin_wallet.subscribe_to(state3.tx_lock.clone()).await;

            if let ExpiredTimelocks::None { .. } = state3.expired_timelock(bitcoin_wallet).await? {
                tracing::info!("Waiting for Alice to lock Monero");

                let buffered_transfer_proof = db
                    .get_buffered_transfer_proof(swap_id)
                    .await
                    .context("Failed to get buffered transfer proof")?;

                if let Some(transfer_proof) = buffered_transfer_proof {
                    tracing::debug!(txid = %transfer_proof.tx_hash(), "Found buffered transfer proof");
                    tracing::info!(txid = %transfer_proof.tx_hash(), "Alice locked Monero");

                    return Ok(BobState::XmrLockProofReceived {
                        state: state3,
                        lock_transfer_proof: transfer_proof,
                        monero_wallet_restore_blockheight,
                    });
                }

                let transfer_proof_watcher = event_loop_handle.recv_transfer_proof();
                let cancel_timelock_expires =
                    tx_lock_status.wait_until_confirmed_with(state3.cancel_timelock);

                select! {
                    transfer_proof = transfer_proof_watcher => {
                        let transfer_proof = transfer_proof?;

                        tracing::info!(txid = %transfer_proof.tx_hash(), "Alice locked Monero");

                        BobState::XmrLockProofReceived {
                            state: state3,
                            lock_transfer_proof: transfer_proof,
                            monero_wallet_restore_blockheight
                        }
                    },
                    result = cancel_timelock_expires => {
                        result?;
                        tracing::info!("Alice took too long to lock Monero, cancelling the swap");

                        let state4 = state3.cancel(monero_wallet_restore_blockheight);
                        BobState::CancelTimelockExpired(state4)
                    },
                }
            } else {
                let state4 = state3.cancel(monero_wallet_restore_blockheight);
                BobState::CancelTimelockExpired(state4)
            }
        }
        BobState::XmrLockProofReceived {
            state,
            lock_transfer_proof,
            monero_wallet_restore_blockheight,
        } => {
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::XmrLockTxInMempool {
                    xmr_lock_txid: lock_transfer_proof.tx_hash(),
                    xmr_lock_tx_confirmations: 0,
                },
            );

            let tx_lock_status = bitcoin_wallet.subscribe_to(state.tx_lock.clone()).await;

            if let ExpiredTimelocks::None { .. } = state.expired_timelock(bitcoin_wallet).await? {
                let watch_request = state.lock_xmr_watch_request(lock_transfer_proof);

                select! {
                    received_xmr = monero_wallet.watch_for_transfer(watch_request) => {
                        match received_xmr {
                            Ok(()) => BobState::XmrLocked(state.xmr_locked(monero_wallet_restore_blockheight)),
                            Err(monero::InsufficientFunds { expected, actual }) => {
                                tracing::warn!(%expected, %actual, "Insufficient Monero have been locked!");
                                tracing::info!(timelock = %state.cancel_timelock, "Waiting for cancel timelock to expire");

                                tx_lock_status.wait_until_confirmed_with(state.cancel_timelock).await?;

                                BobState::CancelTimelockExpired(state.cancel(monero_wallet_restore_blockheight))
                            },
                        }
                    }
                    // TODO: Send Tauri event here everytime we receive a new confirmation
                    result = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                        result?;
                        BobState::CancelTimelockExpired(state.cancel(monero_wallet_restore_blockheight))
                    }
                }
            } else {
                BobState::CancelTimelockExpired(state.cancel(monero_wallet_restore_blockheight))
            }
        }
        BobState::XmrLocked(state) => {
            event_emitter.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::XmrLocked);

            // In case we send the encrypted signature to Alice, but she doesn't give us a confirmation
            // We need to check if she still published the Bitcoin redeem transaction
            // Otherwise we risk staying stuck in "XmrLocked"
            if let Ok(state5) = state.check_for_tx_redeem(bitcoin_wallet).await {
                return Ok(BobState::BtcRedeemed(state5));
            }

            let tx_lock_status = bitcoin_wallet.subscribe_to(state.tx_lock.clone()).await;

            if let ExpiredTimelocks::None { .. } = state.expired_timelock(bitcoin_wallet).await? {
                // Alice has locked Xmr
                // Bob sends Alice his key

                select! {
                    result = event_loop_handle.send_encrypted_signature(state.tx_redeem_encsig()) => {
                        match result {
                            Ok(_) => BobState::EncSigSent(state),
                            Err(bmrng::error::RequestError::RecvError | bmrng::error::RequestError::SendError(_)) => bail!("Failed to communicate encrypted signature through event loop channel"),
                            Err(bmrng::error::RequestError::RecvTimeoutError) => unreachable!("We construct the channel with no timeout"),
                        }
                    },
                    result = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                        result?;
                        BobState::CancelTimelockExpired(state.cancel())
                    }
                }
            } else {
                BobState::CancelTimelockExpired(state.cancel())
            }
        }
        BobState::EncSigSent(state) => {
            // We need to make sure that Alice did not publish the redeem transaction while we were offline
            // Even if the cancel timelock expired, if Alice published the redeem transaction while we were away we cannot miss it
            // If we do we cannot refund and will never be able to leave the "CancelTimelockExpired" state
            if let Ok(state5) = state.check_for_tx_redeem(bitcoin_wallet).await {
                return Ok(BobState::BtcRedeemed(state5));
            }

            let tx_lock_status = bitcoin_wallet.subscribe_to(state.tx_lock.clone()).await;

            if let ExpiredTimelocks::None { .. } = state.expired_timelock(bitcoin_wallet).await? {
                select! {
                    state5 = state.watch_for_redeem_btc(bitcoin_wallet) => {
                        BobState::BtcRedeemed(state5?)
                    },
                    result = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                        result?;
                        BobState::CancelTimelockExpired(state.cancel())
                    }
                }
            } else {
                BobState::CancelTimelockExpired(state.cancel())
            }
        }
        BobState::BtcRedeemed(state) => {
            event_emitter.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::BtcRedeemed);

            state
                .redeem_xmr(monero_wallet, swap_id.to_string(), monero_receive_address)
                .await?;

            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::XmrRedeemInMempool {
                    // TODO: Replace this with the actual txid
                    xmr_redeem_txid: monero::TxHash("placeholder".to_string()),
                    xmr_redeem_address: monero_receive_address,
                },
            );

            BobState::XmrRedeemed {
                tx_lock_id: state.tx_lock_id(),
            }
        }
        BobState::CancelTimelockExpired(state4) => {
            if state4.check_for_tx_cancel(bitcoin_wallet).await.is_err() {
                state4.submit_tx_cancel(bitcoin_wallet).await?;
            }

            BobState::BtcCancelled(state4)
        }
        BobState::BtcCancelled(state) => {
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcCancelled {
                    btc_cancel_txid: state.construct_tx_cancel()?.txid(),
                },
            );

            // Bob has cancelled the swap
            match state.expired_timelock(bitcoin_wallet).await? {
                ExpiredTimelocks::None { .. } => {
                    bail!(
                        "Internal error: canceled state reached before cancel timelock was expired"
                    );
                }
                ExpiredTimelocks::Cancel { .. } => {
                    state.publish_refund_btc(bitcoin_wallet).await?;
                    BobState::BtcRefunded(state)
                }
                ExpiredTimelocks::Punish => {
                    tracing::info!("You have been punished for not refunding in time");
                    BobState::BtcPunished {
                        tx_lock_id: state.tx_lock_id(),
                        state,
                    }
                }
            }
        }
        BobState::BtcRefunded(state4) => {
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcRefunded {
                    btc_refund_txid: state4.signed_refund_transaction()?.txid(),
                },
            );

            BobState::BtcRefunded(state4)
        }
        BobState::BtcPunished { state, tx_lock_id } => {
            event_emitter.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::BtcPunished);

            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::AttemptingCooperativeRedeem,
            );

            tracing::info!("Attempting to cooperatively redeem XMR after being punished");
            let response = event_loop_handle
                .request_cooperative_xmr_redeem(swap_id)
                .await;

            match response {
                Ok(Fullfilled { s_a, .. }) => {
                    tracing::info!(
                        "Alice has accepted our request to cooperatively redeem the XMR"
                    );

                    event_emitter.emit_swap_progress_event(
                        swap_id,
                        TauriSwapProgressEvent::CooperativeRedeemAccepted,
                    );

                    let s_a = monero::PrivateKey { scalar: s_a };

                    let state5 = state.attempt_cooperative_redeem(s_a);

                    match state5
                        .redeem_xmr(monero_wallet, swap_id.to_string(), monero_receive_address)
                        .await
                    {
                        Ok(_) => {
                            event_emitter.emit_swap_progress_event(
                                swap_id,
                                TauriSwapProgressEvent::XmrRedeemInMempool {
                                    xmr_redeem_txid: monero::TxHash("placeholder".to_string()),
                                    xmr_redeem_address: monero_receive_address,
                                },
                            );

                            return Ok(BobState::XmrRedeemed { tx_lock_id });
                        }
                        Err(error) => {
                            event_emitter.emit_swap_progress_event(
                                swap_id,
                                TauriSwapProgressEvent::CooperativeRedeemRejected {
                                    reason: error.to_string(),
                                },
                            );

                            let err: std::result::Result<_, anyhow::Error> =
                                Err(error).context("Failed to redeem XMR with revealed XMR key");

                            return err;
                        }
                    }
                }
                Ok(Rejected { reason, .. }) => {
                    let err = Err(reason.clone())
                        .context("Alice rejected our request for cooperative XMR redeem");

                    event_emitter.emit_swap_progress_event(
                        swap_id,
                        TauriSwapProgressEvent::CooperativeRedeemRejected {
                            reason: reason.to_string(),
                        },
                    );

                    tracing::error!(
                        ?reason,
                        "Alice rejected our request for cooperative XMR redeem"
                    );

                    return err;
                }
                Err(error) => {
                    tracing::error!(
                        ?error,
                        "Failed to request cooperative XMR redeem from Alice"
                    );

                    event_emitter.emit_swap_progress_event(
                        swap_id,
                        TauriSwapProgressEvent::CooperativeRedeemRejected {
                            reason: error.to_string(),
                        },
                    );

                    return Err(error)
                        .context("Failed to request cooperative XMR redeem from Alice");
                }
            };
        }
        BobState::SafelyAborted => BobState::SafelyAborted,
        BobState::XmrRedeemed { tx_lock_id } => {
            // TODO: Replace this with the actual txid
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::XmrRedeemInMempool {
                    xmr_redeem_txid: monero::TxHash("placeholder".to_string()),
                    xmr_redeem_address: monero_receive_address,
                },
            );
            BobState::XmrRedeemed { tx_lock_id }
        }
    })
}
