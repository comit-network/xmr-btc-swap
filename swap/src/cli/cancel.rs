use crate::bitcoin::{parse_rpc_error_code, RpcErrorCode, Txid, Wallet};
use crate::database::{Database, Swap};
use crate::protocol::bob::BobState;
use anyhow::{bail, Result};
use std::sync::Arc;
use uuid::Uuid;

pub async fn cancel(
    swap_id: Uuid,
    bitcoin_wallet: Arc<Wallet>,
    db: Database,
) -> Result<(Txid, BobState)> {
    let state = db.get_state(swap_id)?.try_into_bob()?.into();

    let state6 = match state {
        BobState::BtcLocked(state3) => state3.cancel(),
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

    let txid = match state6.submit_tx_cancel(bitcoin_wallet.as_ref()).await {
        Ok(txid) => txid,
        Err(err) => {
            if let Ok(code) = parse_rpc_error_code(&err) {
                if code == i64::from(RpcErrorCode::RpcVerifyAlreadyInChain) {
                    tracing::info!("Cancel transaction has already been confirmed on chain")
                }
            }
            bail!(err);
        }
    };

    let state = BobState::BtcCancelled(state6);
    let db_state = state.clone().into();
    db.insert_latest_state(swap_id, Swap::Bob(db_state)).await?;

    Ok((txid, state))
}
