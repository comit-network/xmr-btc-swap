use crate::bitcoin::timelocks::BlockHeight;
use crate::bitcoin::{Address, Amount, Transaction};
use crate::env;
use ::bitcoin::util::psbt::PartiallySignedTransaction;
use ::bitcoin::Txid;
use anyhow::{bail, Context, Result};
use bdk::blockchain::{Blockchain, ElectrumBlockchain, GetTx};
use bdk::database::BatchDatabase;
use bdk::electrum_client::{ElectrumApi, GetHistoryRes};
use bdk::sled::Tree;
use bdk::wallet::export::FullyNodedExport;
use bdk::wallet::AddressIndex;
use bdk::{FeeRate, KeychainKind, SignOptions, SyncOptions};
use bitcoin::util::bip32::ExtendedPrivKey;
use bitcoin::{Network, Script};
use reqwest::Url;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::{BTreeMap, HashMap};
use std::convert::TryFrom;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{watch, Mutex};
use tracing::{debug_span, Instrument};

const SLED_TREE_NAME: &str = "default_tree";

/// Assuming we add a spread of 3% we don't want to pay more than 3% of the
/// amount for tx fees.
const MAX_RELATIVE_TX_FEE: Decimal = dec!(0.03);
const MAX_ABSOLUTE_TX_FEE: Decimal = dec!(100_000);
const DUST_AMOUNT: u64 = 546;

const WALLET: &str = "wallet";
const WALLET_OLD: &str = "wallet-old";

pub struct Wallet<D = Tree, C = Client> {
    client: Arc<Mutex<C>>,
    wallet: Arc<Mutex<bdk::Wallet<D>>>,
    finality_confirmations: u32,
    network: Network,
    target_block: usize,
}

impl Wallet {
    pub async fn new(
        electrum_rpc_url: Url,
        data_dir: impl AsRef<Path>,
        xprivkey: ExtendedPrivKey,
        env_config: env::Config,
        target_block: usize,
    ) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        let wallet_dir = data_dir.join(WALLET);
        let database = bdk::sled::open(wallet_dir)?.open_tree(SLED_TREE_NAME)?;
        let network = env_config.bitcoin_network;

        let wallet = match bdk::Wallet::new(
            bdk::template::Bip84(xprivkey, KeychainKind::External),
            Some(bdk::template::Bip84(xprivkey, KeychainKind::Internal)),
            network,
            database,
        ) {
            Ok(w) => w,
            Err(bdk::Error::ChecksumMismatch) => Self::migrate(data_dir, xprivkey, network)?,
            err => err?,
        };

        let client = Client::new(electrum_rpc_url, env_config.bitcoin_sync_interval())?;

        let network = wallet.network();

        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            wallet: Arc::new(Mutex::new(wallet)),
            finality_confirmations: env_config.bitcoin_finality_confirmations,
            network,
            target_block,
        })
    }

    /// Create a new database for the wallet and rename the old one.
    /// This is necessary when getting a ChecksumMismatch from a wallet
    /// created with an older version of BDK. Only affected Testnet wallets.
    // https://github.com/comit-network/xmr-btc-swap/issues/1182
    fn migrate(
        data_dir: &Path,
        xprivkey: ExtendedPrivKey,
        network: bitcoin::Network,
    ) -> Result<bdk::Wallet<Tree>> {
        let from = data_dir.join(WALLET);
        let to = data_dir.join(WALLET_OLD);
        std::fs::rename(from, to)?;

        let wallet_dir = data_dir.join(WALLET);
        let database = bdk::sled::open(wallet_dir)?.open_tree(SLED_TREE_NAME)?;

        let wallet = bdk::Wallet::new(
            bdk::template::Bip84(xprivkey, KeychainKind::External),
            Some(bdk::template::Bip84(xprivkey, KeychainKind::Internal)),
            network,
            database,
        )?;

        Ok(wallet)
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

        let client = self.client.lock().await;
        let blockchain = client.blockchain();

        blockchain.broadcast(&transaction).with_context(|| {
            format!("Failed to broadcast Bitcoin {} transaction {}", kind, txid)
        })?;

        tracing::info!(%txid, %kind, "Published Bitcoin transaction");

        Ok((txid, subscription))
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
                        let new_status = match client.lock().await.status_of_script(&tx) {
                            Ok(new_status) => new_status,
                            Err(error) => {
                                tracing::warn!(%txid, "Failed to get status of script: {:#}", error);
                                ScriptStatus::Retrying
                            }
                        };

                        if new_status != ScriptStatus::Retrying
                        {
                            last_status = Some(print_status_change(txid, last_status, new_status));

                            let all_receivers_gone = sender.send(new_status).is_err();

                            if all_receivers_gone {
                                tracing::debug!(%txid, "All receivers gone, removing subscription");
                                client.lock().await.subscriptions.remove(&(txid, script));
                                return;
                            }
                        }

                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }.instrument(debug_span!("BitcoinWalletSubscription")));

                Subscription {
                    receiver,
                    finality_confirmations: self.finality_confirmations,
                    txid,
                }
            })
            .clone();

        sub
    }

    pub async fn wallet_export(&self, role: &str) -> Result<FullyNodedExport> {
        let wallet = self.wallet.lock().await;
        match bdk::wallet::export::FullyNodedExport::export_wallet(
            &wallet,
            &format!("{}-{}", role, self.network),
            true,
        ) {
            Ok(wallet_export) => Ok(wallet_export),
            Err(err_msg) => Err(anyhow::Error::msg(err_msg)),
        }
    }
}

