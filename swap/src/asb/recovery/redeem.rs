use crate::bitcoin::{Txid, Wallet};
use crate::protocol::alice::AliceState;
use crate::protocol::Database;
use anyhow::{bail, Result};
use std::convert::TryInto;
use std::sync::Arc;
use uuid::Uuid;

pub enum Finality {
    Await,
    NotAwait,
}

impl Finality {
    pub fn from_bool(do_not_await_finality: bool) -> Self {
        if do_not_await_finality {
            Self::NotAwait
        } else {
            Self::Await
        }
    }
}

pub async fn redeem(
    swap_id: Uuid,
    bitcoin_wallet: Arc<Wallet>,
    db: Arc<dyn Database>,
    finality: Finality,
) -> Result<(Txid, AliceState)> {
    let state = db.get_state(swap_id).await?.try_into()?;

    match state {
        AliceState::EncSigLearned {
            state3,
            encrypted_signature,
            ..
        } => {
            tracing::info!(%swap_id, "Trying to redeem swap");

            let redeem_tx = state3.signed_redeem_transaction(*encrypted_signature)?;
            let (txid, subscription) = bitcoin_wallet.broadcast(redeem_tx, "redeem").await?;

            subscription.wait_until_seen().await?;

            let state = AliceState::BtcRedeemTransactionPublished { state3 };
            db.insert_latest_state(swap_id, state.into()).await?;

            if let Finality::Await = finality {
                subscription.wait_until_final().await?;
            }

            let state = AliceState::BtcRedeemed;
            db.insert_latest_state(swap_id, state.clone().into())
                .await?;

            Ok((txid, state))
        }
        AliceState::BtcRedeemTransactionPublished { state3 } => {
            let subscription = bitcoin_wallet.subscribe_to(state3.tx_redeem()).await;
            if let Finality::Await = finality {
                subscription.wait_until_final().await?;
            }

            let state = AliceState::BtcRedeemed;
            db.insert_latest_state(swap_id, state.clone().into())
                .await?;

            let txid = state3.tx_redeem().txid();

            Ok((txid, state))
        }
        AliceState::Started { .. }
        | AliceState::BtcLockTransactionSeen { .. }
        | AliceState::BtcLocked { .. }
        | AliceState::XmrLockTransactionSent { .. }
        | AliceState::XmrLocked { .. }
        | AliceState::XmrLockTransferProofSent { .. }
        | AliceState::CancelTimelockExpired { .. }
        | AliceState::BtcCancelled { .. }
        | AliceState::BtcRefunded { .. }
        | AliceState::BtcPunishable { .. }
        | AliceState::BtcRedeemed
        | AliceState::XmrRefunded
        | AliceState::BtcPunished
        | AliceState::SafelyAborted => bail!(
            "Cannot redeem swap {} because it is in state {} which cannot be manually redeemed",
            swap_id,
            state
        ),
    }
}
