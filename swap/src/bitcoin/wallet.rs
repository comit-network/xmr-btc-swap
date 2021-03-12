use crate::bitcoin::timelocks::BlockHeight;
use crate::bitcoin::{Address, Amount, Transaction};
use crate::execution_params::ExecutionParams;
use ::bitcoin::util::psbt::PartiallySignedTransaction;
use ::bitcoin::Txid;
use anyhow::{anyhow, bail, Context, Result};
use bdk::blockchain::{noop_progress, Blockchain, ElectrumBlockchain};
use bdk::descriptor::Segwitv0;
use bdk::electrum_client::{self, ElectrumApi, GetHistoryRes};
use bdk::keys::DerivableKey;
use bdk::{FeeRate, KeychainKind};
use bitcoin::Script;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const SLED_TREE_NAME: &str = "default_tree";

pub struct Wallet {
    client: Arc<Mutex<Client>>,
    wallet: Arc<Mutex<bdk::Wallet<ElectrumBlockchain, bdk::sled::Tree>>>,
    http_url: Url,
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

        let interval = Duration::from_secs(5);

        Ok(Self {
            wallet: Arc::new(Mutex::new(bdk_wallet)),
            client: Arc::new(Mutex::new(Client::new(electrum, interval)?)),
            http_url: electrum_http_url,
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

    pub async fn watch_until_status(
        &self,
        txid: Txid,
        script: Script,
        mut status_fn: impl FnMut(ScriptStatus) -> bool,
    ) -> Result<()> {
        {
            let mut client = self.client.lock().await;
            client.subscribe_to_script(script.clone())?;
        }

        loop {
            let status = self.client.lock().await.status_of_script(&script, &txid)?;

            tracing::debug!("Transaction {} is {}", txid, status);

            if status_fn(status) {
                break;
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }

        // TODO: Unsubscribe using the client? Or at least clear our local data, we
        // should never get a notification again.

        Ok(())
    }

    pub async fn get_block_height(&self) -> Result<BlockHeight> {
        let mut inner = self.client.lock().await;

        inner.drain_notifications()?;

        Ok(inner.latest_block)
    }

    pub async fn transaction_block_height(&self, txid: Txid) -> Result<Option<BlockHeight>> {
        let status_url = make_tx_status_url(&self.http_url, txid)?;

        #[derive(Serialize, Deserialize, Debug, Clone)]
        struct TransactionStatus {
            block_height: Option<u32>,
            confirmed: bool,
        }

        let block_height = reqwest::get(status_url.clone())
            .await
            .context("Failed to send request")?
            .json::<TransactionStatus>()
            .await
            .context("Failed to deserialize response as TransactionStatus")?
            .block_height;

        Ok(block_height.map(BlockHeight::new))
    }

    pub async fn wait_for_transaction_finality(
        &self,
        txid: Txid,
        script_to_watch: Script,
        execution_params: ExecutionParams,
    ) -> Result<()> {
        let conf_target = execution_params.bitcoin_finality_confirmations;

        tracing::info!(%txid, "Waiting for {} confirmation{} of Bitcoin transaction", conf_target, if conf_target > 1 { "s" } else { "" });

        let mut seen_confirmations = 0;

        self.watch_until_status(txid, script_to_watch, |status| match status {
            ScriptStatus::Confirmed { depth } => {
                if depth > seen_confirmations {
                    tracing::info!(%txid, "Bitcoin tx has {} out of {} confirmation{}", depth, conf_target, if conf_target > 1 { "s" } else { "" });
                    seen_confirmations = depth;
                }

                depth >= conf_target
            },
            _ => false
        })
        .await?;

        Ok(())
    }

    /// Selects an appropriate [`FeeRate`] to be used for getting transactions
    /// confirmed within a reasonable amount of time.
    fn select_feerate(&self) -> FeeRate {
        // TODO: This should obviously not be a const :)
        FeeRate::from_sat_per_vb(5.0)
    }
}

struct Client {
    electrum: bdk::electrum_client::Client,
    latest_block: BlockHeight,
    last_ping: Instant,
    interval: Duration,
    script_history: HashMap<Script, Vec<GetHistoryRes>>,
}

impl Client {
    fn new(electrum: bdk::electrum_client::Client, interval: Duration) -> Result<Self> {
        let latest_block = electrum.block_headers_subscribe().map_err(|e| {
            anyhow!(
                "Electrum client failed to subscribe to header notifications: {:?}",
                e
            )
        })?;

        Ok(Self {
            electrum,
            latest_block: BlockHeight::try_from(latest_block)?,
            last_ping: Instant::now(),
            interval,
            script_history: HashMap::default(),
        })
    }

    /// Ping the electrum server unless we already did within the set interval.
    ///
    /// Returns a boolean indicating whether we actually pinged the server.
    fn ping(&mut self) -> bool {
        if self.last_ping.elapsed() <= self.interval {
            return false;
        }

        match self.electrum.ping() {
            Ok(()) => {
                self.last_ping = Instant::now();

                true
            }
            Err(error) => {
                tracing::debug!(?error, "Failed to ping electrum server");

                false
            }
        }
    }

    fn drain_notifications(&mut self) -> Result<()> {
        let pinged = self.ping();

        if !pinged {
            return Ok(());
        }

        self.drain_blockheight_notifications()?;
        self.drain_script_notifications()?;

        Ok(())
    }

    fn subscribe_to_script(&mut self, script: Script) -> Result<()> {
        if self.script_history.contains_key(&script) {
            return Ok(());
        }

        let _status = self
            .electrum
            .script_subscribe(&script)
            .map_err(|e| anyhow!("Failed to subscribe to script notifications: {:?}", e))?;

        self.script_history.insert(script, Vec::new());

        Ok(())
    }

    fn status_of_script(&mut self, script: &Script, txid: &Txid) -> Result<ScriptStatus> {
        self.drain_notifications()?;

        let history = self.script_history.entry(script.clone()).or_default();

        let history_of_tx = history
            .iter()
            .filter(|entry| &entry.tx_hash == txid)
            .collect::<Vec<_>>();

        match history_of_tx.as_slice() {
            [] => Ok(ScriptStatus::Unseen),
            [single, remaining @ ..] => {
                if !remaining.is_empty() {
                    tracing::warn!("Found more than a single history entry for script. This is highly unexpected and those history entries will be ignored.")
                }

                if single.height <= 0 {
                    Ok(ScriptStatus::InMempool)
                } else {
                    Ok(ScriptStatus::Confirmed {
                        depth: u32::from(self.latest_block) - u32::try_from(single.height)?,
                    })
                }
            }
        }
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

    fn drain_script_notifications(&mut self) -> Result<()> {
        let script_history = &mut self.script_history;
        let electrum = &self.electrum;

        for (script, history) in script_history.iter_mut() {
            if std::iter::from_fn(|| electrum.script_pop(script).transpose())
                .last()
                .transpose()
                .map_err(|e| anyhow!("Failed to pop script notification: {:?}", e))?
                .is_some()
            {
                let new_history = electrum
                    .script_get_history(script)
                    .map_err(|e| anyhow!("Failed to get to script history: {:?}", e))?;

                *history = new_history;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Copy, Clone)]
pub enum ScriptStatus {
    Unseen,
    InMempool,
    Confirmed { depth: u32 },
}

impl fmt::Display for ScriptStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScriptStatus::Unseen => write!(f, "unseen"),
            ScriptStatus::InMempool => write!(f, "in mempool"),
            ScriptStatus::Confirmed { depth } => write!(f, "confirmed with {} blocks", depth),
        }
    }
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
