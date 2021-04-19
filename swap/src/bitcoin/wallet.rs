use crate::bitcoin::timelocks::BlockHeight;
use crate::bitcoin::{Address, Amount, Transaction};
use crate::env;
use ::bitcoin::util::psbt::PartiallySignedTransaction;
use ::bitcoin::Txid;
use anyhow::{bail, Context, Result};
use bdk::blockchain::{noop_progress, Blockchain, ElectrumBlockchain};
use bdk::database::BatchDatabase;
use bdk::descriptor::Segwitv0;
use bdk::electrum_client::{ElectrumApi, GetHistoryRes};
use bdk::keys::DerivableKey;
use bdk::wallet::AddressIndex;
use bdk::{FeeRate, KeychainKind};
use bitcoin::{Network, Script};
use reqwest::Url;
use std::collections::{BTreeMap, HashMap};
use std::convert::TryFrom;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{watch, Mutex};

const SLED_TREE_NAME: &str = "default_tree";

pub struct Wallet<B = ElectrumBlockchain, D = bdk::sled::Tree, C = Client> {
    client: Arc<Mutex<C>>,
    wallet: Arc<Mutex<bdk::Wallet<B, D>>>,
    finality_confirmations: u32,
    network: Network,
}

impl Wallet {
    pub async fn new(
        electrum_rpc_url: Url,
        wallet_dir: &Path,
        key: impl DerivableKey<Segwitv0> + Clone,
        env_config: env::Config,
    ) -> Result<Self> {
        let client = bdk::electrum_client::Client::new(electrum_rpc_url.as_str())
            .context("Failed to initialize Electrum RPC client")?;

        let db = bdk::sled::open(wallet_dir)?.open_tree(SLED_TREE_NAME)?;

        let wallet = bdk::Wallet::new(
            bdk::template::Bip84(key.clone(), KeychainKind::External),
            Some(bdk::template::Bip84(key, KeychainKind::Internal)),
            env_config.bitcoin_network,
            db,
            ElectrumBlockchain::from(client),
        )?;

        let electrum = bdk::electrum_client::Client::new(electrum_rpc_url.as_str())
            .context("Failed to initialize Electrum RPC client")?;

        let network = wallet.network();

        Ok(Self {
            client: Arc::new(Mutex::new(Client::new(
                electrum,
                env_config.bitcoin_sync_interval(),
            )?)),
            wallet: Arc::new(Mutex::new(wallet)),
            finality_confirmations: env_config.bitcoin_finality_confirmations,
            network,
        })
    }

    /// Broadcast the given transaction to the network and emit a log statement
    /// if done so successfully.
    ///
    /// Returns the transaction ID and a future for when the transaction meets
    /// the configured finality confirmations.
    pub async fn broadcast(
        &self,
        transaction: Transaction,
        kind: &str,
    ) -> Result<(Txid, Subscription)> {
        let txid = transaction.txid();

        // to watch for confirmations, watching a single output is enough
        let subscription = self
            .subscribe_to((txid, transaction.output[0].script_pubkey.clone()))
            .await;

        self.wallet
            .lock()
            .await
            .broadcast(transaction)
            .with_context(|| {
                format!("Failed to broadcast Bitcoin {} transaction {}", kind, txid)
            })?;

        tracing::info!(%txid, "Published Bitcoin {} transaction", kind);

        Ok((txid, subscription))
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
            .with_context(|| format!("Could not get raw tx with id: {}", txid))
    }

    pub async fn status_of_script<T>(&self, tx: &T) -> Result<ScriptStatus>
    where
        T: Watchable,
    {
        self.client.lock().await.status_of_script(tx)
    }

