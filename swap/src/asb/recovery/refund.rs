use crate::bitcoin::{self};
use crate::common::retry;
use crate::monero;
use crate::protocol::alice::AliceState;
use crate::protocol::Database;
use anyhow::{bail, Result};
use libp2p::PeerId;
use std::convert::TryInto;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "Counterparty {0} did not refund the BTC yet. You can try again later or try to punish."
    )]
    RefundTransactionNotPublishedYet(PeerId),

    // Errors indicating that the swap cannot be refunded because because it is in a abort/final
    // state
    #[error("Swap is in state {0} where no XMR was locked. Try aborting instead.")]
    NoXmrLocked(AliceState),
    #[error("Swap is in state {0} which is not refundable")]
    SwapNotRefundable(AliceState),
}

pub async fn refund(
    swap_id: Uuid,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallets>,
    db: Arc<dyn Database>,
) -> Result<AliceState> {
    let state = db.get_state(swap_id).await?.try_into()?;

    let (transfer_proof, state3) = match state {
        // In case no XMR has been locked, move to Safely Aborted
        AliceState::Started { .. }
        | AliceState::BtcLockTransactionSeen { .. }
        | AliceState::BtcLocked { .. } => bail!(Error::NoXmrLocked(state)),

        // Refund potentially possible (no knowledge of cancel transaction)
        AliceState::XmrLockTransactionSent { transfer_proof, state3, .. }
        | AliceState::XmrLocked { transfer_proof, state3, .. }
        | AliceState::XmrLockTransferProofSent { transfer_proof, state3, .. }
        | AliceState::EncSigLearned { transfer_proof, state3, .. }
        | AliceState::CancelTimelockExpired { transfer_proof, state3, .. }

        // Refund possible due to cancel transaction already being published
        | AliceState::BtcCancelled { transfer_proof, state3, .. }
        | AliceState::BtcRefunded { transfer_proof, state3, .. }
        | AliceState::BtcPunishable { transfer_proof, state3, .. } => {
            (transfer_proof, state3)
        }

        // Alice already in final state
        AliceState::BtcRedeemTransactionPublished { .. }
        | AliceState::BtcRedeemed
        | AliceState::XmrRefunded
        | AliceState::BtcEarlyRefundable { .. }
        | AliceState::BtcEarlyRefunded(_)
        | AliceState::BtcPunished { .. }
        | AliceState::SafelyAborted => bail!(Error::SwapNotRefundable(state)),
    };

    tracing::info!(%swap_id, "Trying to manually refund swap");

    let spend_key = if let Some(published_refund_tx) =
        state3.fetch_tx_refund(bitcoin_wallet.as_ref()).await?
    {
        tracing::debug!(%swap_id, "Bitcoin refund transaction found, extracting key to refund Monero");
        state3.extract_monero_private_key(published_refund_tx)?
    } else {
        let bob_peer_id = db.get_peer_id(swap_id).await?;
        bail!(Error::RefundTransactionNotPublishedYet(bob_peer_id),);
    };

    retry(
        "Refund Monero",
        || async {
            state3
                .refund_xmr(
                    monero_wallet.clone(),
                    swap_id,
                    spend_key,
                    transfer_proof.clone(),
                )
                .await
                .map_err(backoff::Error::transient)
        },
        None,
        Duration::from_secs(60),
    )
    .await?;

    let state = AliceState::XmrRefunded;
    db.insert_latest_state(swap_id, state.clone().into())
        .await?;

    Ok(state)
}
