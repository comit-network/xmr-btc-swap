use crate::bitcoin::{self, Txid};
use crate::database::{SledDatabase, Swap};
use crate::protocol::alice::AliceState;
use anyhow::{bail, Result};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Cannot punish swap because it is in state {0} which is not punishable")]
    SwapNotPunishable(AliceState),
}

pub async fn punish(
    swap_id: Uuid,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    db: Arc<SledDatabase>,
) -> Result<(Txid, AliceState)> {
    let state = db.get_state(swap_id)?.try_into_alice()?.into();

    let state3 = match state {
        // Punish potentially possible (no knowledge of cancel transaction)
        AliceState::BtcLockTransactionSeen { state3 }
        | AliceState::BtcLocked { state3, .. }
        | AliceState::XmrLockTransactionSent {state3, ..}
        | AliceState::XmrLocked {state3, ..}
        | AliceState::XmrLockTransferProofSent {state3, ..}
        | AliceState::EncSigLearned {state3, ..}
        | AliceState::CancelTimelockExpired {state3, ..}
        // Punish possible due to cancel transaction already being published
        | AliceState::BtcCancelled {state3, ..}
        | AliceState::BtcPunishable {state3, ..} => { state3 }
        // The state machine is in a state where punish is theoretically impossible but we try and punish anyway as this is what the user wants
        AliceState::BtcRedeemTransactionPublished { state3 }
        | AliceState::BtcRefunded { state3,.. }
        | AliceState::Started { state3 }  => { state3 }
        // Alice already in final state
        | AliceState::BtcRedeemed
        | AliceState::XmrRefunded
        | AliceState::BtcPunished
        | AliceState::SafelyAborted => bail!(Error::SwapNotPunishable(state)),
    };

    tracing::info!(%swap_id, "Trying to manually punish swap");

    let txid = state3.punish_btc(&bitcoin_wallet).await?;

    let state = AliceState::BtcPunished;
    let db_state = (&state).into();
    db.insert_latest_state(swap_id, Swap::Alice(db_state))
        .await?;

    Ok((txid, state))
}