    pub async fn subscribe_to(&self, tx: impl Watchable + Send + 'static) -> Subscription {
        let txid = tx.id();
        let script = tx.script();

        let sub = self
            .client
            .lock()
            .await
            .subscriptions
            .entry((txid, script.clone()))
            .or_insert_with(|| {
                let (sender, receiver) = watch::channel(ScriptStatus::Unseen);
                let client = self.client.clone();

                tokio::spawn(async move {
                    let mut last_status = None;

                    loop {
                        tokio::time::sleep(Duration::from_secs(5)).await;

                        let new_status = match client.lock().await.status_of_script(&tx) {
                            Ok(new_status) => new_status,
                            Err(e) => {
                                tracing::warn!(%txid, "Failed to get status of script: {:#}", e);
                                return;
                            }
                        };

                        if Some(new_status) != last_status {
                            tracing::debug!(%txid, "Transaction is {}", new_status);
                        }
                        last_status = Some(new_status);

                        let all_receivers_gone = sender.send(new_status).is_err();

                        if all_receivers_gone {
                            tracing::debug!(%txid, "All receivers gone, removing subscription");
                            client.lock().await.subscriptions.remove(&(txid, script));
                            return;
                        }
                    }
                });

                Subscription {
                    receiver,
                    finality_confirmations: self.finality_confirmations,
                    txid,
                }
            })
            .clone();

        sub
    }
}

/// Represents a subscription to the status of a given transaction.
#[derive(Debug, Clone)]
pub struct Subscription {
    receiver: watch::Receiver<ScriptStatus>,
    finality_confirmations: u32,
    txid: Txid,
}

impl Subscription {
    pub async fn wait_until_final(&self) -> Result<()> {
        let conf_target = self.finality_confirmations;
        let txid = self.txid;

        tracing::info!(%txid, "Waiting for {} confirmation{} of Bitcoin transaction", conf_target, if conf_target > 1 { "s" } else { "" });

        let mut seen_confirmations = 0;

        self.wait_until(|status| match status {
            ScriptStatus::Confirmed(inner) => {
                let confirmations = inner.confirmations();

                if confirmations > seen_confirmations {
                    tracing::info!(%txid, "Bitcoin tx has {} out of {} confirmation{}", confirmations, conf_target, if conf_target > 1 { "s" } else { "" });
                    seen_confirmations = confirmations;
                }

                inner.meets_target(conf_target)
            },
            _ => false
        })
            .await
    }

    pub async fn wait_until_seen(&self) -> Result<()> {
        self.wait_until(ScriptStatus::has_been_seen).await
    }

    pub async fn wait_until_confirmed_with<T>(&self, target: T) -> Result<()>
    where
        u32: PartialOrd<T>,
        T: Copy,
    {
        self.wait_until(|status| status.is_confirmed_with(target))
            .await
    }

    async fn wait_until(&self, mut predicate: impl FnMut(&ScriptStatus) -> bool) -> Result<()> {
        let mut receiver = self.receiver.clone();

        while !predicate(&receiver.borrow()) {
            receiver
                .changed()
                .await
                .context("Failed while waiting for next status update")?;
        }

        Ok(())
    }
}

impl<B, D, C> Wallet<B, D, C>
where
    D: BatchDatabase,
{
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
            .get_address(AddressIndex::New)
            .context("Failed to get new Bitcoin address")?;

        Ok(address)
    }

    pub async fn transaction_fee(&self, txid: Txid) -> Result<Amount> {
        let fees = self
            .wallet
            .lock()
            .await
            .list_transactions(true)?
            .iter()
            .find(|tx| tx.txid == txid)
            .context("Could not find tx in bdk wallet when trying to determine fees")?
            .fees;

        Ok(Amount::from_sat(fees))
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
}

impl<B, D, C> Wallet<B, D, C>
where
    B: Blockchain,
    D: BatchDatabase,
{
    pub async fn get_tx(&self, txid: Txid) -> Result<Option<Transaction>> {
        let tx = self.wallet.lock().await.client().get_tx(&txid)?;

        Ok(tx)
    }

    pub async fn sync(&self) -> Result<()> {
        self.wallet
            .lock()
            .await
            .sync(noop_progress(), None)
            .context("Failed to sync balance of Bitcoin wallet")?;

        Ok(())
    }
}

impl<B, D, C> Wallet<B, D, C> {
    // TODO: Get rid of this by changing bounds on bdk::Wallet
    pub fn get_network(&self) -> bitcoin::Network {
        self.network
    }

