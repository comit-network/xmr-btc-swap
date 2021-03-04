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
    #[error("The cancel transaction has already been published.")]
    CancelTxAlreadyPublished,
}

pub async fn cancel(
    swap_id: Uuid,
    state: BobState,
    bitcoin_wallet: Arc<Wallet>,
    db: Database,
    force: bool,
) -> Result<Result<(Txid, BobState), Error>> {
    let state4 = match state {
        BobState::BtcLocked(state3) => state3.cancel(),
        BobState::XmrLockProofReceived { state, .. } => state.cancel(),
        BobState::XmrLocked(state4) => state4,
        BobState::EncSigSent(state4) => state4,
        BobState::CancelTimelockExpired(state4) => state4,
        _ => bail!(
            "Cannot cancel swap {} because it is in state {} which is not refundable.",
            swap_id,
            state
        ),
    };

    if !force {
        if let ExpiredTimelocks::None = state4.expired_timelock(bitcoin_wallet.as_ref()).await? {
            return Ok(Err(Error::CancelTimelockNotExpiredYet));
        }

        if state4
            .check_for_tx_cancel(bitcoin_wallet.as_ref())
            .await
            .is_ok()
        {
            let state = BobState::BtcCancelled(state4);
            let db_state = state.into();
            db.insert_latest_state(swap_id, Swap::Bob(db_state)).await?;

            return Ok(Err(Error::CancelTxAlreadyPublished));
        }
    }

    let txid = state4.submit_tx_cancel(bitcoin_wallet.as_ref()).await?;

    let state = BobState::BtcCancelled(state4);
    let db_state = state.clone().into();
    db.insert_latest_state(swap_id, Swap::Bob(db_state)).await?;

    Ok(Ok((txid, state)))
}
