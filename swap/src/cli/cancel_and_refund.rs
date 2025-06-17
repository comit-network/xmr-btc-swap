use crate::bitcoin::{ExpiredTimelocks, Wallet};
use crate::protocol::bob::BobState;
use crate::protocol::Database;
use anyhow::{bail, Result};
use bitcoin::Txid;
use std::sync::Arc;
use uuid::Uuid;

pub async fn cancel_and_refund(
    swap_id: Uuid,
    bitcoin_wallet: Arc<Wallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<BobState> {
    if let Err(err) = cancel(swap_id, bitcoin_wallet.clone(), db.clone()).await {
        tracing::warn!(%err, "Could not cancel swap. Attempting to refund anyway");
    };

    let state = match refund(swap_id, bitcoin_wallet, db).await {
        Ok(s) => s,
        Err(e) => bail!(e),
    };

    Ok(state)
}

pub async fn cancel(
    swap_id: Uuid,
    bitcoin_wallet: Arc<Wallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<(Txid, BobState)> {
    let state = db.get_state(swap_id).await?.try_into()?;

    let state6 = match state {
        BobState::BtcLocked {
            state3,
            monero_wallet_restore_blockheight,
        } => state3.cancel(monero_wallet_restore_blockheight),
        BobState::XmrLockProofReceived {
            state,
            monero_wallet_restore_blockheight,
            ..
        } => state.cancel(monero_wallet_restore_blockheight),
        BobState::XmrLocked(state4) => state4.cancel(),
        BobState::EncSigSent(state4) => state4.cancel(),
        BobState::CancelTimelockExpired(state6) => state6,
        BobState::BtcRefunded(state6) => state6,
        BobState::BtcCancelled(state6) => state6,
        BobState::BtcRefundPublished(state6) => state6,
        BobState::BtcEarlyRefundPublished(state6) => state6,

        BobState::Started { .. }
        | BobState::SwapSetupCompleted(_)
        | BobState::BtcRedeemed(_)
        | BobState::XmrRedeemed { .. }
        | BobState::BtcPunished { .. }
        | BobState::BtcEarlyRefunded { .. }
        | BobState::SafelyAborted => bail!(
            "Cannot cancel swap {} because it is in state {} which is not cancellable.",
            swap_id,
            state
        ),
    };

    tracing::info!(%swap_id, "Attempting to manually cancel swap");

    // Attempt to just publish the cancel transaction
    match state6.submit_tx_cancel(bitcoin_wallet.as_ref()).await {
        Ok((txid, _)) => {
            let state = BobState::BtcCancelled(state6);
            db.insert_latest_state(swap_id, state.clone().into())
                .await?;
            Ok((txid, state))
        }

        // If we fail to submit the cancel transaction it can have one of two reasons:
        // 1. The cancel timelock hasn't expired yet
        // 2. The cancel transaction has already been published by Alice
        Err(err) => {
            // Check if Alice has already published the cancel transaction while we were absent
            if let Some(tx) = state6.check_for_tx_cancel(bitcoin_wallet.as_ref()).await? {
                let state = BobState::BtcCancelled(state6);
                db.insert_latest_state(swap_id, state.clone().into())
                    .await?;
                tracing::info!("Alice has already cancelled the swap");

                return Ok((tx.compute_txid(), state));
            }

            // The cancel transaction has not been published yet and we failed to publish it ourselves
            // Here we try to figure out why
            match state6.expired_timelock(bitcoin_wallet.as_ref()).await {
                // We cannot cancel because Alice has already cancelled and punished afterwards
                Ok(ExpiredTimelocks::Punish { .. }) => {
                    let state = BobState::BtcPunished {
                        state: state6.clone(),
                        tx_lock_id: state6.tx_lock_id(),
                    };
                    db.insert_latest_state(swap_id, state.clone().into())
                        .await?;
                    tracing::info!("You have been punished for not refunding in time");
                    bail!(err.context("Cannot cancel swap because we have already been punished"));
                }
                // We cannot cancel because the cancel timelock has not expired yet
                Ok(ExpiredTimelocks::None { blocks_left }) => {
                    bail!(err.context(
                        format!(
                            "Cannot cancel swap because the cancel timelock has not expired yet. Blocks left: {}",
                            blocks_left
                        )
                    ));
                }
                Ok(ExpiredTimelocks::Cancel { .. }) => {
                    bail!(err.context("Failed to cancel swap even though cancel timelock has expired. This is unexpected."));
                }
                Err(timelock_err) => {
                    bail!(err
                        .context(timelock_err)
                        .context("Failed to cancel swap and could not check timelock status"));
                }
            }
        }
    }
}

pub async fn refund(
    swap_id: Uuid,
    bitcoin_wallet: Arc<Wallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<BobState> {
    let state = db.get_state(swap_id).await?.try_into()?;

    let state6 = match state {
        BobState::BtcLocked {
            state3,
            monero_wallet_restore_blockheight,
        } => state3.cancel(monero_wallet_restore_blockheight),
        BobState::XmrLockProofReceived {
            state,
            monero_wallet_restore_blockheight,
            ..
        } => state.cancel(monero_wallet_restore_blockheight),
        BobState::XmrLocked(state4) => state4.cancel(),
        BobState::EncSigSent(state4) => state4.cancel(),
        BobState::CancelTimelockExpired(state6) => state6,
        BobState::BtcCancelled(state6) => state6,
        BobState::BtcRefunded(state6) => state6,
        BobState::BtcRefundPublished(state6) => state6,
        BobState::BtcEarlyRefundPublished(state6) => state6,
        BobState::Started { .. }
        | BobState::SwapSetupCompleted(_)
        | BobState::BtcRedeemed(_)
        | BobState::BtcEarlyRefunded { .. }
        | BobState::XmrRedeemed { .. }
        | BobState::BtcPunished { .. }
        | BobState::SafelyAborted => bail!(
            "Cannot refund swap {} because it is in state {} which is not refundable.",
            swap_id,
            state
        ),
    };

    tracing::info!(%swap_id, "Attempting to manually refund swap");

    // Attempt to just publish the refund transaction
    match state6.publish_refund_btc(bitcoin_wallet.as_ref()).await {
        Ok(_) => {
            let state = BobState::BtcRefunded(state6);
            db.insert_latest_state(swap_id, state.clone().into())
                .await?;

            Ok(state)
        }

        // If we fail to submit the refund transaction it can have one of two reasons:
        // 1. The cancel transaction has not been published yet
        // 2. The refund timelock has already expired and we have been punished
        Err(bitcoin_publication_err) => {
            match state6.expired_timelock(bitcoin_wallet.as_ref()).await {
                // We have been punished
                Ok(ExpiredTimelocks::Punish { .. }) => {
                    let state = BobState::BtcPunished {
                        state: state6.clone(),
                        tx_lock_id: state6.tx_lock_id(),
                    };
                    db.insert_latest_state(swap_id, state.clone().into())
                        .await?;
                    tracing::info!("You have been punished for not refunding in time");
                    bail!(bitcoin_publication_err
                        .context("Cannot refund swap because we have already been punished"));
                }
                Ok(ExpiredTimelocks::None { blocks_left }) => {
                    bail!(
                        bitcoin_publication_err.context(format!(
                            "Cannot refund swap because the cancel timelock has not expired yet. Blocks left: {}",
                            blocks_left
                        ))
                    );
                }
                Ok(ExpiredTimelocks::Cancel { .. }) => {
                    bail!(bitcoin_publication_err.context("Failed to refund swap even though cancel timelock has expired. This is unexpected."));
                }
                Err(e) => {
                    bail!(bitcoin_publication_err
                        .context(e)
                        .context("Failed to refund swap and could not check timelock status"));
                }
            }
        }
    }
}
