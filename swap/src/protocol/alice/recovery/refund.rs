use crate::bitcoin::{self};
use crate::database::{Database, Swap};
use crate::monero;
use crate::protocol::alice::AliceState;
use anyhow::{bail, Result};
use libp2p::PeerId;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // Errors indicating the the swap can *currently* not be refunded but might be later
    #[error("Swap is not in a cancelled state. Make sure to cancel the swap before trying to refund or use --force.")]
    SwapNotCancelled,
    #[error(
        "Counterparty {0} did not refund the BTC yet. You can try again later or try to punish."
    )]
    RefundTransactionNotPublishedYet(PeerId),

    // Errors indicating that the swap cannot be refunded because because it is in a abort/final
    // state
    #[error("Swa is in state {0} where no XMR was locked. Try aborting instead.")]
    NoXmrLocked(AliceState),
    #[error("Swap is in state {0} which is not refundable")]
    SwapNotRefundable(AliceState),
}

pub async fn refund(
    swap_id: Uuid,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    db: Arc<Database>,
    force: bool,
) -> Result<Result<AliceState, Error>> {
    let state = db.get_state(swap_id)?.try_into_alice()?.into();

    let (monero_wallet_restore_blockheight, transfer_proof, state3) = if force {
        match state {

            // In case no XMR has been locked, move to Safely Aborted
            AliceState::Started { .. }
            | AliceState::BtcLocked { .. } => bail!(Error::NoXmrLocked(state)),

            // Refund potentially possible (no knowledge of cancel transaction)
            AliceState::XmrLockTransactionSent { monero_wallet_restore_blockheight, transfer_proof, state3, }
            | AliceState::XmrLocked { monero_wallet_restore_blockheight, transfer_proof, state3 }
            | AliceState::XmrLockTransferProofSent { monero_wallet_restore_blockheight, transfer_proof, state3 }
            | AliceState::EncSigLearned { monero_wallet_restore_blockheight, transfer_proof, state3, .. }
            | AliceState::CancelTimelockExpired { monero_wallet_restore_blockheight, transfer_proof, state3 }

            // Refund possible due to cancel transaction already being published
            | AliceState::BtcCancelled { monero_wallet_restore_blockheight, transfer_proof, state3 }
            | AliceState::BtcRefunded { monero_wallet_restore_blockheight, transfer_proof, state3, .. }
            | AliceState::BtcPunishable { monero_wallet_restore_blockheight, transfer_proof, state3, .. } => {
                (monero_wallet_restore_blockheight, transfer_proof, state3)
            }

            // Alice already in final state
            AliceState::BtcRedeemed
            | AliceState::XmrRefunded
            | AliceState::BtcPunished
            | AliceState::SafelyAborted => bail!(Error::SwapNotRefundable(state)),
        }
    } else {
        match state {
            AliceState::Started { .. } | AliceState::BtcLocked { .. } => {
                bail!(Error::NoXmrLocked(state))
            }

            AliceState::BtcCancelled {
                monero_wallet_restore_blockheight,
                transfer_proof,
                state3,
            }
            | AliceState::BtcRefunded {
                monero_wallet_restore_blockheight,
                transfer_proof,
                state3,
                ..
            }
            | AliceState::BtcPunishable {
                monero_wallet_restore_blockheight,
                transfer_proof,
                state3,
                ..
            } => (monero_wallet_restore_blockheight, transfer_proof, state3),

            AliceState::BtcRedeemed
            | AliceState::XmrRefunded
            | AliceState::BtcPunished
            | AliceState::SafelyAborted => bail!(Error::SwapNotRefundable(state)),

            _ => return Ok(Err(Error::SwapNotCancelled)),
        }
    };

    tracing::info!(%swap_id, "Trying to manually refund swap");

    let spend_key = if let Ok(published_refund_tx) =
        state3.fetch_tx_refund(bitcoin_wallet.as_ref()).await
    {
        tracing::debug!(%swap_id, "Bitcoin refund transaction found, extracting key to refund Monero");
        state3.extract_monero_private_key(published_refund_tx)?
    } else {
        let bob_peer_id = db.get_peer_id(swap_id)?;
        return Ok(Err(Error::RefundTransactionNotPublishedYet(bob_peer_id)));
    };

    state3
        .refund_xmr(
            &monero_wallet,
            monero_wallet_restore_blockheight,
            swap_id.to_string(),
            spend_key,
            transfer_proof,
        )
        .await?;

    let state = AliceState::XmrRefunded;
    let db_state = (&state).into();
    db.insert_latest_state(swap_id, Swap::Alice(db_state))
        .await?;

    Ok(Ok(state))
}
