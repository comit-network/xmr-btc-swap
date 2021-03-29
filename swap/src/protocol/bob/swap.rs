use crate::bitcoin::ExpiredTimelocks;
use crate::database::Swap;
use crate::env::Config;
use crate::protocol::bob;
use crate::protocol::bob::event_loop::EventLoopHandle;
use crate::protocol::bob::state::*;
use crate::{bitcoin, monero};
use anyhow::{bail, Context, Result};
use rand::rngs::OsRng;
use tokio::select;
use tracing::trace;

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
            current_state,
            &mut swap.event_loop_handle,
            swap.bitcoin_wallet.as_ref(),
            swap.monero_wallet.as_ref(),
            &swap.env_config,
            swap.receive_monero_address,
        )
        .await?;

        let db_state = current_state.clone().into();
        swap.db
            .insert_latest_state(swap.swap_id, Swap::Bob(db_state))
            .await?;
    }

    Ok(current_state)
}

async fn next_state(
    state: BobState,
    event_loop_handle: &mut EventLoopHandle,
    bitcoin_wallet: &bitcoin::Wallet,
    monero_wallet: &monero::Wallet,
    env_config: &Config,
    receive_monero_address: monero::Address,
) -> Result<BobState> {
    trace!("Current state: {}", state);

    Ok(match state {
        BobState::Started { btc_amount } => {
            let bitcoin_refund_address = bitcoin_wallet.new_address().await?;

            let state2 = request_price_and_setup(
                btc_amount,
                event_loop_handle,
                env_config,
                bitcoin_refund_address,
            )
            .await?;

            BobState::ExecutionSetupDone(state2)
        }
        BobState::ExecutionSetupDone(state2) => {
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
            if let ExpiredTimelocks::None = state3.current_epoch(bitcoin_wallet).await? {
                let transfer_proof_watcher = event_loop_handle.recv_transfer_proof();
                let cancel_timelock_expires =
                    state3.wait_for_cancel_timelock_to_expire(bitcoin_wallet);

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
            }
        }
        BobState::XmrLockProofReceived {
            state,
            lock_transfer_proof,
            monero_wallet_restore_blockheight,
        } => {
            if let ExpiredTimelocks::None = state.current_epoch(bitcoin_wallet).await? {
                let watch_request = state.lock_xmr_watch_request(lock_transfer_proof);

                select! {
                    received_xmr = monero_wallet.watch_for_transfer(watch_request) => {
                        match received_xmr {
                            Ok(()) => BobState::XmrLocked(state.xmr_locked(monero_wallet_restore_blockheight)),
                            Err(e) => {
                                 tracing::warn!("Waiting for refund because insufficient Monero have been locked! {}", e);
                                 state.wait_for_cancel_timelock_to_expire(bitcoin_wallet).await?;

                                 BobState::CancelTimelockExpired(state.cancel())
                            },
                        }
                    }
                    _ = state.wait_for_cancel_timelock_to_expire(bitcoin_wallet) => {
                        BobState::CancelTimelockExpired(state.cancel())
                    }
                }
            } else {
                BobState::CancelTimelockExpired(state.cancel())
            }
        }
        BobState::XmrLocked(state) => {
            if let ExpiredTimelocks::None = state.expired_timelock(bitcoin_wallet).await? {
                // Alice has locked Xmr
                // Bob sends Alice his key

                select! {
                    _ = event_loop_handle.send_encrypted_signature(state.tx_redeem_encsig()) => {
                        BobState::EncSigSent(state)
                    },
                    _ = state.wait_for_cancel_timelock_to_expire(bitcoin_wallet) => {
                        BobState::CancelTimelockExpired(state.cancel())
                    }
                }
            } else {
                BobState::CancelTimelockExpired(state.cancel())
            }
        }
        BobState::EncSigSent(state) => {
            if let ExpiredTimelocks::None = state.expired_timelock(bitcoin_wallet).await? {
                select! {
                    state5 = state.watch_for_redeem_btc(bitcoin_wallet) => {
                        BobState::BtcRedeemed(state5?)
                    },
                    _ = state.wait_for_cancel_timelock_to_expire(bitcoin_wallet) => {
                        BobState::CancelTimelockExpired(state.cancel())
                    }
                }
            } else {
                BobState::CancelTimelockExpired(state.cancel())
            }
        }
        BobState::BtcRedeemed(state) => {
            let (spend_key, view_key) = state.xmr_keys();

            // NOTE: This actually generates and opens a new wallet, closing the currently
            // open one.
            monero_wallet
                .create_from_and_load(spend_key, view_key, state.monero_wallet_restore_blockheight)
                .await?;

            // Ensure that the generated wallet is synced so we have a proper balance
            monero_wallet.refresh().await?;
            // Sweep (transfer all funds) to the given address
            let tx_hashes = monero_wallet.sweep_all(receive_monero_address).await?;

            for tx_hash in tx_hashes {
                tracing::info!("Sent XMR to {} in tx {}", receive_monero_address, tx_hash.0);
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
                    state.refund_btc(bitcoin_wallet).await?;
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

pub async fn request_price_and_setup(
    btc: bitcoin::Amount,
    event_loop_handle: &mut EventLoopHandle,
    env_config: &Config,
    bitcoin_refund_address: bitcoin::Address,
) -> Result<bob::state::State2> {
    let xmr = event_loop_handle.request_spot_price(btc).await?;

    tracing::info!("Spot price for {} is {}", btc, xmr);

    let state0 = State0::new(
        &mut OsRng,
        btc,
        xmr,
        env_config.bitcoin_cancel_timelock,
        env_config.bitcoin_punish_timelock,
        bitcoin_refund_address,
        env_config.monero_finality_confirmations,
    );

    let state2 = event_loop_handle.execution_setup(state0).await?;

    Ok(state2)
}
