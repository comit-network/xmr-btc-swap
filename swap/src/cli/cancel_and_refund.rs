use crate::bitcoin::{parse_rpc_error_code, ExpiredTimelocks, RpcErrorCode, Wallet};
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
        tracing::info!(%err, "Could not submit cancel transaction");
    };

    let state = match refund(swap_id, bitcoin_wallet, db).await {
        Ok(s) => s,
        Err(e) => bail!(e),
    };
    if matches!(state, BobState::BtcRefunded { .. }) {
        tracing::info!("Refund transaction submitted");
    }
    Ok(state)
}

pub async fn cancel(
    swap_id: Uuid,
    bitcoin_wallet: Arc<Wallet>,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<(Txid, BobState)> {
    let state = db.get_state(swap_id).await?.try_into()?;

    let state6 = match state {
        BobState::BtcLocked { state3, .. } => state3.cancel(),
        BobState::XmrLockProofReceived { state, .. } => state.cancel(),
        BobState::XmrLocked(state4) => state4.cancel(),
        BobState::EncSigSent(state4) => state4.cancel(),
        BobState::CancelTimelockExpired(state6) => state6,
        BobState::BtcRefunded(state6) => state6,
        BobState::BtcCancelled(state6) => state6,

        BobState::Started { .. }
        | BobState::SwapSetupCompleted(_)
        | BobState::BtcRedeemed(_)
        | BobState::XmrRedeemed { .. }
        | BobState::BtcPunished { .. }
        | BobState::SafelyAborted => bail!(
            "Cannot cancel swap {} because it is in state {} which is not refundable.",
            swap_id,
            state
        ),
    };

    tracing::info!(%swap_id, "Manually cancelling swap");

    match state6.submit_tx_cancel(bitcoin_wallet.as_ref()).await {
        Ok((txid, _)) => {
            let state = BobState::BtcCancelled(state6);
            db.insert_latest_state(swap_id, state.clone().into())
                .await?;
            Ok((txid, state))
        }
        Err(err) => {
            if let Ok(error_code) = parse_rpc_error_code(&err) {
                if error_code == i64::from(RpcErrorCode::RpcVerifyError) {
                    if let ExpiredTimelocks::None { .. } =
                        state6.expired_timelock(bitcoin_wallet.as_ref()).await?
                    {
                        tracing::debug!(%error_code, "parse rpc error");
                        tracing::info!("General error trying to submit cancel transaction");
                    } else {
                        let txid = state6
                            .construct_tx_cancel()
                            .expect("Error when constructing tx_cancel")
                            .txid();
                        let state = BobState::BtcCancelled(state6);
                        db.insert_latest_state(swap_id, state.clone().into())
                            .await?;
                        tracing::info!("Cancel transaction has already been confirmed on chain. The swap has therefore already been cancelled by Alice.");
                        return Ok((txid, state));
                    }
                }
            }
            bail!(err);
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
        BobState::BtcLocked { state3, .. } => state3.cancel(),
        BobState::XmrLockProofReceived { state, .. } => state.cancel(),
        BobState::XmrLocked(state4) => state4.cancel(),
        BobState::EncSigSent(state4) => state4.cancel(),
        BobState::CancelTimelockExpired(state6) => state6,
        BobState::BtcCancelled(state6) => state6,
        BobState::Started { .. }
        | BobState::SwapSetupCompleted(_)
        | BobState::BtcRedeemed(_)
        | BobState::BtcRefunded(_)
        | BobState::XmrRedeemed { .. }
        | BobState::BtcPunished { .. }
        | BobState::SafelyAborted => bail!(
            "Cannot refund swap {} because it is in state {} which is not refundable.",
            swap_id,
            state
        ),
    };

    tracing::info!(%swap_id, "Manually refunding swap");

    match state6.publish_refund_btc(bitcoin_wallet.as_ref()).await {
        Ok(_) => (),
        Err(error) => {
            if let Ok(error_code) = parse_rpc_error_code(&error) {
                if error_code == i64::from(RpcErrorCode::RpcVerifyError) {
                    if let ExpiredTimelocks::None { .. } =
                        state6.expired_timelock(bitcoin_wallet.as_ref()).await?
                    {
                        bail!(error);
                    } else {
                        let state = BobState::BtcPunished {
                            tx_lock_id: state6.tx_lock_id(),
                        };
                        db.insert_latest_state(swap_id, state.clone().into())
                            .await?;

                        tracing::info!("Cannot refund because BTC is punished by Alice.");
                        return Ok(state);
                    }
                }
            }
        }
    }
    let state = BobState::BtcRefunded(state6);
    db.insert_latest_state(swap_id, state.clone().into())
        .await?;

    Ok(state)
}
