use crate::bitcoin::{ExpiredTimelocks, TxCancel, TxRefund};
use crate::cli::EventLoopHandle;
use crate::database::Swap;
use crate::network::swap_setup::bob::NewSwap;
use crate::protocol::bob;
use crate::protocol::bob::state::*;
use crate::{bitcoin, monero};
use anyhow::{bail, Context, Result};
use tokio::select;
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
    mut swap: bob::Swap,
    is_target_state: fn(&BobState) -> bool,
) -> Result<BobState> {
    let mut current_state = swap.state;

    while !is_target_state(&current_state) {
        current_state = next_state(
            swap.id,
            current_state,
            &mut swap.event_loop_handle,
            swap.bitcoin_wallet.as_ref(),
            swap.monero_wallet.as_ref(),
            swap.monero_receive_address,
        )
        .await?;

        let db_state = current_state.clone().into();
        swap.db
            .insert_latest_state(swap.id, Swap::Bob(db_state))
            .await?;
    }

    Ok(current_state)
}

async fn next_state(
    swap_id: Uuid,
    state: BobState,
    event_loop_handle: &mut EventLoopHandle,
    bitcoin_wallet: &bitcoin::Wallet,
    monero_wallet: &monero::Wallet,
    monero_receive_address: monero::Address,
) -> Result<BobState> {
    tracing::trace!(%state, "Advancing state");

    Ok(match state {
        BobState::Started { btc_amount } => {
            let bitcoin_refund_address = bitcoin_wallet.new_address().await?;
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
                    bitcoin_refund_address,
                })
                .await?;

            BobState::SwapSetupCompleted(state2)
        }
        BobState::SwapSetupCompleted(state2) => {
            // Alice and Bob have exchanged info
            let (state3, tx_lock) = state2.lock_btc().await?;
            let signed_tx = bitcoin_wallet
                .sign_and_finalize(tx_lock.clone().into())
                .await
                .context("Failed to sign Bitcoin lock transaction")?;
            let (..) = bitcoin_wallet.broadcast(signed_tx, "lock").await?;

            BobState::BtcLocked(state3)
        }
        // Bob has locked Btc
        // Watch for Alice to Lock Xmr or for cancel timelock to elapse
        BobState::BtcLocked(state3) => {
            let tx_lock_status = bitcoin_wallet.subscribe_to(state3.tx_lock.clone()).await;

            if let ExpiredTimelocks::None = state3.current_epoch(bitcoin_wallet).await? {
                let transfer_proof_watcher = event_loop_handle.recv_transfer_proof();
                let cancel_timelock_expires =
                    tx_lock_status.wait_until_confirmed_with(state3.cancel_timelock);

                // Record the current monero wallet block height so we don't have to scan from
                // block 0 once we create the redeem wallet.
                let monero_wallet_restore_blockheight = monero_wallet.block_height().await?;

                tracing::info!("Waiting for Alice to lock Monero");

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
                    _ = cancel_timelock_expires => {
                        tracing::info!("Alice took too long to lock Monero, cancelling the swap");

                        let state4 = state3.cancel();
                        BobState::CancelTimelockExpired(state4)
                    }
                }
            } else {
                let state4 = state3.cancel();
                BobState::CancelTimelockExpired(state4)
            }
        }
        BobState::XmrLockProofReceived {
            state,
            lock_transfer_proof,
            monero_wallet_restore_blockheight,
        } => {
            let tx_lock_status = bitcoin_wallet.subscribe_to(state.tx_lock.clone()).await;

            if let ExpiredTimelocks::None = state.current_epoch(bitcoin_wallet).await? {
                let watch_request = state.lock_xmr_watch_request(lock_transfer_proof);

                select! {
                    received_xmr = monero_wallet.watch_for_transfer(watch_request) => {
                        match received_xmr {
                            Ok(()) => BobState::XmrLocked(state.xmr_locked(monero_wallet_restore_blockheight)),
                            Err(e) => {
                                 tracing::warn!("Waiting for refund because insufficient Monero have been locked! {:#}", e);
                                 tx_lock_status.wait_until_confirmed_with(state.cancel_timelock).await?;

                                 BobState::CancelTimelockExpired(state.cancel())
                            },
                        }
                    }
                    _ = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                        BobState::CancelTimelockExpired(state.cancel())
                    }
                }
            } else {
                BobState::CancelTimelockExpired(state.cancel())
            }
        }
        BobState::XmrLocked(state) => {
            let tx_lock_status = bitcoin_wallet.subscribe_to(state.tx_lock.clone()).await;

            if let ExpiredTimelocks::None = state.expired_timelock(bitcoin_wallet).await? {
                // Alice has locked Xmr
                // Bob sends Alice his key

                select! {
                    _ = event_loop_handle.send_encrypted_signature(state.tx_redeem_encsig()) => {
                        BobState::EncSigSent(state)
                    },
                    _ = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                        BobState::CancelTimelockExpired(state.cancel())
                    }
                }
            } else {
                BobState::CancelTimelockExpired(state.cancel())
            }
        }
        BobState::EncSigSent(state) => {
            let tx_lock_status = bitcoin_wallet.subscribe_to(state.tx_lock.clone()).await;

            if let ExpiredTimelocks::None = state.expired_timelock(bitcoin_wallet).await? {
                select! {
                    state5 = state.watch_for_redeem_btc(bitcoin_wallet) => {
                        BobState::BtcRedeemed(state5?)
                    },
                    _ = tx_lock_status.wait_until_confirmed_with(state.cancel_timelock) => {
                        BobState::CancelTimelockExpired(state.cancel())
                    }
                }
            } else {
                BobState::CancelTimelockExpired(state.cancel())
            }
        }
        BobState::BtcRedeemed(state) => {
            let (spend_key, view_key) = state.xmr_keys();

            let wallet_file_name = swap_id.to_string();
            if let Err(e) = monero_wallet
                .create_from_and_load(
                    wallet_file_name.clone(),
                    spend_key,
                    view_key,
                    state.monero_wallet_restore_blockheight,
                )
                .await
            {
                // In case we failed to refresh/sweep, when resuming the wallet might already
                // exist! This is a very unlikely scenario, but if we don't take care of it we
                // might not be able to ever transfer the Monero.
                tracing::warn!("Failed to generate monero wallet from keys: {:#}", e);
                tracing::info!(%wallet_file_name,
                    "Falling back to trying to open the the wallet if it already exists",
                );
                monero_wallet.open(wallet_file_name).await?;
            }

            // Ensure that the generated wallet is synced so we have a proper balance
            monero_wallet.refresh().await?;
            // Sweep (transfer all funds) to the given address
            let tx_hashes = monero_wallet.sweep_all(monero_receive_address).await?;

            for tx_hash in tx_hashes {
                tracing::info!(%monero_receive_address, txid=%tx_hash.0, "Sent XMR to");
            }

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
            // Bob has cancelled the swap
            match state.expired_timelock(bitcoin_wallet).await? {
                ExpiredTimelocks::None => {
                    bail!(
                        "Internal error: canceled state reached before cancel timelock was expired"
                    );
                }
                ExpiredTimelocks::Cancel => {
                    state.publish_refund_btc(bitcoin_wallet).await?;
                    BobState::BtcRefunded(state)
                }
                ExpiredTimelocks::Punish => BobState::BtcPunished {
                    tx_lock_id: state.tx_lock_id(),
                },
            }
        }
        BobState::BtcRefunded(state4) => BobState::BtcRefunded(state4),
        BobState::BtcPunished { tx_lock_id } => BobState::BtcPunished { tx_lock_id },
        BobState::SafelyAborted => BobState::SafelyAborted,
        BobState::XmrRedeemed { tx_lock_id } => BobState::XmrRedeemed { tx_lock_id },
    })
}
