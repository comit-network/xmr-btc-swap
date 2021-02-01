use crate::{
    bitcoin::Wallet,
    database::{Database, Swap},
    execution_params::ExecutionParams,
    protocol::bob::BobState,
};
use anyhow::Result;
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
) -> Result<Result<BobState, SwapNotCancelledYet>> {
    let state4 = match state {
        BobState::BtcCancelled(state4) => state4,
        _ => {
            return Ok(Err(SwapNotCancelledYet(swap_id)));
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