fn print_status_change(txid: Txid, old: Option<ScriptStatus>, new: ScriptStatus) -> ScriptStatus {
    match (old, new) {
        (None, new_status) => {
            tracing::debug!(%txid, status = %new_status, "Found relevant Bitcoin transaction");
        }
        (Some(old_status), new_status) if old_status != new_status => {
            tracing::debug!(%txid, %new_status, %old_status, "Bitcoin transaction status changed");
        }
        _ => {}
    }

    new
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

        tracing::info!(%txid, required_confirmation=%conf_target, "Waiting for Bitcoin transaction finality");

        let mut seen_confirmations = 0;

        self.wait_until(|status| match status {
            ScriptStatus::Confirmed(inner) => {
                let confirmations = inner.confirmations();

                if confirmations > seen_confirmations {
                    tracing::info!(%txid,
                        seen_confirmations = %confirmations,
                        needed_confirmations = %conf_target,
                        "Waiting for Bitcoin transaction finality");
                    seen_confirmations = confirmations;
                }

                inner.meets_target(conf_target)
            }
            _ => false,
        })
        .await
    }

    pub async fn wait_until_seen(&self) -> Result<()> {
        self.wait_until(ScriptStatus::has_been_seen).await
    }

    pub async fn wait_until_confirmed_with<T>(&self, target: T) -> Result<()>
    where
        T: Into<u32>,
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

impl<D, C> Wallet<D, C>
where
    C: EstimateFeeRate,
    D: BatchDatabase,
{
    pub async fn sign_and_finalize(
        &self,
        mut psbt: PartiallySignedTransaction,
    ) -> Result<Transaction> {
        let finalized = self
            .wallet
            .lock()
            .await
            .sign(&mut psbt, SignOptions::default())?;

        if !finalized {
            bail!("PSBT is not finalized")
        }

        let tx = psbt.extract_tx();

        Ok(tx)
    }

    /// Returns the total Bitcoin balance, which includes pending funds
    pub async fn balance(&self) -> Result<Amount> {
        let balance = self
            .wallet
            .lock()
            .await
            .get_balance()
            .context("Failed to calculate Bitcoin balance")?;

        Ok(Amount::from_sat(balance.get_total()))
    }

    pub async fn new_address(&self) -> Result<Address> {
        let address = self
            .wallet
            .lock()
            .await
            .get_address(AddressIndex::New)
            .context("Failed to get new Bitcoin address")?
            .address;

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
            .fee
            .expect("fees are always present with Electrum backend");

        Ok(Amount::from_sat(fees))
    }

    /// Builds a partially signed transaction
    ///
    /// Ensures that the address script is at output index `0`
    /// for the partially signed transaction.
    pub async fn send_to_address(
        &self,
        address: Address,
        amount: Amount,
        change_override: Option<Address>,
    ) -> Result<PartiallySignedTransaction> {
        if self.network != address.network {
            bail!("Cannot build PSBT because network of given address is {} but wallet is on network {}", address.network, self.network);
        }

        if let Some(change) = change_override.as_ref() {
            if self.network != change.network {
                bail!("Cannot build PSBT because network of given address is {} but wallet is on network {}", change.network, self.network);
            }
        }

        let wallet = self.wallet.lock().await;
        let client = self.client.lock().await;
        let fee_rate = client.estimate_feerate(self.target_block)?;
        let script = address.script_pubkey();

        let mut tx_builder = wallet.build_tx();
        tx_builder.add_recipient(script.clone(), amount.to_sat());
        tx_builder.fee_rate(fee_rate);
        let (psbt, _details) = tx_builder.finish()?;
        let mut psbt: PartiallySignedTransaction = psbt;

        match psbt.unsigned_tx.output.as_mut_slice() {
            // our primary output is the 2nd one? reverse the vectors
            [_, second_txout] if second_txout.script_pubkey == script => {
                psbt.outputs.reverse();
                psbt.unsigned_tx.output.reverse();
            }
            [first_txout, _] if first_txout.script_pubkey == script => {
                // no need to do anything
            }
            [_] => {
                // single output, no need do anything
            }
            _ => bail!("Unexpected transaction layout"),
        }

        if let ([_, change], [_, psbt_output], Some(change_override)) = (
            &mut psbt.unsigned_tx.output.as_mut_slice(),
            &mut psbt.outputs.as_mut_slice(),
            change_override,
        ) {
            change.script_pubkey = change_override.script_pubkey();
            // Might be populated based on the previously set change address, but for the
            // overwrite we don't know unless we ask the user for more information.
            psbt_output.bip32_derivation.clear();
        }

        Ok(psbt)
    }

    /// Calculates the maximum "giveable" amount of this wallet.
    ///
    /// We define this as the maximum amount we can pay to a single output,
    /// already accounting for the fees we need to spend to get the
    /// transaction confirmed.
    pub async fn max_giveable(&self, locking_script_size: usize) -> Result<Amount> {
        let wallet = self.wallet.lock().await;
        let balance = wallet.get_balance()?;
        if balance.get_total() < DUST_AMOUNT {
            return Ok(Amount::ZERO);
        }
        let client = self.client.lock().await;
        let min_relay_fee = client.min_relay_fee()?.to_sat();

        if balance.get_total() < min_relay_fee {
            return Ok(Amount::ZERO);
        }

        let fee_rate = client.estimate_feerate(self.target_block)?;

        let mut tx_builder = wallet.build_tx();

        let dummy_script = Script::from(vec![0u8; locking_script_size]);
        tx_builder.drain_to(dummy_script);
        tx_builder.fee_rate(fee_rate);
        tx_builder.drain_wallet();

        let response = tx_builder.finish();
        match response {
            Ok((_, details)) => {
                let max_giveable = details.sent
                    - details
                        .fee
                        .expect("fees are always present with Electrum backend");
                Ok(Amount::from_sat(max_giveable))
            }
            Err(bdk::Error::InsufficientFunds { .. }) => Ok(Amount::ZERO),
            Err(e) => bail!("Failed to build transaction. {:#}", e),
        }
    }

    /// Estimate total tx fee for a pre-defined target block based on the
    /// transaction weight. The max fee cannot be more than MAX_PERCENTAGE_FEE
    /// of amount
    pub async fn estimate_fee(
        &self,
        weight: usize,
        transfer_amount: bitcoin::Amount,
    ) -> Result<bitcoin::Amount> {
        let client = self.client.lock().await;
        let fee_rate = client.estimate_feerate(self.target_block)?;
        let min_relay_fee = client.min_relay_fee()?;

        estimate_fee(weight, transfer_amount, fee_rate, min_relay_fee)
    }
}

