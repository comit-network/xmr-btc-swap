use crate::database::{Database, Swap};
use crate::protocol::alice::AliceState;
use anyhow::{bail, Result};
use std::sync::Arc;
use uuid::Uuid;

pub async fn safely_abort(swap_id: Uuid, db: Arc<Database>) -> Result<AliceState> {
    let state = db.get_state(swap_id)?.try_into_alice()?.into();

    match state {
        AliceState::Started { .. }
        | AliceState::BtcLockTransactionSeen { .. }
        | AliceState::BtcLocked { .. } => {
            let state = AliceState::SafelyAborted;

            let db_state = (&state).into();
            db.insert_latest_state(swap_id, Swap::Alice(db_state))
                .await?;

            Ok(state)
        }

        AliceState::XmrLockTransactionSent { .. }
        | AliceState::XmrLocked { .. }
        | AliceState::XmrLockTransferProofSent { .. }
        | AliceState::EncSigLearned { .. }
        | AliceState::BtcRedeemTransactionPublished { .. }
        | AliceState::CancelTimelockExpired { .. }
        | AliceState::BtcCancelled { .. }
        | AliceState::BtcRefunded { .. }
        | AliceState::BtcPunishable { .. }
        | AliceState::BtcRedeemed
        | AliceState::XmrRefunded
        | AliceState::BtcPunished
        | AliceState::SafelyAborted => bail!(
            "Cannot safely abort swap {} because it is in state {} which cannot be safely aborted",
            swap_id,
            state
        ),
    }
}
