use crate::bitcoin::wallet::ScriptStatus;
use crate::bitcoin::{ExpiredTimelocks, TxCancel, TxRefund};
use crate::cli::api::tauri_bindings::ApprovalRequestDetails;
use crate::cli::api::tauri_bindings::{
    LockBitcoinDetails, TauriEmitter, TauriHandle, TauriSwapProgressEvent,
};
use crate::cli::EventLoopHandle;
use crate::network::cooperative_xmr_redeem_after_punish::Response::{Fullfilled, Rejected};
use crate::network::swap_setup::bob::NewSwap;
use crate::protocol::bob::state::*;
use crate::protocol::{bob, Database};
use crate::{bitcoin, monero};
use anyhow::{bail, Context as AnyContext, Result};
use std::sync::Arc;
use tokio::select;
use uuid::Uuid;

const PRE_BTC_LOCK_APPROVAL_TIMEOUT_SECS: u64 = 120;

pub fn is_complete(state: &BobState) -> bool {
    matches!(
        state,
        BobState::BtcRefunded(..) | BobState::XmrRedeemed { .. } | BobState::SafelyAborted
    )
}

/// Identifies states that have already processed the transfer proof.
/// This is used to be able to acknowledge the transfer proof multiple times (if it was already processed).
/// This is necessary because sometimes our acknowledgement might not reach Alice.
pub fn has_already_processed_transfer_proof(state: &BobState) -> bool {
    // This match statement MUST match all states which Bob can enter after receiving the transfer proof.
    // We do not match any of the cancel / refund states because in those, the swap cannot be successfull anymore.
    matches!(
        state,
        BobState::XmrLockProofReceived { .. }
            | BobState::XmrLocked(..)
            | BobState::EncSigSent(..)
            | BobState::BtcRedeemed(..)
            | BobState::XmrRedeemed { .. }
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

            // Emit an event to tauri that we are negotiating with the maker to lock the Bitcoin
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::SwapSetupInflight {
                    btc_lock_amount: btc_amount,
                    // TODO: Replace this with the actual fee
                    btc_tx_lock_fee: bitcoin::Amount::ZERO,
                },
            );

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

            let xmr_receive_amount = state2.xmr;

            // Alice and Bob have exchanged info
            // Sign the Bitcoin lock transaction
            let (state3, tx_lock) = state2.lock_btc().await?;
            let signed_tx = bitcoin_wallet
                .sign_and_finalize(tx_lock.clone().into())
                .await
                .context("Failed to sign Bitcoin lock transaction")?;

            let btc_network_fee = tx_lock.fee().context("Failed to get fee")?;
            let btc_lock_amount = bitcoin::Amount::from_sat(
                signed_tx
                    .output
                    .get(0)
                    .context("Failed to get lock amount")?
                    .value,
            );

            let request = ApprovalRequestDetails::LockBitcoin(LockBitcoinDetails {
                btc_lock_amount,
                btc_network_fee,
                xmr_receive_amount,
                swap_id,
            });

            // We request approval before publishing the Bitcoin lock transaction, as the exchange rate determined at this step might be different from the
            // we previously displayed to the user.
            let approval_result = event_emitter
                .request_approval(request, PRE_BTC_LOCK_APPROVAL_TIMEOUT_SECS)
                .await;

            match approval_result {
                Ok(true) => {
                    tracing::debug!("User approved swap offer");

                    // Publish the signed Bitcoin lock transaction
                    let (..) = bitcoin_wallet.broadcast(signed_tx, "lock").await?;

                    BobState::BtcLocked {
                        state3,
                        monero_wallet_restore_blockheight,
                    }
                }
                Ok(false) => {
                    tracing::warn!("User denied or timed out on swap offer approval");

                    BobState::SafelyAborted
                }
                Err(err) => {
                    tracing::warn!(%err, "Failed to get user approval for swap offer. Assuming swap was aborted.");

                    BobState::SafelyAborted
                }
            }
        }
        // Bob has locked Bitcoin
        // Watch for Alice to lock Monero or for cancel timelock to elapse
        BobState::BtcLocked {
            state3,
            monero_wallet_restore_blockheight,
        } => {
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::BtcLockTxInMempool {
                    btc_lock_txid: state3.tx_lock_id(),
                    btc_lock_confirmations: 0,
                },
            );

            let tx_lock_status = bitcoin_wallet.subscribe_to(state3.tx_lock.clone()).await;

            // Check whether we can cancel the swap, and do so if possible
            if state3
                .expired_timelock(bitcoin_wallet)
                .await?
                .cancel_timelock_expired()
            {
                let state4 = state3.cancel(monero_wallet_restore_blockheight);
                return Ok(BobState::CancelTimelockExpired(state4));
            };

            tracing::info!("Waiting for Alice to lock Monero");

            // Check if we have already buffered the XMR transfer proof
            if let Some(transfer_proof) = db
                .get_buffered_transfer_proof(swap_id)
                .await
                .context("Failed to get buffered transfer proof")?
            {
                tracing::debug!(txid = %transfer_proof.tx_hash(), "Found buffered transfer proof");
                tracing::info!(txid = %transfer_proof.tx_hash(), "Alice locked Monero");

                return Ok(BobState::XmrLockProofReceived {
                    state: state3,
                    lock_transfer_proof: transfer_proof,
                    monero_wallet_restore_blockheight,
                });
            }

            // Wait for either Alice to send the XMR transfer proof or until we can cancel the swap
            let transfer_proof_watcher = event_loop_handle.recv_transfer_proof();
            let cancel_timelock_expires = tx_lock_status.wait_until(|status| {
                // Emit a tauri event on new confirmations
                if let ScriptStatus::Confirmed(confirmed) = status {
                    event_emitter.emit_swap_progress_event(
                        swap_id,
                        TauriSwapProgressEvent::BtcLockTxInMempool {
                            btc_lock_txid: state3.tx_lock_id(),
                            btc_lock_confirmations: u64::from(confirmed.confirmations()),
                        },
                    );
                }

                // Stop when the cancel timelock expires
                status.is_confirmed_with(state3.cancel_timelock)
            });

            select! {
                // Alice sent us the transfer proof for the Monero she locked
                transfer_proof = transfer_proof_watcher => {
                    let transfer_proof = transfer_proof?;

                    tracing::info!(txid = %transfer_proof.tx_hash(), "Alice locked Monero");

                    BobState::XmrLockProofReceived {
                        state: state3,
                        lock_transfer_proof: transfer_proof,
                        monero_wallet_restore_blockheight
                    }
                },
                // The cancel timelock expired before Alice locked her Monero
                result = cancel_timelock_expires => {
                    result?;
                    tracing::info!("Alice took too long to lock Monero, cancelling the swap");

                    let state4 = state3.cancel(monero_wallet_restore_blockheight);
                    BobState::CancelTimelockExpired(state4)
                },
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

            // Check if the cancel timelock has expired
            // If it has, we have to cancel the swap
            if state
                .expired_timelock(bitcoin_wallet)
                .await?
                .cancel_timelock_expired()
            {
                return Ok(BobState::CancelTimelockExpired(
                    state.cancel(monero_wallet_restore_blockheight),
                ));
            };

            // Clone these so that we can move them into the listener closure
            let tauri_clone = event_emitter.clone();
            let transfer_proof_clone = lock_transfer_proof.clone();
            let watch_request = state.lock_xmr_watch_request(lock_transfer_proof);

            // We pass a listener to the function that get's called everytime a new confirmation is spotted.
            let watch_future = monero_wallet.watch_for_transfer_with(
                watch_request,
                Some(Box::new(move |confirmations| {
                    // Clone them again so that we can move them again
                    let tranfer = transfer_proof_clone.clone();
                    let tauri = tauri_clone.clone();

                    // Emit an event to notify about the new confirmation
                    Box::pin(async move {
                        tauri.emit_swap_progress_event(
                            swap_id,
                            TauriSwapProgressEvent::XmrLockTxInMempool {
                                xmr_lock_txid: tranfer.tx_hash(),
                                xmr_lock_tx_confirmations: confirmations,
                            },
                        );
                    })
                })),
            );

            select! {
                received_xmr = watch_future => {
                    match received_xmr {
                        Ok(()) =>
                            BobState::XmrLocked(state.xmr_locked(monero_wallet_restore_blockheight)),
                        Err(monero::InsufficientFunds { expected, actual }) => {
                            // Alice locked insufficient Monero
                            tracing::warn!(%expected, %actual, "Insufficient Monero have been locked!");
                            tracing::info!(timelock = %state.cancel_timelock, "Waiting for cancel timelock to expire");

                            // We wait for the cancel timelock to expire before we cancel the swap
                            // because there's no way of recovering from this state
                            tx_lock_status.wait_until_confirmed_with(state.cancel_timelock).await?;

                            BobState::CancelTimelockExpired(state.cancel(monero_wallet_restore_blockheight))
                        },
                    }
                }
                result = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                    result?;
                    BobState::CancelTimelockExpired(state.cancel(monero_wallet_restore_blockheight))
                }
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

            // Check whether we can cancel the swap and do so if possible.
            if state
                .expired_timelock(bitcoin_wallet)
                .await?
                .cancel_timelock_expired()
            {
                return Ok(BobState::CancelTimelockExpired(state.cancel()));
            }

            // Alice has locked their Monero
            // Bob sends Alice the encrypted signature which allows her to sign and broadcast the Bitcoin redeem transaction
            select! {
                result = event_loop_handle.send_encrypted_signature(state.tx_redeem_encsig()) => {
                    match result {
                        Ok(_) => BobState::EncSigSent(state),
                        Err(err) => {
                            tracing::error!(%err, "Failed to send encrypted signature to Alice");
                            bail!("Failed to send encrypted signature to Alice");
                        }
                    }
                },
                result = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                    result?;
                    BobState::CancelTimelockExpired(state.cancel())
                }
            }
        }
        BobState::EncSigSent(state) => {
            event_emitter
                .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::EncryptedSignatureSent);

            // We need to make sure that Alice did not publish the redeem transaction while we were offline
            // Even if the cancel timelock expired, if Alice published the redeem transaction while we were away we cannot miss it
            // If we do we cannot refund and will never be able to leave the "CancelTimelockExpired" state
            if let Ok(state5) = state.check_for_tx_redeem(bitcoin_wallet).await {
                return Ok(BobState::BtcRedeemed(state5));
            }

            let tx_lock_status = bitcoin_wallet.subscribe_to(state.tx_lock.clone()).await;

            if state
                .expired_timelock(bitcoin_wallet)
                .await?
                .cancel_timelock_expired()
            {
                return Ok(BobState::CancelTimelockExpired(state.cancel()));
            }

            select! {
                state5 = state.watch_for_redeem_btc(bitcoin_wallet) => {
                    BobState::BtcRedeemed(state5?)
                },
                result = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                    result?;
                    BobState::CancelTimelockExpired(state.cancel())
                }
            }
        }
        BobState::BtcRedeemed(state) => {
            event_emitter.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::BtcRedeemed);

            let xmr_redeem_txids = state
                .redeem_xmr(monero_wallet, swap_id.to_string(), monero_receive_address)
                .await?;

            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::XmrRedeemInMempool {
                    xmr_redeem_txids,
                    xmr_redeem_address: monero_receive_address,
                },
            );

            BobState::XmrRedeemed {
                tx_lock_id: state.tx_lock_id(),
            }
        }
        BobState::CancelTimelockExpired(state4) => {
            event_emitter
                .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::CancelTimelockExpired);

            if let Err(err) = state4.check_for_tx_cancel(bitcoin_wallet).await {
                tracing::debug!(
                    %err,
                    "Couldn't find tx_cancel yet, publishing ourselves"
                );
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
                    let btc_refund_txid = state.publish_refund_btc(bitcoin_wallet).await?;

                    event_emitter.emit_swap_progress_event(
                        swap_id,
                        TauriSwapProgressEvent::BtcRefunded { btc_refund_txid },
                    );

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
            let response = event_loop_handle.request_cooperative_xmr_redeem().await;

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
                        Ok(xmr_redeem_txids) => {
                            event_emitter.emit_swap_progress_event(
                                swap_id,
                                TauriSwapProgressEvent::XmrRedeemInMempool {
                                    xmr_redeem_txids,
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
                        %reason,
                        "Alice rejected our request for cooperative XMR redeem"
                    );

                    return err;
                }
                Err(error) => {
                    tracing::error!(
                        %error,
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
        // TODO: Emit a Tauri event here
        BobState::SafelyAborted => BobState::SafelyAborted,
        BobState::XmrRedeemed { tx_lock_id } => {
            event_emitter.emit_swap_progress_event(
                swap_id,
                TauriSwapProgressEvent::XmrRedeemInMempool {
                    // We don't have the txids of the redeem transaction here, so we can't emit them
                    // We return an empty array instead
                    xmr_redeem_txids: vec![],
                    xmr_redeem_address: monero_receive_address,
                },
            );
            BobState::XmrRedeemed { tx_lock_id }
        }
    })
}
