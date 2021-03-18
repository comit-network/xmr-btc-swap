use crate::bitcoin;
use crate::bitcoin::{CancelTimelock, PunishTimelock, TxCancel, TxLock, TxRefund};
use anyhow::{bail, Context, Result};

pub async fn publish_cancel_transaction(
    tx_lock: TxLock,
    a: bitcoin::SecretKey,
    B: bitcoin::PublicKey,
    cancel_timelock: CancelTimelock,
    tx_cancel_sig_bob: bitcoin::Signature,
    bitcoin_wallet: &bitcoin::Wallet,
) -> Result<()> {
    let tx_cancel = bitcoin::TxCancel::new(&tx_lock, cancel_timelock, a.public(), B);

    // If Bob hasn't yet broadcasted the tx cancel, we do it
    if bitcoin_wallet
        .get_raw_transaction(tx_cancel.txid())
        .await
        .is_err()
    {
        // TODO(Franck): Maybe the cancel transaction is already mined, in this case,
        // the broadcast will error out.

        let transaction = tx_cancel
            .complete_as_alice(a, B, tx_cancel_sig_bob)
            .context("Failed to complete Bitcoin cancel transaction")?;

        // TODO(Franck): Error handling is delicate, why can't we broadcast?
        let (..) = bitcoin_wallet.broadcast(transaction, "cancel").await?;

        // TODO(Franck): Wait until transaction is mined and returned mined
        // block height
    }

    Ok(())
}

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
