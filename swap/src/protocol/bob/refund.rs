use crate::bitcoin::Wallet;
use crate::database::{Database, Swap};
use crate::execution_params::ExecutionParams;
use crate::protocol::bob::BobState;
use anyhow::{bail, Result};
use std::sync::Arc;
use uuid::Uuid;

#[derive(thiserror::Error, Debug, Clone, Copy)]
#[error("Cannot refund because swap {0} was not cancelled yet. Make sure to cancel the swap before trying to refund.")]
pub struct SwapNotCancelledYet(Uuid);

pub async fn refund(
    swap_id: Uuid,
    state: BobState,
    execution_params: ExecutionParams,
    bitcoin_wallet: Arc<Wallet>,
    db: Database,
    force: bool,
) -> Result<Result<BobState, SwapNotCancelledYet>> {
    let state4 = if force {
        match state {
            BobState::BtcLocked(state3) => state3.cancel(),
            BobState::XmrLockProofReceived { state, .. } => state.cancel(),
            BobState::XmrLocked(state4) => state4,
            BobState::EncSigSent(state4) => state4,
            BobState::CancelTimelockExpired(state4) => state4,
            BobState::BtcCancelled(state4) => state4,
            _ => bail!(
                "Cannot refund swap {} because it is in state {} which is not refundable.",
                swap_id,
                state
            ),
        }
    } else {
        match state {
            BobState::BtcCancelled(state4) => state4,
            _ => {
                return Ok(Err(SwapNotCancelledYet(swap_id)));
            }
        }
    };

    state4
        .refund_btc(bitcoin_wallet.as_ref(), execution_params)
        .await?;

    let state = BobState::BtcRefunded(state4);
    let db_state = state.clone().into();

    db.insert_latest_state(swap_id, Swap::Bob(db_state)).await?;

    Ok(Ok(state))
}
