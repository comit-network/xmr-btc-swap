use crate::bitcoin::timelocks::BlockHeight;
use crate::bitcoin::{Address, Amount, Transaction};
use crate::execution_params::ExecutionParams;
use ::bitcoin::util::psbt::PartiallySignedTransaction;
use ::bitcoin::Txid;
use anyhow::{anyhow, bail, Context, Result};
use backoff::backoff::Constant as ConstantBackoff;
use backoff::future::retry;
use bdk::blockchain::{noop_progress, Blockchain, ElectrumBlockchain};
use bdk::descriptor::Segwitv0;
use bdk::electrum_client::{self, ElectrumApi};
use bdk::keys::DerivableKey;
use bdk::{FeeRate, KeychainKind};
use bitcoin::Script;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::interval;

const SLED_TREE_NAME: &str = "default_tree";

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("Sending the request failed")]
    Io(reqwest::Error),
    #[error("The transaction is not minded yet")]
    NotYetMined,
    #[error("Deserialization failed")]
    JsonDeserialization(reqwest::Error),
    #[error("Electrum client error")]
    ElectrumClient(electrum_client::Error),
}

pub struct Client {
    electrum: bdk::electrum_client::Client,
    latest_block: BlockHeight,
    last_ping: Instant,
    interval: Duration,
}

impl Client {
    fn ping(&mut self) -> Result<()> {
        if self.last_ping.elapsed() > self.interval {
            self.electrum
                .ping()
                .map_err(|e| anyhow!("Could not refresh subscriptions: {:?}", e))?;
            self.last_ping = Instant::now();
        }
        Ok(())
    }

    fn drain_blockheight_notifications(&mut self) -> Result<()> {
        let latest_block = std::iter::from_fn(|| self.electrum.block_headers_pop().transpose())
            .last()
            .transpose()
            .map_err(|e| anyhow!("Failed to pop header notification: {:?}", e))?;

        if let Some(new_block) = latest_block {
            self.latest_block = BlockHeight::try_from(new_block)?;
        }

        Ok(())
    }
}

pub struct Wallet {
    client: Arc<Mutex<Client>>,
    wallet: Arc<Mutex<bdk::Wallet<ElectrumBlockchain, bdk::sled::Tree>>>,
    http_url: Url,
    rpc_url: Url,
}

impl Wallet {
    pub async fn new(
        electrum_rpc_url: Url,
        electrum_http_url: Url,
        network: bitcoin::Network,
        wallet_dir: &Path,
        key: impl DerivableKey<Segwitv0> + Clone,
    ) -> Result<Self> {
        // Workaround for https://github.com/bitcoindevkit/rust-electrum-client/issues/47.
        let config = electrum_client::ConfigBuilder::default().retry(2).build();

        let client =
            bdk::electrum_client::Client::from_config(electrum_rpc_url.as_str(), config.clone())
                .map_err(|e| anyhow!("Failed to init electrum rpc client: {:?}", e))?;

        let db = bdk::sled::open(wallet_dir)?.open_tree(SLED_TREE_NAME)?;

        let bdk_wallet = bdk::Wallet::new(
            bdk::template::BIP84(key.clone(), KeychainKind::External),
            Some(bdk::template::BIP84(key, KeychainKind::Internal)),
            network,
            db,
            ElectrumBlockchain::from(client),
        )?;

        let electrum = bdk::electrum_client::Client::from_config(electrum_rpc_url.as_str(), config)
            .map_err(|e| anyhow!("Failed to init electrum rpc client {:?}", e))?;

        let latest_block = electrum.block_headers_subscribe().map_err(|e| {
            anyhow!(
                "Electrum client failed to subscribe to header notifications: {:?}",
                e
            )
        })?;

        let interval = Duration::from_secs(5);

        Ok(Self {
            wallet: Arc::new(Mutex::new(bdk_wallet)),
            client: Arc::new(Mutex::new(Client {
                electrum,
                latest_block: BlockHeight::try_from(latest_block)?,
                last_ping: Instant::now() - interval,
                interval,
            })),
            http_url: electrum_http_url,
            rpc_url: electrum_rpc_url,
        })
    }