    /// Selects an appropriate [`FeeRate`] to be used for getting transactions
    /// confirmed within a reasonable amount of time.
    fn select_feerate(&self) -> FeeRate {
        // TODO: This should obviously not be a const :)
        FeeRate::from_sat_per_vb(5.0)
    }
}

#[cfg(test)]
impl Wallet<(), bdk::database::MemoryDatabase, ()> {
    /// Creates a new, funded wallet to be used within tests.
    pub fn new_funded(amount: u64) -> Self {
        use bdk::database::MemoryDatabase;
        use bdk::{LocalUtxo, TransactionDetails};
        use bitcoin::OutPoint;
        use std::str::FromStr;
        use testutils::testutils;

        let descriptors = testutils!(@descriptors ("wpkh(tpubEBr4i6yk5nf5DAaJpsi9N2pPYBeJ7fZ5Z9rmN4977iYLCGco1VyjB9tvvuvYtfZzjD5A8igzgw3HeWeeKFmanHYqksqZXYXGsw5zjnj7KM9/*)"));

        let mut database = MemoryDatabase::new();
        bdk::populate_test_db!(
            &mut database,
            testutils! {
                @tx ( (@external descriptors, 0) => amount ) (@confirmations 1)
            },
            Some(100)
        );

        let wallet =
            bdk::Wallet::new_offline(&descriptors.0, None, Network::Regtest, database).unwrap();

        Self {
            client: Arc::new(Mutex::new(())),
            wallet: Arc::new(Mutex::new(wallet)),
            finality_confirmations: 1,
            network: Network::Regtest,
        }
    }
}

/// Defines a watchable transaction.
///
/// For a transaction to be watchable, we need to know two things: Its
/// transaction ID and the specific output script that is going to change.
/// A transaction can obviously have multiple outputs but our protocol purposes,
/// we are usually interested in a specific one.
pub trait Watchable {
    fn id(&self) -> Txid;
    fn script(&self) -> Script;
}

impl Watchable for (Txid, Script) {
    fn id(&self) -> Txid {
        self.0
    }

    fn script(&self) -> Script {
        self.1.clone()
    }
}

pub struct Client {
    electrum: bdk::electrum_client::Client,
    latest_block: BlockHeight,
    last_ping: Instant,
    interval: Duration,
    script_history: BTreeMap<Script, Vec<GetHistoryRes>>,
    subscriptions: HashMap<(Txid, Script), Subscription>,
}