fn estimate_fee(
    weight: usize,
    transfer_amount: Amount,
    fee_rate: FeeRate,
    min_relay_fee: Amount,
) -> Result<Amount> {
    if transfer_amount.to_sat() <= 546 {
        bail!("Amounts needs to be greater than Bitcoin dust amount.")
    }
    let fee_rate_svb = fee_rate.as_sat_per_vb();
    if fee_rate_svb <= 0.0 {
        bail!("Fee rate needs to be > 0")
    }
    if fee_rate_svb > 100_000_000.0 || min_relay_fee.to_sat() > 100_000_000 {
        bail!("A fee_rate or min_relay_fee of > 1BTC does not make sense")
    }

    let min_relay_fee = if min_relay_fee.to_sat() == 0 {
        // if min_relay_fee is 0 we don't fail, we just set it to 1 satoshi;
        Amount::ONE_SAT
    } else {
        min_relay_fee
    };

    let weight = Decimal::from(weight);
    let weight_factor = dec!(4.0);
    let fee_rate = Decimal::from_f32(fee_rate_svb).context("Failed to parse fee rate")?;

    let sats_per_vbyte = weight / weight_factor * fee_rate;

    tracing::debug!(
        %weight,
        %fee_rate,
        %sats_per_vbyte,
        "Estimated fee for transaction",
    );

    let transfer_amount = Decimal::from(transfer_amount.to_sat());
    let max_allowed_fee = transfer_amount * MAX_RELATIVE_TX_FEE;
    let min_relay_fee = Decimal::from(min_relay_fee.to_sat());

    let recommended_fee = if sats_per_vbyte < min_relay_fee {
        tracing::warn!(
            "Estimated fee of {} is smaller than the min relay fee, defaulting to min relay fee {}",
            sats_per_vbyte,
            min_relay_fee
        );
        min_relay_fee.to_u64()
    } else if sats_per_vbyte > max_allowed_fee && sats_per_vbyte > MAX_ABSOLUTE_TX_FEE {
        tracing::warn!(
            "Hard bound of transaction fees reached. Falling back to: {} sats",
            MAX_ABSOLUTE_TX_FEE
        );
        MAX_ABSOLUTE_TX_FEE.to_u64()
    } else if sats_per_vbyte > max_allowed_fee {
        tracing::warn!(
            "Relative bound of transaction fees reached. Falling back to: {} sats",
            max_allowed_fee
        );
        max_allowed_fee.to_u64()
    } else {
        sats_per_vbyte.to_u64()
    };
    let amount = recommended_fee
        .map(bitcoin::Amount::from_sat)
        .context("Could not estimate tranasction fee.")?;

    Ok(amount)
}

