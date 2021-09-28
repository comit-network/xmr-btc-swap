use crate::protocol::Database;
use crate::protocol::alice::AliceState;
use anyhow::{bail, Result};
use uuid::Uuid;
use std::convert::TryInto;

pub async fn safely_abort(swap_id: Uuid, db: impl Database) -> Result<AliceState> {
    let state = db.get_state(swap_id).await?.try_into()?;

    match state {
        AliceState::Started { .. }
        | AliceState::BtcLockTransactionSeen { .. }
        | AliceState::BtcLocked { .. } => {
            let state = AliceState::SafelyAborted;

            db.insert_latest_state(swap_id, state.clone().into())
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
