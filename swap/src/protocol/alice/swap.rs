//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::bitcoin::{ExpiredTimelocks, TxRedeem};
use crate::database::Database;
use crate::env::Config;
use crate::monero_ext::ScalarExt;
use crate::protocol::alice;
use crate::protocol::alice::event_loop::EventLoopHandle;
use crate::protocol::alice::AliceState;
use crate::{bitcoin, database, monero};
use anyhow::{bail, Context, Result};
use async_recursion::async_recursion;
use rand::{CryptoRng, RngCore};
use std::sync::Arc;
use tokio::select;
use tokio::time::timeout;
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
        swap.env_config,
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
    env_config: Config,
    swap_id: Uuid,
    db: Arc<Database>,
) -> Result<AliceState> {
    info!("Current state: {}", state);
    if is_target_state(&state) {
        return Ok(state);
    }

    let new_state = match state {
        AliceState::Started { state3 } => {
            timeout(
                env_config.bob_time_to_act,
                bitcoin_wallet.watch_until_status(&state3.tx_lock, |status| status.has_been_seen()),
            )
            .await
            .context("Failed to find lock Bitcoin tx")??;

            bitcoin_wallet
                .watch_until_status(&state3.tx_lock, |status| {
                    status.is_confirmed_with(env_config.bitcoin_finality_confirmations)
                })
                .await?;

            AliceState::BtcLocked { state3 }
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
        } => match state3.expired_timelocks(bitcoin_wallet.as_ref()).await? {
            ExpiredTimelocks::None => {
                select! {
                    _ = state3.wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref()) => {
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
        },
        AliceState::EncSigLearned {
            state3,
            encrypted_signature,
            monero_wallet_restore_blockheight,
        } => match state3.expired_timelocks(bitcoin_wallet.as_ref()).await? {
            ExpiredTimelocks::None => {
                match TxRedeem::new(&state3.tx_lock, &state3.redeem_address).complete(
                    *encrypted_signature,
                    state3.a.clone(),
                    state3.s_a.to_secpfun_scalar(),
                    state3.B,
                ) {
                    Ok(tx) => match bitcoin_wallet.broadcast(tx, "redeem").await {
                        Ok((_, finality)) => match finality.await {
                            Ok(_) => AliceState::BtcRedeemed,
                            Err(e) => {
                                bail!("Waiting for Bitcoin transaction finality failed with {}! The redeem transaction was published, but it is not ensured that the transaction was included! You're screwed.", e)
                            }
                        },
                        Err(e) => {
                            error!("Publishing the redeem transaction failed with {}, attempting to wait for cancellation now. If you restart the application before the timelock is expired publishing the redeem transaction will be retried.", e);
                            state3
                                .wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref())
                                .await?;

                            AliceState::CancelTimelockExpired {
                                state3,
                                monero_wallet_restore_blockheight,
                            }
                        }
                    },
                    Err(e) => {
                        error!("Constructing the redeem transaction failed with {}, attempting to wait for cancellation now.", e);
                        state3
                            .wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref())
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
            let tx_cancel = state3.tx_cancel();

            // If Bob hasn't yet broadcasted the tx cancel, we do it
            if bitcoin_wallet
                .get_raw_transaction(tx_cancel.txid())
                .await
                .is_err()
            {
                let transaction = tx_cancel
                    .complete_as_alice(state3.a.clone(), state3.B, state3.tx_cancel_sig_bob.clone())
                    .context("Failed to complete Bitcoin cancel transaction")?;

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
            let tx_refund = state3.tx_refund();
            let tx_cancel = state3.tx_cancel();

            let seen_refund_tx =
                bitcoin_wallet.watch_until_status(&tx_refund, |status| status.has_been_seen());

            let punish_timelock_expired = bitcoin_wallet.watch_until_status(&tx_cancel, |status| {
                status.is_confirmed_with(state3.punish_timelock)
            });

            select! {
                seen_refund = seen_refund_tx => {
                    seen_refund.context("Failed to monitor refund transaction")?;
                    let published_refund_tx = bitcoin_wallet.get_raw_transaction(tx_refund.txid()).await?;

                    let spend_key = tx_refund.extract_monero_private_key(
                        published_refund_tx,
                        state3.s_a,
                        state3.a.clone(),
                        state3.S_b_bitcoin,
                    )?;

                    AliceState::BtcRefunded {
                        spend_key,
                        state3,
                        monero_wallet_restore_blockheight,
                    }
                }
                _ = punish_timelock_expired => {
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
            let signed_tx_punish = state3.tx_punish().complete(
                state3.tx_punish_sig_bob.clone(),
                state3.a.clone(),
                state3.B,
            )?;

            let punish_tx_finalised = async {
                let (txid, finality) = bitcoin_wallet.broadcast(signed_tx_punish, "punish").await?;

                finality.await?;

                Result::<_, anyhow::Error>::Ok(txid)
            };

            let tx_refund = state3.tx_refund();
            let refund_tx_seen =
                bitcoin_wallet.watch_until_status(&tx_refund, |status| status.has_been_seen());

            select! {
                result = refund_tx_seen => {
                    result.context("Failed to monitor refund transaction")?;

                    let published_refund_tx =
                        bitcoin_wallet.get_raw_transaction(tx_refund.txid()).await?;

                    let spend_key = tx_refund.extract_monero_private_key(
                        published_refund_tx,
                        state3.s_a,
                        state3.a.clone(),
                        state3.S_b_bitcoin,
                    )?;
                    AliceState::BtcRefunded {
                        spend_key,
                        state3,
                        monero_wallet_restore_blockheight,
                    }
                }
                _ = punish_tx_finalised => {
                    AliceState::BtcPunished
                }
            }
        }
        AliceState::XmrRefunded => AliceState::XmrRefunded,
        AliceState::BtcRedeemed => AliceState::BtcRedeemed,
        AliceState::BtcPunished => AliceState::BtcPunished,
        AliceState::SafelyAborted => AliceState::SafelyAborted,
    };

    let db_state = (&new_state).into();
    db.insert_latest_state(swap_id, database::Swap::Alice(db_state))
        .await?;
    run_until_internal(
        new_state,
        is_target_state,
        event_loop_handle,
        bitcoin_wallet,
        monero_wallet,
        env_config,
        swap_id,
        db,
    )
    .await
}