impl<D> Wallet<D>
where
    D: BatchDatabase,
{
    pub async fn get_tx(&self, txid: Txid) -> Result<Option<Transaction>> {
        let client = self.client.lock().await;
        let tx = client.get_tx(&txid)?;

        Ok(tx)
    }

    pub async fn sync(&self) -> Result<()> {
        let client = self.client.lock().await;
        let blockchain = client.blockchain();
        let sync_opts = SyncOptions::default();
        self.wallet
            .lock()
            .await
            .sync(blockchain, sync_opts)
            .context("Failed to sync balance of Bitcoin wallet")?;

        Ok(())
    }
}

impl<D, C> Wallet<D, C> {
    // TODO: Get rid of this by changing bounds on bdk::Wallet
    pub fn get_network(&self) -> bitcoin::Network {
        self.network
    }
}

pub trait EstimateFeeRate {
    fn estimate_feerate(&self, target_block: usize) -> Result<FeeRate>;
    fn min_relay_fee(&self) -> Result<bitcoin::Amount>;
}

#[cfg(test)]
pub struct StaticFeeRate {
    fee_rate: FeeRate,
    min_relay_fee: bitcoin::Amount,
}

#[cfg(test)]
impl EstimateFeeRate for StaticFeeRate {
    fn estimate_feerate(&self, _target_block: usize) -> Result<FeeRate> {
        Ok(self.fee_rate)
    }

    fn min_relay_fee(&self) -> Result<bitcoin::Amount> {
        Ok(self.min_relay_fee)
    }
}

#[cfg(test)]
#[derive(Debug)]
pub struct WalletBuilder {
    utxo_amount: u64,
    sats_per_vb: f32,
    min_relay_fee_sats: u64,
    key: bitcoin::util::bip32::ExtendedPrivKey,
    num_utxos: u8,
}

#[cfg(test)]
impl WalletBuilder {
    /// Creates a new, funded wallet with sane default fees.
    ///
    /// Unless you are testing things related to fees, this is likely what you
    /// want.
    pub fn new(amount: u64) -> Self {
        WalletBuilder {
            utxo_amount: amount,
            sats_per_vb: 1.0,
            min_relay_fee_sats: 1000,
            key: "tprv8ZgxMBicQKsPeZRHk4rTG6orPS2CRNFX3njhUXx5vj9qGog5ZMH4uGReDWN5kCkY3jmWEtWause41CDvBRXD1shKknAMKxT99o9qUTRVC6m".parse().unwrap(),
            num_utxos: 1,
        }
    }

    pub fn with_zero_fees(self) -> Self {
        Self {
            sats_per_vb: 0.0,
            min_relay_fee_sats: 0,
            ..self
        }
    }

    pub fn with_fees(self, sats_per_vb: f32, min_relay_fee_sats: u64) -> Self {
        Self {
            sats_per_vb,
            min_relay_fee_sats,
            ..self
        }
    }

    pub fn with_key(self, key: bitcoin::util::bip32::ExtendedPrivKey) -> Self {
        Self { key, ..self }
    }

    pub fn with_num_utxos(self, number: u8) -> Self {
        Self {
            num_utxos: number,
            ..self
        }
    }

