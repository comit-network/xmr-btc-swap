use crate::bitcoin::{ExpiredTimelocks, Txid, Wallet};
use crate::database::{Database, Swap};
use crate::protocol::bob::BobState;
use anyhow::{bail, Result};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, thiserror::Error, Clone, Copy)]
pub enum Error {
    #[error("The cancel timelock has not expired yet.")]
    CancelTimelockNotExpiredYet,
}

pub async fn cancel(
    swap_id: Uuid,
    bitcoin_wallet: Arc<Wallet>,
    db: Database,
    force: bool,
) -> Result<Result<(Txid, BobState), Error>> {
    let state = db.get_state(swap_id)?.try_into_bob()?.into();

    let state6 = match state {
        BobState::BtcLocked(state3) => state3.cancel(),
        BobState::XmrLockProofReceived { state, .. } => state.cancel(),
        BobState::XmrLocked(state4) => state4.cancel(),
        BobState::EncSigSent(state4) => state4.cancel(),
        BobState::CancelTimelockExpired(state6) => state6,
        BobState::Started { .. }
        | BobState::SwapSetupCompleted(_)
        | BobState::BtcRedeemed(_)
        | BobState::BtcCancelled(_)
        | BobState::BtcRefunded(_)
        | BobState::XmrRedeemed { .. }
        | BobState::BtcPunished { .. }
        | BobState::SafelyAborted => bail!(
            "Cannot cancel swap {} because it is in state {} which is not refundable.",
            swap_id,
            state
        ),
    };

    tracing::info!(%swap_id, "Manually cancelling swap");

    if !force {
        tracing::debug!(%swap_id, "Checking if cancel timelock is expired");

        if let ExpiredTimelocks::None = state6.expired_timelock(bitcoin_wallet.as_ref()).await? {
            return Ok(Err(Error::CancelTimelockNotExpiredYet));
        }
    }

    let txid = if let Ok(tx) = state6.check_for_tx_cancel(bitcoin_wallet.as_ref()).await {
        tracing::debug!(%swap_id, "Cancel transaction has already been published");

        tx.txid()
    } else {
        state6.submit_tx_cancel(bitcoin_wallet.as_ref()).await?
    };

    let state = BobState::BtcCancelled(state6);
    let db_state = state.clone().into();
    db.insert_latest_state(swap_id, Swap::Bob(db_state)).await?;

    Ok(Ok((txid, state)))
}
