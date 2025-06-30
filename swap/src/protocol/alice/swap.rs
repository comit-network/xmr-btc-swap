//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use crate::asb::{EventLoopHandle, LatestRate};
use crate::bitcoin::ExpiredTimelocks;
use crate::common::retry;
use crate::env::Config;
use crate::monero::TransferProof;
use crate::protocol::alice::{AliceState, Swap};
use crate::{bitcoin, monero};
use ::bitcoin::consensus::encode::serialize_hex;
use anyhow::{bail, Context, Result};
use tokio::select;
use tokio::time::timeout;
use uuid::Uuid;

pub async fn run<LR>(swap: Swap, rate_service: LR) -> Result<AliceState>
where
    LR: LatestRate + Clone,
{
    run_until(swap, |_| false, rate_service).await
}

#[tracing::instrument(name = "swap", skip(swap,exit_early,rate_service), fields(id = %swap.swap_id), err)]
pub async fn run_until<LR>(
    mut swap: Swap,
    exit_early: fn(&AliceState) -> bool,
    rate_service: LR,
) -> Result<AliceState>
where
    LR: LatestRate + Clone,
{
    let mut current_state = swap.state;

    while !is_complete(&current_state) && !exit_early(&current_state) {
        current_state = next_state(
            swap.swap_id,
            current_state,
            &mut swap.event_loop_handle,
            swap.bitcoin_wallet.as_ref(),
            swap.monero_wallet.clone(),
            &swap.env_config,
            rate_service.clone(),
        )
        .await?;

        swap.db
            .insert_latest_state(swap.swap_id, current_state.clone().into())
            .await?;
    }

    Ok(current_state)
}

