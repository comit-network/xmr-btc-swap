use crate::bitcoin::{self, ExpiredTimelocks, Txid};
use crate::database::{Database, Swap};
use crate::protocol::alice::AliceState;
use anyhow::{bail, Result};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // Errors indicating the swap can *currently* not be punished but might be later
    #[error("Swap is not in a cancelled state Make sure to cancel the swap before trying to punish or use --force.")]
    SwapNotCancelled,
    #[error("The punish transaction cannot be published because the punish timelock has not expired yet. Please try again later")]
    PunishTimelockNotExpiredYet,

    // Errors indicating that the swap cannot be refunded because it is in a abort/final state
    // state
    #[error("Cannot punish swap because it is in state {0} where no BTC was locked. Try aborting instead.")]
    NoBtcLocked(AliceState),
    #[error("Cannot punish swap because it is in state {0} which is not punishable")]
    SwapNotPunishable(AliceState),
}

pub async fn punish(
    swap_id: Uuid,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    db: Arc<Database>,
    force: bool,
) -> Result<Result<(Txid, AliceState), Error>> {
    let state = db.get_state(swap_id)?.try_into_alice()?.into();

    let state3 = if force {
        match state {

            // In case no XMR has been locked, move to Safely Aborted
            AliceState::Started { .. } => bail!(Error::NoBtcLocked(state)),

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
            | AliceState::BtcPunishable {state3, ..} => {
                state3
            }

            // If the swap was refunded it cannot be punished
            AliceState::BtcRedeemTransactionPublished { .. }
            | AliceState::BtcRefunded {..}
            // Alice already in final state
            | AliceState::BtcRedeemed
            | AliceState::XmrRefunded
            | AliceState::BtcPunished
            | AliceState::SafelyAborted => bail!(Error::SwapNotPunishable(state)),
        }
    } else {
        match state {
            AliceState::Started { .. } => {
                bail!(Error::NoBtcLocked(state))
            }

            AliceState::BtcCancelled { state3, .. } | AliceState::BtcPunishable { state3, .. } => {
                state3
            }

            AliceState::BtcRefunded { .. }
            | AliceState::BtcRedeemed
            | AliceState::XmrRefunded
            | AliceState::BtcPunished
            | AliceState::SafelyAborted => bail!(Error::SwapNotPunishable(state)),

            _ => return Ok(Err(Error::SwapNotCancelled)),
        }
    };

    tracing::info!(%swap_id, "Trying to manually punish swap");

    if !force {
        tracing::debug!(%swap_id, "Checking if punish timelock is expired");

        if let ExpiredTimelocks::Cancel = state3.expired_timelocks(bitcoin_wallet.as_ref()).await? {
            return Ok(Err(Error::PunishTimelockNotExpiredYet));
        }
    }

    let txid = state3.punish_btc(&bitcoin_wallet).await?;

    let state = AliceState::BtcPunished;
    let db_state = (&state).into();
    db.insert_latest_state(swap_id, Swap::Alice(db_state))
        .await?;

    Ok(Ok((txid, state)))
}
