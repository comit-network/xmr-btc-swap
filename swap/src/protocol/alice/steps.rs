use crate::bitcoin;
use crate::bitcoin::{PunishTimelock, TxCancel, TxRefund};
use anyhow::{bail, Result};

pub async fn wait_for_bitcoin_refund(
    tx_cancel: &TxCancel,
    tx_refund: &TxRefund,
    punish_timelock: PunishTimelock,
    bitcoin_wallet: &bitcoin::Wallet,
) -> Result<Option<bitcoin::Transaction>> {
    let refund_tx_id = tx_refund.txid();
    let seen_refund_tx =
        bitcoin_wallet.watch_until_status(tx_refund, |status| status.has_been_seen());

    let punish_timelock_expired = bitcoin_wallet.watch_until_status(tx_cancel, |status| {
        status.is_confirmed_with(punish_timelock)
    });

    tokio::select! {
        seen_refund = seen_refund_tx => {
            match seen_refund {
                Ok(()) => {
                    let published_refund_tx = bitcoin_wallet.get_raw_transaction(refund_tx_id).await?;

                    Ok(Some(published_refund_tx))
                }
                Err(e) => {
                    bail!(e.context("Failed to monitor refund transaction"))
                }
            }
        }
        _ = punish_timelock_expired => {
            Ok(None)
        }
    }
}