    pub async fn balance(&self) -> Result<Amount> {
        let balance = self
            .wallet
            .lock()
            .await
            .get_balance()
            .context("Failed to calculate Bitcoin balance")?;

        Ok(Amount::from_sat(balance))
    }

    pub async fn new_address(&self) -> Result<Address> {
        let address = self
            .wallet
            .lock()
            .await
            .get_new_address()
            .context("Failed to get new Bitcoin address")?;

        Ok(address)
    }

    pub async fn get_tx(&self, txid: Txid) -> Result<Option<Transaction>> {
        let tx = self.wallet.lock().await.client().get_tx(&txid)?;
        Ok(tx)
    }

    pub async fn transaction_fee(&self, txid: Txid) -> Result<Amount> {
        let fees = self
            .wallet
            .lock()
            .await
            .list_transactions(true)?
            .iter()
            .find(|tx| tx.txid == txid)
            .ok_or_else(|| {
                anyhow!("Could not find tx in bdk wallet when trying to determine fees")
            })?
            .fees;

        Ok(Amount::from_sat(fees))
    }

    pub async fn sync(&self) -> Result<()> {
        self.wallet
            .lock()
            .await
            .sync(noop_progress(), None)
            .context("Failed to sync balance of Bitcoin wallet")?;

        Ok(())
    }

    pub async fn send_to_address(
        &self,
        address: Address,
        amount: Amount,
    ) -> Result<PartiallySignedTransaction> {
        let wallet = self.wallet.lock().await;

        let mut tx_builder = wallet.build_tx();
        tx_builder.add_recipient(address.script_pubkey(), amount.as_sat());
        tx_builder.fee_rate(self.select_feerate());
        let (psbt, _details) = tx_builder.finish()?;

        Ok(psbt)
    }

    /// Calculates the maximum "giveable" amount of this wallet.
    ///
    /// We define this as the maximum amount we can pay to a single output,
    /// already accounting for the fees we need to spend to get the
    /// transaction confirmed.
    pub async fn max_giveable(&self, locking_script_size: usize) -> Result<Amount> {
        let wallet = self.wallet.lock().await;

        let mut tx_builder = wallet.build_tx();

        let dummy_script = Script::from(vec![0u8; locking_script_size]);
        tx_builder.set_single_recipient(dummy_script);
        tx_builder.drain_wallet();
        tx_builder.fee_rate(self.select_feerate());
        let (_, details) = tx_builder.finish().context("Failed to build transaction")?;

        let max_giveable = details.sent - details.fees;

        Ok(Amount::from_sat(max_giveable))
    }

    pub async fn get_network(&self) -> bitcoin::Network {
        self.wallet.lock().await.network()
    }

    /// Broadcast the given transaction to the network and emit a log statement
    /// if done so successfully.
    pub async fn broadcast(&self, transaction: Transaction, kind: &str) -> Result<Txid> {
        let txid = transaction.txid();

        self.wallet
            .lock()
            .await
            .broadcast(transaction)
            .with_context(|| {
                format!("Failed to broadcast Bitcoin {} transaction {}", kind, txid)
            })?;

        tracing::info!(%txid, "Published Bitcoin {} transaction", kind);

        Ok(txid)
    }

    pub async fn sign_and_finalize(&self, psbt: PartiallySignedTransaction) -> Result<Transaction> {
        let (signed_psbt, finalized) = self.wallet.lock().await.sign(psbt, None)?;

        if !finalized {
            bail!("PSBT is not finalized")
        }

        let tx = signed_psbt.extract_tx();

        Ok(tx)
    }

    pub async fn get_raw_transaction(&self, txid: Txid) -> Result<Transaction> {
        self.get_tx(txid)
            .await?
            .ok_or_else(|| anyhow!("Could not get raw tx with id: {}", txid))
    }

