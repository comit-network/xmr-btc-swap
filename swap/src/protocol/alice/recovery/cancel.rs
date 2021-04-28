use crate::bitcoin::{ExpiredTimelocks, Txid, Wallet};
use crate::database::{Database, Swap};
use crate::protocol::alice::AliceState;
use anyhow::{bail, Result};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, thiserror::Error, Clone, Copy)]
pub enum Error {
    #[error("The cancel transaction cannot be published because the cancel timelock has not expired yet. Please try again later")]
    CancelTimelockNotExpiredYet,
}

pub async fn cancel(
    swap_id: Uuid,
    bitcoin_wallet: Arc<Wallet>,
    db: Arc<Database>,
    force: bool,
) -> Result<Result<(Txid, AliceState), Error>> {
    let state = db.get_state(swap_id)?.try_into_alice()?.into();

    let (monero_wallet_restore_blockheight, transfer_proof, state3) = match state {

        // In case no XMR has been locked, move to Safely Aborted
        AliceState::Started { .. }
        | AliceState::BtcLocked { .. } => bail!("Cannot cancel swap {} because it is in state {} where no XMR was locked.", swap_id, state),

        AliceState::XmrLockTransactionSent { monero_wallet_restore_blockheight, transfer_proof, state3,  }
        | AliceState::XmrLocked { monero_wallet_restore_blockheight, transfer_proof, state3 }
        | AliceState::XmrLockTransferProofSent { monero_wallet_restore_blockheight, transfer_proof, state3 }
        // in cancel mode we do not care about the fact that we could redeem, but always wait for cancellation (leading either refund or punish)
        | AliceState::EncSigLearned { monero_wallet_restore_blockheight, transfer_proof, state3, .. }
        | AliceState::CancelTimelockExpired { monero_wallet_restore_blockheight, transfer_proof, state3} => {
            (monero_wallet_restore_blockheight, transfer_proof, state3)
        }

        // The cancel tx was already published, but Alice not yet in final state
        AliceState::BtcCancelled { .. }
        | AliceState::BtcRefunded { .. }
        | AliceState::BtcPunishable { .. }

        // Alice already in final state
        | AliceState::BtcRedeemed
        | AliceState::XmrRefunded
        | AliceState::BtcPunished
        | AliceState::SafelyAborted => bail!("Cannot cancel swap {} because it is in state {} which is not cancelable", swap_id, state),
    };

    tracing::info!(%swap_id, "Trying to manually cancel swap");

    if !force {
        tracing::debug!(%swap_id, "Checking if cancel timelock is expired");

        if let ExpiredTimelocks::None = state3.expired_timelocks(bitcoin_wallet.as_ref()).await? {
            return Ok(Err(Error::CancelTimelockNotExpiredYet));
        }
    }

    let txid = if let Ok(tx) = state3.check_for_tx_cancel(bitcoin_wallet.as_ref()).await {
        let txid = tx.txid();
        tracing::debug!(%swap_id, "Cancel transaction has already been published: {}", txid);
        txid
    } else {
        state3.submit_tx_cancel(bitcoin_wallet.as_ref()).await?
    };

    let state = AliceState::BtcCancelled {
        monero_wallet_restore_blockheight,
        transfer_proof,
        state3,
    };
    let db_state = (&state).into();
    db.insert_latest_state(swap_id, Swap::Alice(db_state))
        .await?;

    Ok(Ok((txid, state)))
}
