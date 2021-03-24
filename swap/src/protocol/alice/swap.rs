//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::bitcoin::ExpiredTimelocks;
use crate::env::Config;
use crate::protocol::alice;
use crate::protocol::alice::event_loop::EventLoopHandle;
use crate::protocol::alice::AliceState;
use crate::{bitcoin, database, monero};
use anyhow::{bail, Context, Result};
use rand::{CryptoRng, RngCore};
use tokio::select;
use tokio::time::timeout;
use tracing::{error, info};

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
    mut swap: alice::Swap,
    is_target_state: fn(&AliceState) -> bool,
) -> Result<AliceState> {
    let mut current_state = swap.state;

    while !is_target_state(&current_state) {
        current_state = next_state(
            current_state,
            &mut swap.event_loop_handle,
            swap.bitcoin_wallet.as_ref(),
            swap.monero_wallet.as_ref(),
            &swap.env_config,
        )
        .await?;

        let db_state = (&current_state).into();
        swap.db
            .insert_latest_state(swap.swap_id, database::Swap::Alice(db_state))
            .await?;
    }

    Ok(current_state)
}

async fn next_state(
    state: AliceState,
    event_loop_handle: &mut EventLoopHandle,
    bitcoin_wallet: &bitcoin::Wallet,
    monero_wallet: &monero::Wallet,
    env_config: &Config,
) -> Result<AliceState> {
    info!("Current state: {}", state);

    Ok(match state {
        AliceState::Started { state3 } => {
            let tx_lock_status = bitcoin_wallet.subscribe_to(state3.tx_lock.clone()).await;
            match timeout(
                env_config.bitcoin_lock_confirmed_timeout,
                tx_lock_status.wait_until_final(),
            )
            .await
            {
                Err(_) => {
                    tracing::info!(
                        "TxLock lock did not get {} confirmations in {} minutes",
                        env_config.bitcoin_finality_confirmations,
                        env_config.bitcoin_lock_confirmed_timeout.as_secs_f64() / 60.0
                    );
                    AliceState::SafelyAborted
                }
                Ok(res) => {
                    res?;
                    AliceState::BtcLocked { state3 }
                }
            }
        }
        AliceState::BtcLocked { state3 } => {
            // Record the current monero wallet block height so we don't have to scan from
            // block 0 for scenarios where we create a refund wallet.
            let monero_wallet_restore_blockheight = monero_wallet.block_height().await?;

            let transfer_proof = monero_wallet
                .transfer(state3.lock_xmr_transfer_request())
                .await?;

            monero_wallet
                .watch_for_transfer(state3.lock_xmr_watch_request(transfer_proof.clone(), 1))
                .await?;

            // TODO: Waiting for XMR confirmations should be done in a separate
            //  state! We have to record that Alice has already sent the transaction.
            //  Otherwise Alice might publish the lock tx twice!

            event_loop_handle
                .send_transfer_proof(transfer_proof.clone())
                .await?;

            monero_wallet
                .watch_for_transfer(state3.lock_xmr_watch_request(transfer_proof, 10))
                .await?;

            AliceState::XmrLocked {
                state3,
                monero_wallet_restore_blockheight,
            }
        }
        AliceState::XmrLocked {
            state3,
            monero_wallet_restore_blockheight,
        } => {
            let tx_lock_status = bitcoin_wallet.subscribe_to(state3.tx_lock.clone()).await;

            match state3.expired_timelocks(bitcoin_wallet).await? {
                ExpiredTimelocks::None => {
                    select! {
                        _ = tx_lock_status.wait_until_confirmed_with(state3.cancel_timelock) => {
                            AliceState::CancelTimelockExpired {
                                state3,
                                monero_wallet_restore_blockheight,
                            }
                        }
                        enc_sig = event_loop_handle.recv_encrypted_signature() => {
                            tracing::info!("Received encrypted signature");

                            AliceState::EncSigLearned {
                                state3,
                                encrypted_signature: Box::new(enc_sig?),
                                monero_wallet_restore_blockheight,
                            }
                        }
                    }
                }
                _ => AliceState::CancelTimelockExpired {
                    state3,
                    monero_wallet_restore_blockheight,
                },
            }
        }
        AliceState::EncSigLearned {
            state3,
            encrypted_signature,
            monero_wallet_restore_blockheight,
        } => match state3.expired_timelocks(bitcoin_wallet).await? {
            ExpiredTimelocks::None => {
                let tx_lock_status = bitcoin_wallet.subscribe_to(state3.tx_lock.clone()).await;
                match state3.signed_redeem_transaction(*encrypted_signature) {
                    Ok(tx) => match bitcoin_wallet.broadcast(tx, "redeem").await {
                        Ok((_, subscription)) => match subscription.wait_until_final().await {
                            Ok(_) => AliceState::BtcRedeemed,
                            Err(e) => {
                                bail!("Waiting for Bitcoin transaction finality failed with {}! The redeem transaction was published, but it is not ensured that the transaction was included! You're screwed.", e)
                            }
                        },
                        Err(e) => {
                            error!("Publishing the redeem transaction failed with {}, attempting to wait for cancellation now. If you restart the application before the timelock is expired publishing the redeem transaction will be retried.", e);
                            tx_lock_status
                                .wait_until_confirmed_with(state3.cancel_timelock)
                                .await?;

                            AliceState::CancelTimelockExpired {
                                state3,
                                monero_wallet_restore_blockheight,
                            }
                        }
                    },
                    Err(e) => {
                        error!("Constructing the redeem transaction failed with {}, attempting to wait for cancellation now.", e);
                        tx_lock_status
                            .wait_until_confirmed_with(state3.cancel_timelock)
                            .await?;

                        AliceState::CancelTimelockExpired {
                            state3,
                            monero_wallet_restore_blockheight,
                        }
                    }
                }
            }
            _ => AliceState::CancelTimelockExpired {
                state3,
                monero_wallet_restore_blockheight,
            },
        },
        AliceState::CancelTimelockExpired {
            state3,
            monero_wallet_restore_blockheight,
        } => {
            let transaction = state3.signed_cancel_transaction()?;

            // If Bob hasn't yet broadcasted the tx cancel, we do it
            if bitcoin_wallet
                .get_raw_transaction(transaction.txid())
                .await
                .is_err()
            {
                if let Err(e) = bitcoin_wallet.broadcast(transaction, "cancel").await {
                    tracing::debug!(
                        "Assuming transaction is already broadcasted because: {:#}",
                        e
                    )
                }

                // TODO(Franck): Wait until transaction is mined and
                // returned mined block height
            }

            AliceState::BtcCancelled {
                state3,
                monero_wallet_restore_blockheight,
            }
        }
        AliceState::BtcCancelled {
            state3,
            monero_wallet_restore_blockheight,
        } => {
            let tx_refund_status = bitcoin_wallet.subscribe_to(state3.tx_refund()).await;
            let tx_cancel_status = bitcoin_wallet.subscribe_to(state3.tx_cancel()).await;

            select! {
                seen_refund = tx_refund_status.wait_until_seen() => {
                    seen_refund.context("Failed to monitor refund transaction")?;

                    let published_refund_tx = bitcoin_wallet.get_raw_transaction(state3.tx_refund().txid()).await?;
                    let spend_key = state3.extract_monero_private_key(published_refund_tx)?;

                    AliceState::BtcRefunded {
                        spend_key,
                        state3,
                        monero_wallet_restore_blockheight,
                    }
                }
                _ = tx_cancel_status.wait_until_confirmed_with(state3.punish_timelock) => {
                    AliceState::BtcPunishable {
                        state3,
                        monero_wallet_restore_blockheight,
                    }
                }
            }
        }
        AliceState::BtcRefunded {
            spend_key,
            state3,
            monero_wallet_restore_blockheight,
        } => {
            let view_key = state3.v;

            monero_wallet
                .create_from(spend_key, view_key, monero_wallet_restore_blockheight)
                .await?;

            AliceState::XmrRefunded
        }
        AliceState::BtcPunishable {
            state3,
            monero_wallet_restore_blockheight,
        } => {
            let signed_tx_punish = state3.signed_punish_transaction()?;

            let punish = async {
                let (txid, subscription) =
                    bitcoin_wallet.broadcast(signed_tx_punish, "punish").await?;
                subscription.wait_until_final().await?;

                Result::<_, anyhow::Error>::Ok(txid)
            }
            .await;

            match punish {
                Ok(_) => AliceState::BtcPunished,
                Err(e) => {
                    tracing::warn!(
                        "Falling back to refund because punish transaction failed with {:#}",
                        e
                    );

                    // Upon punish failure we assume that the refund tx was included but we
                    // missed seeing it. In case we fail to fetch the refund tx we fail
                    // with no state update because it is unclear what state we should transition
                    // to. It does not help to race punish and refund inclusion,
                    // because a punish tx failure is not recoverable (besides re-trying) if the
                    // refund tx was not included.

                    let published_refund_tx = bitcoin_wallet
                        .get_raw_transaction(state3.tx_refund().txid())
                        .await?;

                    let spend_key = state3.extract_monero_private_key(published_refund_tx)?;

                    AliceState::BtcRefunded {
                        spend_key,
                        state3,
                        monero_wallet_restore_blockheight,
                    }
                }
            }
        }
        AliceState::XmrRefunded => AliceState::XmrRefunded,
        AliceState::BtcRedeemed => AliceState::BtcRedeemed,
        AliceState::BtcPunished => AliceState::BtcPunished,
        AliceState::SafelyAborted => AliceState::SafelyAborted,
    })
}