    pub fn build(self) -> Wallet<bdk::database::MemoryDatabase, StaticFeeRate> {
        use bdk::database::{BatchOperations, MemoryDatabase, SyncTime};
        use bdk::{testutils, BlockTime};

        let descriptors = testutils!(@descriptors (&format!("wpkh({}/*)", self.key)));

        let mut database = MemoryDatabase::new();

        for index in 0..self.num_utxos {
            bdk::populate_test_db!(
                &mut database,
                testutils! {
                    @tx ( (@external descriptors, index as u32) => self.utxo_amount ) (@confirmations 1)
                },
                Some(100)
            );
        }
        let block_time = bdk::BlockTime {
            height: 100,
            timestamp: 0,
        };
        let sync_time = SyncTime { block_time };
        database.set_sync_time(sync_time).unwrap();

        let wallet = bdk::Wallet::new(&descriptors.0, None, Network::Regtest, database).unwrap();

        Wallet {
            client: Arc::new(Mutex::new(StaticFeeRate {
                fee_rate: FeeRate::from_sat_per_vb(self.sats_per_vb),
                min_relay_fee: bitcoin::Amount::from_sat(self.min_relay_fee_sats),
            })),
            wallet: Arc::new(Mutex::new(wallet)),
            finality_confirmations: 1,
            network: Network::Regtest,
            target_block: 1,
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
    blockchain: ElectrumBlockchain,
    latest_block_height: BlockHeight,
    last_sync: Instant,
    sync_interval: Duration,
    script_history: BTreeMap<Script, Vec<GetHistoryRes>>,
    subscriptions: HashMap<(Txid, Script), Subscription>,
}

impl Client {
    fn new(electrum_rpc_url: Url, interval: Duration) -> Result<Self> {
        let config = bdk::electrum_client::ConfigBuilder::default()
            .retry(5)
            .build();
        let electrum = bdk::electrum_client::Client::from_config(electrum_rpc_url.as_str(), config)
            .context("Failed to initialize Electrum RPC client")?;
        // Initially fetch the latest block for storing the height.
        // We do not act on this subscription after this call.
        let latest_block = electrum
            .block_headers_subscribe()
            .context("Failed to subscribe to header notifications")?;

        let client = bdk::electrum_client::Client::new(electrum_rpc_url.as_str())
            .context("Failed to initialize Electrum RPC client")?;
        let blockchain = ElectrumBlockchain::from(client);
        let last_sync = Instant::now()
            .checked_sub(interval)
            .expect("no underflow since block time is only 600 secs");

        Ok(Self {
            electrum,
            blockchain,
            latest_block_height: BlockHeight::try_from(latest_block)?,
            last_sync,
            sync_interval: interval,
            script_history: Default::default(),
            subscriptions: Default::default(),
        })
    }

    fn blockchain(&self) -> &ElectrumBlockchain {
        &self.blockchain
    }

    fn get_tx(&self, txid: &Txid) -> Result<Option<Transaction>, bdk::Error> {
        self.blockchain.get_tx(txid)
    }

    fn update_state(&mut self, force_sync: bool) -> Result<()> {
        let now = Instant::now();

        if !force_sync && now < self.last_sync + self.sync_interval {
            return Ok(());
        }

        self.last_sync = now;
        self.update_latest_block()?;
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

            // When we first subscribe to a script we want to immediately fetch its status
            // Otherwise we would have to wait for the next sync interval, which can take a minute
            // This would result in potentially inaccurate status updates until that next sync interval is hit
            self.update_state(true)?;
        } else {
            self.update_state(false)?;
        }

        let history = self.script_history.entry(script).or_default();

        let history_of_tx = history
            .iter()
            .filter(|entry| entry.tx_hash == txid)
            .collect::<Vec<_>>();

        match history_of_tx.as_slice() {
            [] => Ok(ScriptStatus::Unseen),
            [remaining @ .., last] => {
                if !remaining.is_empty() {
                    tracing::warn!("Found more than a single history entry for script. This is highly unexpected and those history entries will be ignored")
                }

                if last.height <= 0 {
                    Ok(ScriptStatus::InMempool)
                } else {
                    Ok(ScriptStatus::Confirmed(
                        Confirmed::from_inclusion_and_latest_block(
                            u32::try_from(last.height)?,
                            u32::from(self.latest_block_height),
                        ),
                    ))
                }
            }
        }
    }

    fn update_latest_block(&mut self) -> Result<()> {
        // Fetch the latest block for storing the height.
        // We do not act on this subscription after this call, as we cannot rely on
        // subscription push notifications because eventually the Electrum server will
        // close the connection and subscriptions are not automatically renewed
        // upon renewing the connection.
        let latest_block = self
            .electrum
            .block_headers_subscribe()
            .context("Failed to subscribe to header notifications")?;
        let latest_block_height = BlockHeight::try_from(latest_block)?;

        if latest_block_height > self.latest_block_height {
            tracing::debug!(
                block_height = u32::from(latest_block_height),
                "Got notification for new block"
            );
            self.latest_block_height = latest_block_height;
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

impl EstimateFeeRate for Client {
    fn estimate_feerate(&self, target_block: usize) -> Result<FeeRate> {
        // https://github.com/romanz/electrs/blob/f9cf5386d1b5de6769ee271df5eef324aa9491bc/src/rpc.rs#L213
        // Returned estimated fees are per BTC/kb.
        let fee_per_byte = self.electrum.estimate_fee(target_block)?;
        // we do not expect fees being that high.
        #[allow(clippy::cast_possible_truncation)]
        Ok(FeeRate::from_btc_per_kvb(fee_per_byte as f32))
    }

    fn min_relay_fee(&self) -> Result<bitcoin::Amount> {
        // https://github.com/romanz/electrs/blob/f9cf5386d1b5de6769ee271df5eef324aa9491bc/src/rpc.rs#L219
        // Returned fee is in BTC/kb
        let relay_fee = bitcoin::Amount::from_btc(self.electrum.relay_fee()?)?;
        Ok(relay_fee)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ScriptStatus {
    Unseen,
    InMempool,
    Confirmed(Confirmed),
    Retrying,
}

impl ScriptStatus {
    pub fn from_confirmations(confirmations: u32) -> Self {
        match confirmations {
            0 => Self::InMempool,
            confirmations => Self::Confirmed(Confirmed::new(confirmations - 1)),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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
        T: Into<u32>,
    {
        self.confirmations() >= target.into()
    }

    pub fn blocks_left_until<T>(&self, target: T) -> u32
    where
        T: Into<u32> + Copy,
    {
        if self.meets_target(target) {
            0
        } else {
            target.into() - self.confirmations()
        }
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
        T: Into<u32>,
    {
        match self {
            ScriptStatus::Confirmed(inner) => inner.meets_target(target),
            _ => false,
        }
    }

    // Calculate the number of blocks left until the target is met.
    pub fn blocks_left_until<T>(&self, target: T) -> u32
    where
        T: Into<u32> + Copy,
    {
        match self {
            ScriptStatus::Confirmed(inner) => inner.blocks_left_until(target),
            _ => target.into(),
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
            ScriptStatus::Retrying => write!(f, "retrying"),
            ScriptStatus::Confirmed(inner) => {
                write!(f, "confirmed with {} blocks", inner.confirmations())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitcoin::{PublicKey, TxLock};
    use crate::tracing_ext::capture_logs;
    use bitcoin::hashes::Hash;
    use proptest::prelude::*;
    use tracing::level_filters::LevelFilter;

    #[test]
    fn given_depth_0_should_meet_confirmation_target_one() {
        let script = ScriptStatus::Confirmed(Confirmed { depth: 0 });

        let confirmed = script.is_confirmed_with(1_u32);

        assert!(confirmed)
    }

    #[test]
    fn given_confirmations_1_should_meet_confirmation_target_one() {
        let script = ScriptStatus::from_confirmations(1);

        let confirmed = script.is_confirmed_with(1_u32);

        assert!(confirmed)
    }

    #[test]
    fn given_inclusion_after_lastest_known_block_at_least_depth_0() {
        let included_in = 10;
        let latest_block = 9;

        let confirmed = Confirmed::from_inclusion_and_latest_block(included_in, latest_block);

        assert_eq!(confirmed.depth, 0)
    }

    #[test]
    fn given_depth_0_should_return_0_blocks_left_until_1() {
        let script = ScriptStatus::Confirmed(Confirmed { depth: 0 });

        let blocks_left = script.blocks_left_until(1_u32);

        assert_eq!(blocks_left, 0)
    }

    #[test]
    fn given_depth_1_should_return_0_blocks_left_until_1() {
        let script = ScriptStatus::Confirmed(Confirmed { depth: 1 });

        let blocks_left = script.blocks_left_until(1_u32);

        assert_eq!(blocks_left, 0)
    }

    #[test]
    fn given_depth_0_should_return_1_blocks_left_until_2() {
        let script = ScriptStatus::Confirmed(Confirmed { depth: 0 });

        let blocks_left = script.blocks_left_until(2_u32);

        assert_eq!(blocks_left, 1)
    }

    #[test]
    fn given_one_BTC_and_100k_sats_per_vb_fees_should_not_hit_max() {
        // 400 weight = 100 vbyte
        let weight = 400;
        let amount = bitcoin::Amount::from_sat(100_000_000);

        let sat_per_vb = 100.0;
        let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb);

        let relay_fee = bitcoin::Amount::ONE_SAT;
        let is_fee = estimate_fee(weight, amount, fee_rate, relay_fee).unwrap();

        // weight / 4.0 *  sat_per_vb
        let should_fee = bitcoin::Amount::from_sat(10_000);
        assert_eq!(is_fee, should_fee);
    }

    #[test]
    fn given_1BTC_and_1_sat_per_vb_fees_and_100ksat_min_relay_fee_should_hit_min() {
        // 400 weight = 100 vbyte
        let weight = 400;
        let amount = bitcoin::Amount::from_sat(100_000_000);

        let sat_per_vb = 1.0;
        let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb);

        let relay_fee = bitcoin::Amount::from_sat(100_000);
        let is_fee = estimate_fee(weight, amount, fee_rate, relay_fee).unwrap();

        // weight / 4.0 *  sat_per_vb would be smaller than relay fee hence we take min
        // relay fee
        let should_fee = bitcoin::Amount::from_sat(100_000);
        assert_eq!(is_fee, should_fee);
    }

    #[test]
    fn given_1mio_sat_and_1k_sats_per_vb_fees_should_hit_relative_max() {
        // 400 weight = 100 vbyte
        let weight = 400;
        let amount = bitcoin::Amount::from_sat(1_000_000);

        let sat_per_vb = 1_000.0;
        let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb);

        let relay_fee = bitcoin::Amount::ONE_SAT;
        let is_fee = estimate_fee(weight, amount, fee_rate, relay_fee).unwrap();

        // weight / 4.0 *  sat_per_vb would be greater than 3% hence we take max
        // relative fee.
        let should_fee = bitcoin::Amount::from_sat(30_000);
        assert_eq!(is_fee, should_fee);
    }

    #[test]
    fn given_1BTC_and_4mio_sats_per_vb_fees_should_hit_total_max() {
        // even if we send 1BTC we don't want to pay 0.3BTC in fees. This would be
        // $1,650 at the moment.
        let weight = 400;
        let amount = bitcoin::Amount::from_sat(100_000_000);

        let sat_per_vb = 4_000_000.0;
        let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb);

        let relay_fee = bitcoin::Amount::ONE_SAT;
        let is_fee = estimate_fee(weight, amount, fee_rate, relay_fee).unwrap();

        // weight / 4.0 *  sat_per_vb would be greater than 3% hence we take total
        // max allowed fee.
        assert_eq!(is_fee.to_sat(), MAX_ABSOLUTE_TX_FEE.to_u64().unwrap());
    }

    proptest! {
        #[test]
        fn given_randon_amount_random_fee_and_random_relay_rate_but_fix_weight_does_not_error(
            amount in 547u64..,
            sat_per_vb in 1.0f32..100_000_000.0f32,
            relay_fee in 0u64..100_000_000u64
        ) {
            let weight = 400;
            let amount = bitcoin::Amount::from_sat(amount);

            let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb);

            let relay_fee = bitcoin::Amount::from_sat(relay_fee);
            let _is_fee = estimate_fee(weight, amount, fee_rate, relay_fee).unwrap();

        }
    }

    proptest! {
        #[test]
        fn given_amount_in_range_fix_fee_fix_relay_rate_fix_weight_fee_always_smaller_max(
            amount in 1u64..100_000_000,
        ) {
            let weight = 400;
            let amount = bitcoin::Amount::from_sat(amount);

            let sat_per_vb = 100.0;
            let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb);

            let relay_fee = bitcoin::Amount::ONE_SAT;
            let is_fee = estimate_fee(weight, amount, fee_rate, relay_fee).unwrap();

            // weight / 4 * 1_000 is always lower than MAX_ABSOLUTE_TX_FEE
            assert!(is_fee.to_sat() < MAX_ABSOLUTE_TX_FEE.to_u64().unwrap());
        }
    }

    proptest! {
        #[test]
        fn given_amount_high_fix_fee_fix_relay_rate_fix_weight_fee_always_max(
            amount in 100_000_000u64..,
        ) {
            let weight = 400;
            let amount = bitcoin::Amount::from_sat(amount);

            let sat_per_vb = 1_000.0;
            let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb);

            let relay_fee = bitcoin::Amount::ONE_SAT;
            let is_fee = estimate_fee(weight, amount, fee_rate, relay_fee).unwrap();

            // weight / 4 * 1_000  is always higher than MAX_ABSOLUTE_TX_FEE
            assert!(is_fee.to_sat() >= MAX_ABSOLUTE_TX_FEE.to_u64().unwrap());
        }
    }

    proptest! {
        #[test]
        fn given_fee_above_max_should_always_errors(
            sat_per_vb in 100_000_000.0f32..,
        ) {
            let weight = 400;
            let amount = bitcoin::Amount::from_sat(547u64);

            let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb);

            let relay_fee = bitcoin::Amount::from_sat(1);
            assert!(estimate_fee(weight, amount, fee_rate, relay_fee).is_err());

        }
    }

    proptest! {
        #[test]
        fn given_relay_fee_above_max_should_always_errors(
            relay_fee in 100_000_000u64..
        ) {
            let weight = 400;
            let amount = bitcoin::Amount::from_sat(547u64);

            let fee_rate = FeeRate::from_sat_per_vb(1.0);

            let relay_fee = bitcoin::Amount::from_sat(relay_fee);
            assert!(estimate_fee(weight, amount, fee_rate, relay_fee).is_err());
        }
    }

    #[tokio::test]
    async fn given_no_balance_returns_amount_0() {
        let wallet = WalletBuilder::new(0).with_fees(1.0, 1).build();
        let amount = wallet.max_giveable(TxLock::script_size()).await.unwrap();

        assert_eq!(amount, Amount::ZERO);
    }

    #[tokio::test]
    async fn given_balance_below_min_relay_fee_returns_amount_0() {
        let wallet = WalletBuilder::new(1000).with_fees(1.0, 1001).build();
        let amount = wallet.max_giveable(TxLock::script_size()).await.unwrap();

        assert_eq!(amount, Amount::ZERO);
    }

    #[tokio::test]
    async fn given_balance_above_relay_fee_returns_amount_greater_0() {
        let wallet = WalletBuilder::new(10_000).build();
        let amount = wallet.max_giveable(TxLock::script_size()).await.unwrap();

        assert!(amount.to_sat() > 0);
    }

    /// This test ensures that the relevant script output of the transaction
    /// created out of the PSBT is at index 0. This is important because
    /// subscriptions to the transaction are on index `0` when broadcasting the
    /// transaction.
    #[tokio::test]
    async fn given_amounts_with_change_outputs_when_signing_tx_then_output_index_0_is_ensured_for_script(
    ) {
        // This value is somewhat arbitrary but the indexation problem usually occurred
        // on the first or second value (i.e. 547, 548) We keep the test
        // iterations relatively low because these tests are expensive.
        let above_dust = 547;
        let balance = 2000;

        // We don't care about fees in this test, thus use a zero fee rate
        let wallet = WalletBuilder::new(balance).with_zero_fees().build();

        // sorting is only relevant for amounts that have a change output
        // if the change output is below dust it will be dropped by the BDK
        for amount in above_dust..(balance - (above_dust - 1)) {
            let (A, B) = (PublicKey::random(), PublicKey::random());
            let change = wallet.new_address().await.unwrap();
            let txlock = TxLock::new(&wallet, bitcoin::Amount::from_sat(amount), A, B, change)
                .await
                .unwrap();
            let txlock_output = txlock.script_pubkey();

            let tx = wallet.sign_and_finalize(txlock.into()).await.unwrap();
            let tx_output = tx.output[0].script_pubkey.clone();

            assert_eq!(
                tx_output, txlock_output,
                "Output {:?} index mismatch for amount {} and balance {}",
                tx.output, amount, balance
            );
        }
    }

    #[tokio::test]
    async fn can_override_change_address() {
        let wallet = WalletBuilder::new(50_000).build();
        let custom_change = "bcrt1q08pfqpsyrt7acllzyjm8q5qsz5capvyahm49rw"
            .parse::<Address>()
            .unwrap();

        let psbt = wallet
            .send_to_address(
                wallet.new_address().await.unwrap(),
                Amount::from_sat(10_000),
                Some(custom_change.clone()),
            )
            .await
            .unwrap();
        let transaction = wallet.sign_and_finalize(psbt).await.unwrap();

        match transaction.output.as_slice() {
            [first, change] => {
                assert_eq!(first.value, 10_000);
                assert_eq!(change.script_pubkey, custom_change.script_pubkey());
            }
            _ => panic!("expected exactly two outputs"),
        }
    }

    #[test]
    fn printing_status_change_doesnt_spam_on_same_status() {
        let writer = capture_logs(LevelFilter::DEBUG);

        let inner = bitcoin::hashes::sha256d::Hash::all_zeros();
        let tx = Txid::from_hash(inner);
        let mut old = None;
        old = Some(print_status_change(tx, old, ScriptStatus::Unseen));
        old = Some(print_status_change(tx, old, ScriptStatus::InMempool));
        old = Some(print_status_change(tx, old, ScriptStatus::InMempool));
        old = Some(print_status_change(tx, old, ScriptStatus::InMempool));
        old = Some(print_status_change(tx, old, ScriptStatus::InMempool));
        old = Some(print_status_change(tx, old, ScriptStatus::InMempool));
        old = Some(print_status_change(tx, old, ScriptStatus::InMempool));
        old = Some(print_status_change(tx, old, confs(1)));
        old = Some(print_status_change(tx, old, confs(2)));
        old = Some(print_status_change(tx, old, confs(3)));
        old = Some(print_status_change(tx, old, confs(3)));
        print_status_change(tx, old, confs(3));

        assert_eq!(
            writer.captured(),
            r"DEBUG swap::bitcoin::wallet: Found relevant Bitcoin transaction txid=0000000000000000000000000000000000000000000000000000000000000000 status=unseen
DEBUG swap::bitcoin::wallet: Bitcoin transaction status changed txid=0000000000000000000000000000000000000000000000000000000000000000 new_status=in mempool old_status=unseen
DEBUG swap::bitcoin::wallet: Bitcoin transaction status changed txid=0000000000000000000000000000000000000000000000000000000000000000 new_status=confirmed with 1 blocks old_status=in mempool
DEBUG swap::bitcoin::wallet: Bitcoin transaction status changed txid=0000000000000000000000000000000000000000000000000000000000000000 new_status=confirmed with 2 blocks old_status=confirmed with 1 blocks
DEBUG swap::bitcoin::wallet: Bitcoin transaction status changed txid=0000000000000000000000000000000000000000000000000000000000000000 new_status=confirmed with 3 blocks old_status=confirmed with 2 blocks
"
        )
    }

    fn confs(confirmations: u32) -> ScriptStatus {
        ScriptStatus::from_confirmations(confirmations)
    }

    proptest::proptest! {
        #[test]
        fn funding_never_fails_with_insufficient_funds(funding_amount in 3000u32.., num_utxos in 1..5u8, sats_per_vb in 1.0..500.0f32, key in crate::proptest::bitcoin::extended_priv_key(), alice in crate::proptest::ecdsa_fun::point(), bob in crate::proptest::ecdsa_fun::point()) {
            proptest::prop_assume!(alice != bob);

            tokio::runtime::Runtime::new().unwrap().block_on(async move {
                let wallet = WalletBuilder::new(funding_amount as u64).with_key(key).with_num_utxos(num_utxos).with_fees(sats_per_vb, 1000).build();

                let amount = wallet.max_giveable(TxLock::script_size()).await.unwrap();
                let psbt: PartiallySignedTransaction = TxLock::new(&wallet, amount, PublicKey::from(alice), PublicKey::from(bob), wallet.new_address().await.unwrap()).await.unwrap().into();
                let result = wallet.sign_and_finalize(psbt).await;

                result.expect("transaction to be signed");
            });
        }
    }
}
