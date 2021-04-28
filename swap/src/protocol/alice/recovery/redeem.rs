use crate::bitcoin::{ExpiredTimelocks, Txid, Wallet};
use crate::database::{Database, Swap};
use crate::protocol::alice::AliceState;
use anyhow::{bail, Result};
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
    db: Arc<Database>,
    force: bool,
    finality: Finality,
) -> Result<(Txid, AliceState)> {
    let state = db.get_state(swap_id)?.try_into_alice()?.into();

    match state {
        AliceState::EncSigLearned {
            state3,
            encrypted_signature,
            ..
        } => {
            tracing::info!(%swap_id, "Trying to redeem swap");

            if !force {
                tracing::debug!(%swap_id, "Checking if timelocks have expired");

                let expired_timelocks = state3.expired_timelocks(bitcoin_wallet.as_ref()).await?;
                match expired_timelocks {
                    ExpiredTimelocks::None => (),
                    _ => bail!("{:?} timelock already expired, consider using refund or punish. You can use --force to publish the redeem transaction, but be aware that it is not safe to do so anymore!", expired_timelocks)
                }
            }

            let redeem_tx = state3.signed_redeem_transaction(*encrypted_signature)?;
            let (txid, subscription) = bitcoin_wallet.broadcast(redeem_tx, "redeem").await?;

            if let Finality::Await = finality {
                subscription.wait_until_final().await?;
            }

            let state = AliceState::BtcRedeemed;
            let db_state = (&state).into();

            db.insert_latest_state(swap_id, Swap::Alice(db_state))
                .await?;

            Ok((txid, state))
        }

        AliceState::Started { .. }
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