async fn next_state<LR>(
    swap_id: Uuid,
    state: AliceState,
    event_loop_handle: &mut EventLoopHandle,
    bitcoin_wallet: &bitcoin::Wallet,
    monero_wallet: Arc<monero::Wallets>,
    env_config: &Config,
    mut rate_service: LR,
) -> Result<AliceState>
where
    LR: LatestRate,
{
    let rate = rate_service
        .latest_rate()
        .map_or("NaN".to_string(), |rate| format!("{}", rate));

    tracing::info!(%state, %rate, "Advancing state");

    Ok(match state {
        AliceState::Started { state3 } => {
            let tx_lock_status = bitcoin_wallet.subscribe_to(state3.tx_lock.clone()).await;

            match timeout(
                env_config.bitcoin_lock_mempool_timeout,
                tx_lock_status.wait_until_seen(),
            )
            .await
            {
                Err(_) => {
                    tracing::info!(
                        minutes = %env_config.bitcoin_lock_mempool_timeout.as_secs_f64() / 60.0,
                        "TxLock lock was not seen in mempool in time. Alice might have denied our offer.",
                    );
                    AliceState::SafelyAborted
                }
                Ok(res) => {
                    res?;
                    AliceState::BtcLockTransactionSeen { state3 }
                }
            }
        }
        AliceState::BtcLockTransactionSeen { state3 } => {
            let tx_lock_status = bitcoin_wallet.subscribe_to(state3.tx_lock.clone()).await;

            match timeout(
                env_config.bitcoin_lock_confirmed_timeout,
                tx_lock_status.wait_until_final(),
            )
            .await
            {
                Err(_) => {
                    tracing::info!(
                        confirmations_needed = %env_config.bitcoin_finality_confirmations,
                        minutes = %env_config.bitcoin_lock_confirmed_timeout.as_secs_f64() / 60.0,
                        "TxLock lock did not get enough confirmations in time",
                    );

                    AliceState::BtcEarlyRefundable { state3 }
                }
                Ok(res) => {
                    res?;
                    AliceState::BtcLocked { state3 }
                }
            }
        }
        AliceState::BtcLocked { state3 } => {
            // Sometimes locking the Monero can fail e.g due to the daemon not being fully synced
            // We will retry indefinitely to lock the Monero funds, until either:
            // - the cancel timelock expires
            // - we do not manage to lock the Monero funds within the timeout
            let backoff = backoff::ExponentialBackoffBuilder::new()
                .with_max_elapsed_time(Some(env_config.monero_lock_retry_timeout))
                .with_max_interval(Duration::from_secs(30))
                .build();

            let transfer_proof = backoff::future::retry_notify(
                backoff,
                || async {
                    // We check the status of the Bitcoin lock transaction
                    // If the swap is cancelled, there is no need to lock the Monero funds anymore
                    // because there is no way for the swap to succeed.
                    if !matches!(
                        state3
                            .expired_timelocks(bitcoin_wallet)
                            .await
                            .context("Failed to check for expired timelocks before locking Monero")
                            .map_err(backoff::Error::transient)?,
                        ExpiredTimelocks::None { .. }
                    ) {
                        return Ok(None);
                    }

                    // Record the current monero wallet block height so we don't have to scan from
                    // block 0 for scenarios where we create a refund wallet.
                    let monero_wallet_restore_blockheight = monero_wallet
                        .blockchain_height()
                        .await
                        .context("Failed to get Monero wallet block height")
                        .map_err(backoff::Error::transient)?;

                    let (address, amount) = state3
                        .lock_xmr_transfer_request()
                        .address_and_amount(env_config.monero_network);

                    // Lock the Monero
                    let receipt = monero_wallet
                        .main_wallet()
                        .await
                        .transfer(&address, amount)
                        .await
                        .map_err(|e| tracing::error!(err=%e, "Failed to lock Monero"))
                        .ok();

                    let Some(receipt) = receipt else {
                        return Err(backoff::Error::transient(anyhow::anyhow!(
                            "Failed to lock Monero"
                        )));
                    };

                    Ok(Some((
                        monero_wallet_restore_blockheight,
                        TransferProof::new(
                            monero::TxHash(receipt.txid),
                            monero::PrivateKey::from_str(&receipt.tx_key)
                                .expect("tx key to be valid private key"),
                        ),
                    )))
                },
                |e, wait_time: Duration| {
                    tracing::warn!(
                        swap_id = %swap_id,
                        error = ?e,
                        "Failed to lock Monero. We will retry in {} seconds",
                        wait_time.as_secs()
                    )
                },
            )
            .await;

            match transfer_proof {
                // If the transfer was successful, we transition to the next state
                Ok(Some((monero_wallet_restore_blockheight, transfer_proof))) => {
                    AliceState::XmrLockTransactionSent {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        state3,
                    }
                }
                // If we were not able to lock the Monero funds before the timelock expired,
                // we can safely abort the swap because we did not lock any funds
                // We do not do an early refund because Bob can refund himself (timelock expired)
                Ok(None) => {
                    tracing::info!(
                        swap_id = %swap_id,
                        "We did not manage to lock the Monero funds before the timelock expired. Aborting swap."
                    );

                    AliceState::SafelyAborted
                }
                Err(e) => {
                    tracing::error!(
                        swap_id = %swap_id,
                        error = ?e,
                        "Failed to lock Monero within {} seconds. We will do an early refund of the Bitcoin. We didn't lock any Monero funds so this is safe.",
                        env_config.monero_lock_retry_timeout.as_secs()
                    );

                    AliceState::BtcEarlyRefundable { state3 }
                }
            }
        }
        AliceState::BtcEarlyRefundable { state3 } => {
            if let Some(tx_early_refund) = state3.signed_early_refund_transaction() {
                let tx_early_refund = tx_early_refund?;
                let tx_early_refund_txid = tx_early_refund.compute_txid();

                // Bob might cancel the swap and refund for himself. We won't need to early refund anymore.
                let tx_cancel_status = bitcoin_wallet.subscribe_to(state3.tx_cancel()).await;

                let backoff = backoff::ExponentialBackoffBuilder::new()
                    // We give up after 6 hours
                    // (Most likely Bob the a Replace-by-Fee on the tx_lock transaction)
                    .with_max_elapsed_time(Some(Duration::from_secs(6 * 60 * 60)))
                    // We wait a while between retries
                    .with_max_interval(Duration::from_secs(10 * 60))
                    .build();

                // Concurrently retry to broadcast the early refund transaction
                // and wait for the cancel transaction to be broadcasted.
                tokio::select! {
                    // If Bob cancels the swap, he can refund himself.
                    // Nothing for us to do anymore.
                    result = tx_cancel_status.wait_until_seen() => {
                        result?;
                        AliceState::SafelyAborted
                    }

                    // Retry repeatedly to broadcast tx_early_refund
                    result = async {
                        backoff::future::retry_notify(backoff, || async {
                            bitcoin_wallet.broadcast(tx_early_refund.clone(), "early_refund").await.map_err(backoff::Error::transient)
                        }, |e, wait_time: Duration| {
                            tracing::warn!(
                                %tx_early_refund_txid,
                                error = ?e,
                                "Failed to broadcast early refund transaction. We will retry in {} seconds",
                                wait_time.as_secs()
                            )
                        })
                        .await
                    } => {
                        match result {
                            Ok((_txid, _subscription)) => {
                                tracing::info!(
                                    %tx_early_refund_txid,
                                    "Refunded Bitcoin early for Bob"
                                );

                                AliceState::BtcEarlyRefunded(state3)
                            }
                            Err(e) => {
                                tracing::error!(
                                    %tx_early_refund_txid,
                                    error = ?e,
                                    "Failed to broadcast early refund transaction after retries exhausted. Bob will have to wait for the timelock to expire then refund himself."
                                );
                                AliceState::SafelyAborted
                            }
                        }
                    }
                }
            } else {
                // We do not have Bob's signature for the early refund transaction
                // Therefore we cannot do an early refund.
                // We abort the swap on our side.
                // Bob will have to wait for the timelock to expire then refund himself.
                AliceState::SafelyAborted
            }
        }
        AliceState::XmrLockTransactionSent {
            monero_wallet_restore_blockheight,
            transfer_proof,
            state3,
        } => match state3.expired_timelocks(bitcoin_wallet).await? {
            ExpiredTimelocks::None { .. } => {
                tracing::info!("Locked Monero, waiting for confirmations");
                monero_wallet
                    .wait_until_confirmed(
                        state3.lock_xmr_watch_request(transfer_proof.clone(), 1),
                        Some(|(confirmations, target_confirmations)| {
                            tracing::debug!(
                                %confirmations,
                                %target_confirmations,
                                "Monero lock tx got new confirmation"
                            )
                        }),
                    )
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to wait until Monero transaction was confirmed ({})",
                            transfer_proof.tx_hash()
                        )
                    })?;

                AliceState::XmrLocked {
                    monero_wallet_restore_blockheight,
                    transfer_proof,
                    state3,
                }
            }
            _ => AliceState::CancelTimelockExpired {
                monero_wallet_restore_blockheight,
                transfer_proof,
                state3,
            },
        },
        AliceState::XmrLocked {
            monero_wallet_restore_blockheight,
            transfer_proof,
            state3,
        } => {
            let tx_lock_status = bitcoin_wallet.subscribe_to(state3.tx_lock.clone()).await;

            tokio::select! {
                result = event_loop_handle.send_transfer_proof(transfer_proof.clone()) => {
                   result?;

                   AliceState::XmrLockTransferProofSent {
                       monero_wallet_restore_blockheight,
                       transfer_proof,
                       state3,
                   }
                },
                // TODO: We should already listen for the encrypted signature here.
                //
                // If we send Bob the transfer proof, but for whatever reason we do not receive an acknoledgement from him
                // we would be stuck in this state forever until the timelock expires. By listening for the encrypted signature here we
                // can still proceed to the next state even if Bob does not respond with an acknoledgement.
                result = tx_lock_status.wait_until_confirmed_with(state3.cancel_timelock) => {
                    result?;
                    AliceState::CancelTimelockExpired {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        state3,
                    }
                }
            }
        }
        AliceState::XmrLockTransferProofSent {
            monero_wallet_restore_blockheight,
            transfer_proof,
            state3,
        } => {
            let tx_lock_status_subscription =
                bitcoin_wallet.subscribe_to(state3.tx_lock.clone()).await;

            select! {
                biased; // make sure the cancel timelock expiry future is polled first
                result = tx_lock_status_subscription.wait_until_confirmed_with(state3.cancel_timelock) => {
                    result?;
                    AliceState::CancelTimelockExpired {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        state3,
                    }
                }
                enc_sig = event_loop_handle.recv_encrypted_signature() => {
                    // Fetch the status as early as possible to update the internal cache of our Electurm client
                    // Prevents redundant network requests later on when we redeem the Bitcoin
                    let tx_lock_status = bitcoin_wallet.status_of_script(&state3.tx_lock.clone()).await?;

                    if tx_lock_status.is_confirmed_with(state3.cancel_timelock.half()) {
                        tx_lock_status_subscription.wait_until_confirmed_with(state3.cancel_timelock).await?;

                        return Ok(AliceState::CancelTimelockExpired {
                            monero_wallet_restore_blockheight,
                            transfer_proof,
                            state3,
                        })
                    }

                    tracing::info!("Received encrypted signature");

                    AliceState::EncSigLearned {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        encrypted_signature: Box::new(enc_sig?),
                        state3,
                    }
                }
            }
        }
        AliceState::EncSigLearned {
            monero_wallet_restore_blockheight,
            transfer_proof,
            encrypted_signature,
            state3,
        } => {
            // Try to sign the redeem transaction, otherwise wait for the cancel timelock to expire
            let tx_redeem = match state3.signed_redeem_transaction(*encrypted_signature) {
                Ok(tx_redeem) => tx_redeem,
                Err(error) => {
                    tracing::error!("Failed to construct redeem transaction: {:#}", error);
                    tracing::info!(
                        timelock = %state3.cancel_timelock,
                        "Waiting for cancellation timelock to expire",
                    );

                    let tx_lock_status = bitcoin_wallet.subscribe_to(state3.tx_lock.clone()).await;

                    tx_lock_status
                        .wait_until_confirmed_with(state3.cancel_timelock)
                        .await?;

                    return Ok(AliceState::CancelTimelockExpired {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        state3,
                    });
                }
            };

            // Retry indefinitely to publish the redeem transaction, until the cancel timelock expires
            // Publishing the redeem transaction might fail on the first try due to any number of reasons
            let backoff = backoff::ExponentialBackoffBuilder::new()
                .with_max_elapsed_time(None)
                .with_max_interval(Duration::from_secs(60))
                .build();

            match backoff::future::retry_notify(backoff.clone(), || async {
                // If the cancel timelock is expired, there is no need to try to publish the redeem transaction anymore
                if !matches!(
                    state3.expired_timelocks(bitcoin_wallet).await?,
                    ExpiredTimelocks::None { .. }
                ) {
                    return Ok(None);
                }

                bitcoin_wallet
                    .broadcast(tx_redeem.clone(), "redeem")
                    .await
                    .map(Some)
                    .map_err(backoff::Error::transient)
            }, |e, wait_time: Duration| {
                tracing::warn!(
                    swap_id = %swap_id,
                    error = ?e,
                    "Failed to broadcast Bitcoin redeem transaction. We will retry in {} seconds",
                    wait_time.as_secs()
                )
            })
            .await
            .expect("We should never run out of retries while publishing the Bitcoin redeem transaction")
            {
                // We successfully published the redeem transaction
                // We wait until we see the transaction in the mempool before transitioning to the next state
                Some((txid, subscription)) => match subscription.wait_until_seen().await {
                    Ok(_) => AliceState::BtcRedeemTransactionPublished { state3, transfer_proof },
                    Err(e) => {
                        // We extract the txid and the hex representation of the transaction
                        // this'll allow the user to manually re-publish the transaction
                        let tx_hex = serialize_hex(&tx_redeem);

                        bail!("Waiting for Bitcoin redeem transaction to be in mempool failed with {}! The redeem transaction was published, but it is not ensured that the transaction was included! You might be screwed. You can try to manually re-publish the transaction (TxID: {}, Tx Hex: {})", e, txid, tx_hex)
                    }
                },

                // Cancel timelock expired before we could publish the redeem transaction
                None => {
                    tracing::error!("We were unable to publish the redeem transaction before the timelock expired.");

                    AliceState::CancelTimelockExpired {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        state3,
                    }
                }
            }
        }
        AliceState::BtcRedeemTransactionPublished { state3, .. } => {
            let subscription = bitcoin_wallet.subscribe_to(state3.tx_redeem()).await;

            match subscription.wait_until_final().await {
                Ok(_) => AliceState::BtcRedeemed,
                Err(e) => {
                    bail!("The Bitcoin redeem transaction was seen in mempool, but waiting for finality timed out with {}. Manual investigation might be needed to ensure that the transaction was included.", e)
                }
            }
        }
        AliceState::CancelTimelockExpired {
            monero_wallet_restore_blockheight,
            transfer_proof,
            state3,
        } => {
            if state3.check_for_tx_cancel(bitcoin_wallet).await?.is_none() {
                // If Bob hasn't yet broadcasted the cancel transaction, Alice has to publish it
                // to be able to eventually punish. Since the punish timelock is
                // relative to the publication of the cancel transaction we have to ensure it
                // gets published once the cancel timelock expires.

                if let Err(e) = state3.submit_tx_cancel(bitcoin_wallet).await {
                    // TODO: Actually ensure the transaction is published
                    // What about a wrapper function ensure_tx_published that repeats the tx submission until
                    // our subscription sees it in the mempool?

                    tracing::debug!(
                        "Assuming cancel transaction is already broadcasted because we failed to publish: {:#}",
                        e
                    )
                }
            }

            AliceState::BtcCancelled {
                monero_wallet_restore_blockheight,
                transfer_proof,
                state3,
            }
        }
        AliceState::BtcCancelled {
            monero_wallet_restore_blockheight,
            transfer_proof,
            state3,
        } => {
            let tx_cancel_status = bitcoin_wallet.subscribe_to(state3.tx_cancel()).await;

            select! {
                spend_key = state3.watch_for_btc_tx_refund(bitcoin_wallet) => {
                    let spend_key = spend_key?;

                    AliceState::BtcRefunded {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        spend_key,
                        state3,
                    }
                }
                result = tx_cancel_status.wait_until_confirmed_with(state3.punish_timelock) => {
                    result?;

                    AliceState::BtcPunishable {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        state3,
                    }
                }
            }
        }
        AliceState::BtcRefunded {
            transfer_proof,
            spend_key,
            state3,
            ..
        } => {
            retry(
                "Refund Monero",
                || async {
                    state3
                        .refund_xmr(
                            monero_wallet.clone(),
                            swap_id,
                            spend_key,
                            transfer_proof.clone(),
                        )
                        .await
                        .map_err(backoff::Error::transient)
                },
                None,
                Duration::from_secs(60),
            )
            .await
            .expect("We should never run out of retries while refunding Monero");

            AliceState::XmrRefunded
        }
        AliceState::BtcPunishable {
            monero_wallet_restore_blockheight,
            transfer_proof,
            state3,
        } => {
            // TODO: We should retry indefinitely here until we find the refund transaction
            // TODO: If we crash while we are waiting for the punish_tx to be confirmed (punish_btc waits until confirmation), we will remain in this state forever because we will attempt to re-publish the punish transaction
            let punish = state3.punish_btc(bitcoin_wallet).await;

            match punish {
                Ok(_) => AliceState::BtcPunished {
                    state3,
                    transfer_proof,
                },
                Err(error) => {
                    tracing::warn!("Failed to publish punish transaction: {:#}", error);

                    // Upon punish failure we assume that the refund tx was included but we
                    // missed seeing it. In case we fail to fetch the refund tx we fail
                    // with no state update because it is unclear what state we should transition
                    // to. It does not help to race punish and refund inclusion,
                    // because a punish tx failure is not recoverable (besides re-trying) if the
                    // refund tx was not included.

                    tracing::info!("Falling back to refund");

                    let published_refund_tx = bitcoin_wallet
                        .get_raw_transaction(state3.tx_refund().txid())
                        .await
                        .context("Failed to fetch refund transaction after assuming it was included because the punish transaction failed")?
                        .context("Bitcoin refund transaction not found")?;

                    let spend_key = state3.extract_monero_private_key(published_refund_tx)?;

                    AliceState::BtcRefunded {
                        monero_wallet_restore_blockheight,
                        transfer_proof,
                        spend_key,
                        state3,
                    }
                }
            }
        }
        AliceState::XmrRefunded => AliceState::XmrRefunded,
        AliceState::BtcRedeemed => AliceState::BtcRedeemed,
        AliceState::BtcPunished {
            state3,
            transfer_proof,
        } => AliceState::BtcPunished {
            state3,
            transfer_proof,
        },
        AliceState::BtcEarlyRefunded(state3) => AliceState::BtcEarlyRefunded(state3),
        AliceState::SafelyAborted => AliceState::SafelyAborted,
    })
}

pub fn is_complete(state: &AliceState) -> bool {
    matches!(
        state,
        AliceState::XmrRefunded
            | AliceState::BtcRedeemed
            | AliceState::BtcPunished { .. }
            | AliceState::SafelyAborted
            | AliceState::BtcEarlyRefunded(_)
    )
}

/// This function is used to check if Alice is in a state where it is clear that she has already received the encrypted signature from Bob.
/// This allows us to acknowledge the encrypted signature multiple times
/// If our acknowledgement does not reach Bob, he might send the encrypted signature again.
pub(crate) fn has_already_processed_enc_sig(state: &AliceState) -> bool {
    matches!(
        state,
        AliceState::EncSigLearned { .. }
            | AliceState::BtcRedeemTransactionPublished { .. }
            | AliceState::BtcRedeemed
    )
}