    pub async fn watch_for_raw_transaction(&self, txid: Txid) -> Result<Transaction> {
        tracing::debug!("watching for tx: {}", txid);
        let tx = retry(ConstantBackoff::new(Duration::from_secs(1)), || async {
            let client = bdk::electrum_client::Client::new(self.rpc_url.as_ref())
                .map_err(|err| backoff::Error::Permanent(Error::ElectrumClient(err)))?;

            let tx = client.transaction_get(&txid).map_err(|err| match err {
                electrum_client::Error::Protocol(err) => {
                    tracing::debug!("Received protocol error {} from Electrum, retrying...", err);
                    backoff::Error::Transient(Error::NotYetMined)
                }
                err => backoff::Error::Permanent(Error::ElectrumClient(err)),
            })?;

            Result::<_, backoff::Error<Error>>::Ok(tx)
        })
        .await
        .context("Transient errors should be retried")?;

        Ok(tx)
    }

    pub async fn get_block_height(&self) -> Result<BlockHeight> {
        let mut inner = self.client.lock().await;

        inner.ping()?;
        inner.drain_blockheight_notifications()?;

        Ok(inner.latest_block)
    }

    pub async fn transaction_block_height(&self, txid: Txid) -> Result<BlockHeight> {
        let status_url = make_tx_status_url(&self.http_url, txid)?;

        #[derive(Serialize, Deserialize, Debug, Clone)]
        struct TransactionStatus {
            block_height: Option<u32>,
            confirmed: bool,
        }
        let height = retry(ConstantBackoff::new(Duration::from_secs(1)), || async {
            let block_height = reqwest::get(status_url.clone())
                .await
                .map_err(|err| backoff::Error::Transient(Error::Io(err)))?
                .json::<TransactionStatus>()
                .await
                .map_err(|err| backoff::Error::Permanent(Error::JsonDeserialization(err)))?
                .block_height
                .ok_or(backoff::Error::Transient(Error::NotYetMined))?;

            Result::<_, backoff::Error<Error>>::Ok(block_height)
        })
        .await
        .context("Transient errors should be retried")?;

        Ok(BlockHeight::new(height))
    }

    pub async fn wait_for_transaction_finality(
        &self,
        txid: Txid,
        execution_params: ExecutionParams,
    ) -> Result<()> {
        let conf_target = execution_params.bitcoin_finality_confirmations;

        tracing::info!(%txid, "Waiting for {} confirmation{} of Bitcoin transaction", conf_target, if conf_target > 1 { "s" } else { "" });

        // Divide by 4 to not check too often yet still be aware of the new block early
        // on.
        let mut interval = interval(execution_params.bitcoin_avg_block_time / 4);

        loop {
            let tx_block_height = self.transaction_block_height(txid).await?;
            tracing::debug!("tx_block_height: {:?}", tx_block_height);

            let block_height = self.get_block_height().await?;
            tracing::debug!("latest_block_height: {:?}", block_height);

            if let Some(confirmations) = block_height.checked_sub(
                tx_block_height
                    .checked_sub(BlockHeight::new(1))
                    .expect("transaction must be included in block with height >= 1"),
            ) {
                tracing::debug!(%txid, "confirmations: {:?}", confirmations);
                if u32::from(confirmations) >= conf_target {
                    break;
                }
            }
            interval.tick().await;
        }

        Ok(())
    }

    /// Selects an appropriate [`FeeRate`] to be used for getting transactions
    /// confirmed within a reasonable amount of time.
    fn select_feerate(&self) -> FeeRate {
        // TODO: This should obviously not be a const :)
        FeeRate::from_sat_per_vb(5.0)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum ScriptStatus {
    Unseen,
    InMempool,
    Confirmed { depth: u64 },
}

fn make_tx_status_url(base_url: &Url, txid: Txid) -> Result<Url> {
    let url = base_url.join(&format!("tx/{}/status", txid))?;

    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::config::DEFAULT_ELECTRUM_HTTP_URL;

    #[test]
    fn create_tx_status_url_from_default_base_url_success() {
        let base_url = DEFAULT_ELECTRUM_HTTP_URL.parse().unwrap();
        let txid = Txid::default;

        let url = make_tx_status_url(&base_url, txid()).unwrap();

        assert_eq!(url.as_str(), "https://blockstream.info/testnet/api/tx/0000000000000000000000000000000000000000000000000000000000000000/status");
    }
}