impl Client {
    fn new(electrum: bdk::electrum_client::Client, interval: Duration) -> Result<Self> {
        let latest_block = electrum
            .block_headers_subscribe()
            .context("Failed to subscribe to header notifications")?;

        Ok(Self {
            electrum,
            latest_block: BlockHeight::try_from(latest_block)?,
            last_ping: Instant::now(),
            interval,
            script_history: Default::default(),
            subscriptions: Default::default(),
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
        self.update_script_histories()?;

        Ok(())
    }

    fn status_of_script<T>(&mut self, tx: &T) -> Result<ScriptStatus>
    where
        T: Watchable,
    {
        let txid = tx.id();
        let script = tx.script();

        if !self.script_history.contains_key(&script) {
            self.script_history.insert(script.clone(), vec![]);
        }

        self.drain_notifications()?;

        let history = self.script_history.entry(script).or_default();

        let history_of_tx = history
            .iter()
            .filter(|entry| entry.tx_hash == txid)
            .collect::<Vec<_>>();

        match history_of_tx.as_slice() {
            [] => Ok(ScriptStatus::Unseen),
            [remaining @ .., last] => {
                if !remaining.is_empty() {
                    tracing::warn!("Found more than a single history entry for script. This is highly unexpected and those history entries will be ignored.")
                }

                if last.height <= 0 {
                    Ok(ScriptStatus::InMempool)
                } else {
                    Ok(ScriptStatus::Confirmed(
                        Confirmed::from_inclusion_and_latest_block(
                            u32::try_from(last.height)?,
                            u32::from(self.latest_block),
                        ),
                    ))
                }
            }
        }
    }

    fn drain_blockheight_notifications(&mut self) -> Result<()> {
        let latest_block = std::iter::from_fn(|| self.electrum.block_headers_pop().transpose())
            .last()
            .transpose()
            .context("Failed to pop header notification")?;

        if let Some(new_block) = latest_block {
            tracing::debug!(
                "Got notification for new block at height {}",
                new_block.height
            );
            self.latest_block = BlockHeight::try_from(new_block)?;
        }

        Ok(())
    }

    fn update_script_histories(&mut self) -> Result<()> {
        let histories = self
            .electrum
            .batch_script_get_history(self.script_history.keys())
            .context("Failed to get script histories")?;

        if histories.len() != self.script_history.len() {
            bail!(
                "Expected {} history entries, received {}",
                self.script_history.len(),
                histories.len()
            );
        }

        let scripts = self.script_history.keys().cloned();
        let histories = histories.into_iter();

        self.script_history = scripts.zip(histories).collect::<BTreeMap<_, _>>();

        Ok(())
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ScriptStatus {
    Unseen,
    InMempool,
    Confirmed(Confirmed),
}

impl ScriptStatus {
    pub fn from_confirmations(confirmations: u32) -> Self {
        match confirmations {
            0 => Self::InMempool,
            confirmations => Self::Confirmed(Confirmed::new(confirmations - 1)),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Confirmed {
    /// The depth of this transaction within the blockchain.
    ///
    /// Will be zero if the transaction is included in the latest block.
    depth: u32,
}

impl Confirmed {
    pub fn new(depth: u32) -> Self {
        Self { depth }
    }

    /// Compute the depth of a transaction based on its inclusion height and the
    /// latest known block.
    ///
    /// Our information about the latest block might be outdated. To avoid an
    /// overflow, we make sure the depth is 0 in case the inclusion height
    /// exceeds our latest known block,
    pub fn from_inclusion_and_latest_block(inclusion_height: u32, latest_block: u32) -> Self {
        let depth = latest_block.saturating_sub(inclusion_height);

        Self { depth }
    }

    pub fn confirmations(&self) -> u32 {
        self.depth + 1
    }

    pub fn meets_target<T>(&self, target: T) -> bool
    where
        u32: PartialOrd<T>,
    {
        self.confirmations() >= target
    }
}

impl ScriptStatus {
    /// Check if the script has any confirmations.
    pub fn is_confirmed(&self) -> bool {
        matches!(self, ScriptStatus::Confirmed(_))
    }

    /// Check if the script has met the given confirmation target.
    pub fn is_confirmed_with<T>(&self, target: T) -> bool
    where
        u32: PartialOrd<T>,
    {
        match self {
            ScriptStatus::Confirmed(inner) => inner.meets_target(target),
            _ => false,
        }
    }

    pub fn has_been_seen(&self) -> bool {
        matches!(self, ScriptStatus::InMempool | ScriptStatus::Confirmed(_))
    }
}

impl fmt::Display for ScriptStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScriptStatus::Unseen => write!(f, "unseen"),
            ScriptStatus::InMempool => write!(f, "in mempool"),
            ScriptStatus::Confirmed(inner) => {
                write!(f, "confirmed with {} blocks", inner.confirmations())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_depth_0_should_meet_confirmation_target_one() {
        let script = ScriptStatus::Confirmed(Confirmed { depth: 0 });

        let confirmed = script.is_confirmed_with(1);

        assert!(confirmed)
    }

    #[test]
    fn given_confirmations_1_should_meet_confirmation_target_one() {
        let script = ScriptStatus::from_confirmations(1);

        let confirmed = script.is_confirmed_with(1);

        assert!(confirmed)
    }

    #[test]
    fn given_inclusion_after_lastest_known_block_at_least_depth_0() {
        let included_in = 10;
        let latest_block = 9;

        let confirmed = Confirmed::from_inclusion_and_latest_block(included_in, latest_block);

        assert_eq!(confirmed.depth, 0)
    }
}
