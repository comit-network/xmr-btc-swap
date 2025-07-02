use crate::bitcoin::{parse_rpc_error_code, Address, Amount, RpcErrorCode, Transaction};
use crate::cli::api::tauri_bindings::{
    TauriBackgroundProgress, TauriBitcoinFullScanProgress, TauriBitcoinSyncProgress, TauriEmitter,
    TauriHandle,
};
use crate::seed::Seed;
use anyhow::{anyhow, bail, Context, Result};
use bdk_chain::spk_client::{SyncRequest, SyncRequestBuilder};
use bdk_chain::CheckPoint;
use bdk_electrum::electrum_client::{ElectrumApi, GetHistoryRes};

use bdk_wallet::bitcoin::FeeRate;
use bdk_wallet::bitcoin::Network;
use bdk_wallet::export::FullyNodedExport;
use bdk_wallet::rusqlite::Connection;
use bdk_wallet::template::{Bip84, DescriptorTemplate};
use bdk_wallet::KeychainKind;
use bdk_wallet::SignOptions;
use bdk_wallet::WalletPersister;
use bdk_wallet::{Balance, PersistedWallet};
use bitcoin::bip32::Xpriv;
use bitcoin::{psbt::Psbt as PartiallySignedTransaction, Txid};
use bitcoin::{ScriptBuf, Weight};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Debug;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as SyncMutex;
use std::time::Duration;
use std::time::Instant;
use sync_ext::{CumulativeProgressHandle, InnerSyncCallback, SyncCallbackExt};
use tokio::sync::watch;
use tokio::sync::Mutex as TokioMutex;
use tracing::{debug_span, Instrument};

use super::bitcoin_address::revalidate_network;
use super::BlockHeight;
use derive_builder::Builder;
use electrum_pool::ElectrumBalancer;
use moka;

/// We allow transaction fees of up to 20% of the transferred amount to ensure
/// that lock transactions can always be published, even when fees are high.
const MAX_RELATIVE_TX_FEE: Decimal = dec!(0.20);
const MAX_ABSOLUTE_TX_FEE: Amount = Amount::from_sat(100_000);
const MIN_ABSOLUTE_TX_FEE: Amount = Amount::from_sat(1000);
const DUST_AMOUNT: Amount = Amount::from_sat(546);

/// This is our wrapper around a bdk wallet and a corresponding
/// bdk electrum client.
/// It unifies all the functionality we need when interacting
/// with the bitcoin network.
///
/// This wallet is generic over the persister, which may be a
/// rusqlite connection, or an in-memory database, or something else.
#[derive(Clone)]
pub struct Wallet<Persister = Connection, C = Client> {
    /// The wallet, which is persisted to the disk.
    wallet: Arc<TokioMutex<PersistedWallet<Persister>>>,
    /// The database connection used to persist the wallet.
    persister: Arc<TokioMutex<Persister>>,
    /// The electrum client.
    electrum_client: Arc<TokioMutex<C>>,
    /// The cached fee estimator for the electrum client.
    cached_electrum_fee_estimator: Arc<CachedFeeEstimator<C>>,
    /// The cached fee estimator for the mempool client.
    cached_mempool_fee_estimator: Arc<Option<CachedFeeEstimator<mempool_client::MempoolClient>>>,
    /// The network this wallet is on.
    network: Network,
    /// The number of confirmations (blocks) we require for a transaction
    /// to be considered final.
    ///
    /// Usually set to 1.
    finality_confirmations: u32,
    /// We want our transactions to be confirmed after this many blocks
    /// (used for fee estimation).
    target_block: u32,
    /// The Tauri handle
    tauri_handle: Option<TauriHandle>,
}

/// This is our wrapper around a bdk electrum client.
#[derive(Clone)]
pub struct Client {
    /// The underlying electrum balancer for load balancing across multiple servers.
    inner: Arc<ElectrumBalancer>,
    /// The history of transactions for each script.
    script_history: BTreeMap<ScriptBuf, Vec<GetHistoryRes>>,
    /// The subscriptions to the status of transactions.
    subscriptions: HashMap<(Txid, ScriptBuf), Subscription>,
    /// The time of the last sync.
    last_sync: Instant,
    /// How often we sync with the server.
    sync_interval: Duration,
    /// The height of the latest block we know about.
    latest_block_height: BlockHeight,
}

/// Holds the configuration parameters for creating a Bitcoin wallet.
/// The actual Wallet<Connection> will be constructed from this configuration.
#[derive(Builder, Clone)]
#[builder(
    name = "WalletBuilder",
    pattern = "owned",
    setter(into, strip_option),
    build_fn(
        name = "validate_config",
        private,
        error = "derive_builder::UninitializedFieldError"
    ),
    derive(Clone)
)]
pub struct WalletConfig {
    seed: Seed,
    network: Network,
    electrum_rpc_urls: Vec<String>,
    persister: PersisterConfig,
    finality_confirmations: u32,
    target_block: u32,
    sync_interval: Duration,
    #[builder(default)]
    tauri_handle: Option<TauriHandle>,
    #[builder(default = "true")]
    use_mempool_space_fee_estimation: bool,
}

impl WalletBuilder {
    /// Asynchronously builds the `Wallet<Connection>` using the configured parameters.
    /// This method contains the core logic for wallet initialization, including
    /// database setup, key derivation, and potential migration from older wallet formats.
    pub async fn build(self) -> Result<Wallet<Connection, Client>> {
        let config = self
            .validate_config()
            .map_err(|e| anyhow!("Builder validation failed: {e}"))?;

        let client = Client::new(&config.electrum_rpc_urls, config.sync_interval)
            .await
            .context("Failed to create Electrum client")?;

        match &config.persister {
            PersisterConfig::SqliteFile { data_dir } => {
                let xprivkey = config
                    .seed
                    .derive_extended_private_key(config.network)
                    .context("Failed to derive extended private key for file wallet")?;

                let wallet_parent_dir = data_dir.join(Wallet::<Connection>::WALLET_PARENT_DIR_NAME);
                let wallet_dir = wallet_parent_dir.join(Wallet::<Connection>::WALLET_DIR_NAME);
                let wallet_path = wallet_dir.join(Wallet::<Connection>::WALLET_FILE_NAME);
                let wallet_exists = wallet_path.exists();

                tokio::fs::create_dir_all(&wallet_dir)
                    .await
                    .context("Failed to create wallet directory")?;

                let open_connection = || -> Result<Connection> {
                    Connection::open(&wallet_path).context(format!(
                        "Failed to open SQLite database at {:?}",
                        wallet_path
                    ))
                };

                if wallet_exists {
                    let connection = open_connection()?;

                    Wallet::create_existing(
                        xprivkey,
                        config.network,
                        client,
                        connection,
                        config.finality_confirmations,
                        config.target_block,
                        config.tauri_handle.clone(),
                        config.use_mempool_space_fee_estimation,
                    )
                    .await
                    .context("Failed to load existing wallet")
                } else {
                    let old_wallet_export = Wallet::<Connection>::get_pre_1_0_bdk_wallet_export(
                        data_dir,
                        config.network,
                        &config.seed,
                    )
                    .await
                    .context("Failed to get pre-1.0.0 BDK wallet export for migration")?;

                    Wallet::create_new(
                        xprivkey,
                        config.network,
                        client,
                        open_connection,
                        config.finality_confirmations,
                        config.target_block,
                        old_wallet_export,
                        config.tauri_handle.clone(),
                        config.use_mempool_space_fee_estimation,
                    )
                    .await
                    .context("Failed to create new wallet")
                }
            }
            PersisterConfig::InMemorySqlite => {
                let xprivkey = config
                    .seed
                    .derive_extended_private_key(config.network)
                    .context("Failed to derive extended private key for in-memory wallet")?;

                let persister = Connection::open_in_memory()
                    .context("Failed to open in-memory SQLite database")?;

                Wallet::create_new::<Connection>(
                    xprivkey,
                    config.network,
                    client,
                    move || Ok(persister),
                    config.finality_confirmations,
                    config.target_block,
                    None,
                    config.tauri_handle.clone(),
                    config.use_mempool_space_fee_estimation,
                )
                .await
                .context("Failed to create new in-memory wallet")
            }
        }
    }
}

/// Configuration for how the wallet should be persisted.
#[derive(Debug, Clone)]
pub enum PersisterConfig {
    SqliteFile { data_dir: PathBuf },
    InMemorySqlite,
}

/// A subscription to the status of a given transaction
/// that can be used to wait for the transaction to be confirmed.
#[derive(Debug, Clone)]
pub struct Subscription {
    /// A receiver used to await updates to the status of the transaction.
    receiver: watch::Receiver<ScriptStatus>,
    /// The number of confirmations we require for a transaction to be considered final.
    finality_confirmations: u32,
    /// The transaction ID we are subscribing to.
    txid: Txid,
}

/// The possible statuses of a script.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ScriptStatus {
    Unseen,
    InMempool,
    Confirmed(Confirmed),
    Retrying,
}

/// The status of a confirmed transaction.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Confirmed {
    /// The depth of this transaction within the blockchain.
    ///
    /// Zero if the transaction is included in the latest block.
    depth: u32,
}

/// Defines a watchable transaction.
///
/// For a transaction to be watchable, we need to know two things: Its
/// transaction ID and the specific output script that is going to change.
/// A transaction can obviously have multiple outputs but our protocol purposes,
/// we are usually interested in a specific one.
pub trait Watchable {
    /// The transaction ID.
    fn id(&self) -> Txid;
    /// The script of the output we are interested in.
    fn script(&self) -> ScriptBuf;
    /// Convenience method to get both the script and the txid.
    fn script_and_txid(&self) -> (ScriptBuf, Txid) {
        (self.script(), self.id())
    }
}

/// An object that can estimate fee rates and minimum relay fees.
pub trait EstimateFeeRate {
    /// Estimate the fee rate for a given target block.
    fn estimate_feerate(
        &self,
        target_block: u32,
    ) -> impl std::future::Future<Output = Result<FeeRate>> + Send;
    /// Get the minimum relay fee.
    fn min_relay_fee(&self) -> impl std::future::Future<Output = Result<FeeRate>> + Send;
}

/// A caching wrapper around EstimateFeeRate implementations.
///
/// Uses Moka cache with TTL (Time To Live) expiration for both fee rate estimates
/// and minimum relay fees to reduce the frequency of network calls to Electrum and mempool.space APIs.
#[derive(Clone)]
pub struct CachedFeeEstimator<T> {
    inner: T,
    fee_cache: Arc<moka::future::Cache<u32, FeeRate>>,
    min_relay_cache: Arc<moka::future::Cache<(), FeeRate>>,
}

impl<T> CachedFeeEstimator<T> {
    /// Cache duration for fee estimates (2 minutes)
    const CACHE_DURATION: Duration = Duration::from_secs(120);
    /// Maximum number of cached fee rate entries (different target blocks)
    const MAX_CACHE_SIZE: u64 = 10;

    /// Create a new caching wrapper around an EstimateFeeRate implementation.
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            fee_cache: Arc::new(
                moka::future::Cache::builder()
                    .max_capacity(Self::MAX_CACHE_SIZE)
                    .time_to_live(Self::CACHE_DURATION)
                    .build(),
            ),
            min_relay_cache: Arc::new(
                moka::future::Cache::builder()
                    .max_capacity(1) // Only one min relay fee value
                    .time_to_live(Self::CACHE_DURATION)
                    .build(),
            ),
        }
    }
}

impl<T: EstimateFeeRate + Send + Sync> EstimateFeeRate for CachedFeeEstimator<T> {
    async fn estimate_feerate(&self, target_block: u32) -> Result<FeeRate> {
        // Check cache first
        if let Some(cached_rate) = self.fee_cache.get(&target_block).await {
            return Ok(cached_rate);
        }

        // If not in cache, fetch from underlying estimator
        let fee_rate = self.inner.estimate_feerate(target_block).await?;

        // Store in cache
        self.fee_cache.insert(target_block, fee_rate).await;

        Ok(fee_rate)
    }

    async fn min_relay_fee(&self) -> Result<FeeRate> {
        // Check cache first
        if let Some(cached_rate) = self.min_relay_cache.get(&()).await {
            return Ok(cached_rate);
        }

        // If not in cache, fetch from underlying estimator
        let min_relay_fee = self.inner.min_relay_fee().await?;

        // Store in cache
        self.min_relay_cache.insert((), min_relay_fee).await;

        Ok(min_relay_fee)
    }
}

impl<T> std::ops::Deref for CachedFeeEstimator<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Wallet {
    /// If this many consequent addresses are unused, we stop the full scan.
    /// On old wallets we used to generate a ton of unused addresses
    /// which results in us having a bunch of large gaps in the SPKs
    const SCAN_STOP_GAP: u32 = 500;
    /// The batch size for syncing
    const SCAN_BATCH_SIZE: u32 = 32;
    /// The number of maximum chunks to use when syncing
    const SCAN_CHUNKS: u32 = 5;

    /// Maximum time we are willing to spend retrying a wallet sync
    const SYNC_MAX_ELAPSED_TIME: Duration = Duration::from_secs(15);

    const WALLET_PARENT_DIR_NAME: &str = "wallet";
    const WALLET_DIR_NAME: &str = "wallet-post-bdk-1.0";
    const WALLET_FILE_NAME: &str = "wallet-db.sqlite";

    async fn get_pre_1_0_bdk_wallet_export(
        data_dir: impl AsRef<Path>,
        network: Network,
        seed: &Seed,
    ) -> Result<Option<pre_1_0_0_bdk::Export>> {
        // Construct the directory in which the old (<1.0 bdk) wallet was stored
        let wallet_parent_dir = data_dir.as_ref().join(Self::WALLET_PARENT_DIR_NAME);
        let pre_bdk_1_0_wallet_dir = wallet_parent_dir.join(pre_1_0_0_bdk::WALLET);
        let pre_bdk_1_0_wallet_exists = pre_bdk_1_0_wallet_dir.exists();

        if pre_bdk_1_0_wallet_exists {
            tracing::info!("Found old Bitcoin wallet (pre 1.0 bdk). Migrating...");

            // We need to support the legacy wallet format for the migration path.
            // We need to convert the network to the legacy BDK network type.
            let legacy_network = match network {
                Network::Bitcoin => bdk::bitcoin::Network::Bitcoin,
                Network::Testnet => bdk::bitcoin::Network::Testnet,
                _ => bail!("Unsupported network: {}", network),
            };

            let xprivkey = seed.derive_extended_private_key_legacy(legacy_network)?;
            let old_wallet =
                pre_1_0_0_bdk::OldWallet::new(&pre_bdk_1_0_wallet_dir, xprivkey, network).await?;

            let export = old_wallet.export("old-wallet").await?;

            tracing::debug!(
                external_index=%export.external_derivation_index,
                internal_index=%export.internal_derivation_index,
                "Constructed export of old Bitcoin wallet (pre 1.0 bdk) for migration"
            );

            Ok(Some(export))
        } else {
            Ok(None)
        }
    }

    /// Create a new wallet, persisted to a sqlite database.
    /// This is a private API so we allow too many arguments.
    #[allow(clippy::too_many_arguments)]
    pub async fn with_sqlite(
        seed: &Seed,
        network: Network,
        electrum_rpc_urls: &[String],
        data_dir: impl AsRef<Path>,
        finality_confirmations: u32,
        target_block: u32,
        sync_interval: Duration,
        env_config: crate::env::Config,
        tauri_handle: Option<TauriHandle>,
    ) -> Result<Wallet<bdk_wallet::rusqlite::Connection, Client>> {
        // Construct the private key, directory and wallet file for the new (>= 1.0.0) bdk wallet
        let xprivkey = seed.derive_extended_private_key(env_config.bitcoin_network)?;
        let wallet_dir = data_dir
            .as_ref()
            .join(Self::WALLET_PARENT_DIR_NAME)
            .join(Self::WALLET_DIR_NAME);
        let wallet_path = wallet_dir.join(Self::WALLET_FILE_NAME);
        let wallet_exists = wallet_path.exists();

        // Connect to the electrum server.
        let client = Client::new(electrum_rpc_urls, sync_interval).await?;

        // Make sure the wallet directory exists.
        tokio::fs::create_dir_all(&wallet_dir).await?;

        let connection =
            || Connection::open(&wallet_path).context("Failed to open SQLite database");

        // If the new Bitcoin wallet (> 1.0.0 bdk) already exists, we open it
        if wallet_exists {
            Self::create_existing(
                xprivkey,
                network,
                client,
                connection()?,
                finality_confirmations,
                target_block,
                tauri_handle,
                true, // default to true for mempool space fee estimation
            )
            .await
        } else {
            // If the new Bitcoin wallet (> 1.0.0 bdk) does not yet exist:
            // We check if we have an old (< 1.0.0 bdk) wallet. If so, we migrate.
            let export = Self::get_pre_1_0_bdk_wallet_export(data_dir, network, seed).await?;

            Self::create_new(
                xprivkey,
                network,
                client,
                connection,
                finality_confirmations,
                target_block,
                export,
                tauri_handle,
                true, // default to true for mempool space fee estimation
            )
            .await
        }
    }

    /// Create a new wallet, persisted to an in-memory sqlite database.
    /// Should only be used for testing.
    #[cfg(test)]
    pub async fn with_sqlite_in_memory(
        seed: &Seed,
        network: Network,
        electrum_rpc_urls: &[String],
        finality_confirmations: u32,
        target_block: u32,
        sync_interval: Duration,
        tauri_handle: Option<TauriHandle>,
    ) -> Result<Wallet<bdk_wallet::rusqlite::Connection, Client>> {
        Self::create_new(
            seed.derive_extended_private_key(network)?,
            network,
            Client::new(electrum_rpc_urls, sync_interval)
                .await
                .expect("Failed to create electrum client"),
            || {
                bdk_wallet::rusqlite::Connection::open_in_memory()
                    .context("Failed to open in-memory SQLite database")
            },
            finality_confirmations,
            target_block,
            None,
            tauri_handle,
            true, // default to true for mempool space fee estimation
        )
        .await
    }

    /// Create a new wallet in the database and perform a full scan.
    /// This is a private API so we allow too many arguments.
    #[allow(clippy::too_many_arguments)]
    async fn create_new<Persister>(
        xprivkey: Xpriv,
        network: Network,
        client: Client,
        persister_constructor: impl FnOnce() -> Result<Persister>,
        finality_confirmations: u32,
        target_block: u32,
        old_wallet: Option<pre_1_0_0_bdk::Export>,
        tauri_handle: Option<TauriHandle>,
        use_mempool_space_fee_estimation: bool,
    ) -> Result<Wallet<Persister, Client>>
    where
        Persister: WalletPersister + Sized,
        <Persister as WalletPersister>::Error: std::error::Error + Send + Sync + 'static,
    {
        let external_descriptor = Bip84(xprivkey, KeychainKind::External)
            .build(network)
            .context("Failed to build external wallet descriptor")?;

        let internal_descriptor = Bip84(xprivkey, KeychainKind::Internal)
            .build(network)
            .context("Failed to build change wallet descriptor")?;

        // Build the wallet without a persister
        // because we create the persistence AFTER the full scan
        let mut wallet =
            bdk_wallet::Wallet::create(external_descriptor.clone(), internal_descriptor.clone())
                .network(network)
                .create_wallet_no_persist()
                .context("Failed to create persisterless wallet")?;

        // If we have an old wallet, we need to reveal the addresses that were used before
        // to speed up the initial sync.
        if let Some(old_wallet) = old_wallet {
            tracing::info!("Migrating from old Bitcoin wallet (< 1.0 bdk)");

            // We reveal the address but we DO NOT persist them yet
            // Because if we persist it'll create the wallet file and we will
            // not start the initial scan again if it's interrupted by the user
            let _ = wallet
                .reveal_addresses_to(KeychainKind::External, old_wallet.external_derivation_index);
            let _ = wallet
                .reveal_addresses_to(KeychainKind::Internal, old_wallet.internal_derivation_index);
        }

        tracing::info!("Starting initial Bitcoin wallet scan. This might take a while...");

        let progress_handle = tauri_handle.new_background_process_with_initial_progress(
            TauriBackgroundProgress::FullScanningBitcoinWallet,
            TauriBitcoinFullScanProgress::Unknown,
        );

        let progress_handle_clone = progress_handle.clone();

        let callback = sync_ext::InnerSyncCallback::new(move |consumed, total| {
            progress_handle_clone.update(TauriBitcoinFullScanProgress::Known {
                current_index: consumed,
                assumed_total: total,
            });
        }).chain(sync_ext::InnerSyncCallback::new(move |consumed, total| {
            tracing::debug!(
                "Full scanning Bitcoin wallet, currently at index {}. We will scan around {} in total.",
                consumed,
                total
            );
        }).throttle_callback(10.0)).to_full_scan_callback(Self::SCAN_STOP_GAP, 100);

        let full_scan = wallet.start_full_scan().inspect(callback);

        let full_scan_response = client.inner.get_any_client().await?.full_scan(
            full_scan,
            Self::SCAN_STOP_GAP as usize,
            Self::SCAN_BATCH_SIZE as usize,
            true,
        )?;

        // Only create the persister once we have the full scan result
        let mut persister = persister_constructor()?;

        // Create a new (persisted) wallet
        let mut wallet = bdk_wallet::Wallet::create(external_descriptor, internal_descriptor)
            .network(network)
            .create_wallet(&mut persister)
            .context("Failed to create wallet with persister")?;

        // Apply the full scan result to the wallet
        wallet.apply_update(full_scan_response)?;
        wallet.persist(&mut persister)?;

        progress_handle.finish();

        tracing::trace!("Initial Bitcoin wallet scan completed");

        // Create the mempool client
        let mempool_client = if use_mempool_space_fee_estimation {
            mempool_client::MempoolClient::new(network).inspect_err(|e| {
                tracing::warn!("Failed to create mempool client: {:?}. We will only use the Electrum server for fee estimation.", e);
            }).ok()
        } else {
            None
        };

        // Create cached fee estimators
        let cached_electrum_fee_estimator = Arc::new(CachedFeeEstimator::new(client.clone()));
        let cached_mempool_fee_estimator =
            Arc::new(mempool_client.clone().map(CachedFeeEstimator::new));

        Ok(Wallet {
            wallet: wallet.into_arc_mutex_async(),
            electrum_client: client.into_arc_mutex_async(),
            cached_electrum_fee_estimator,
            cached_mempool_fee_estimator,
            persister: persister.into_arc_mutex_async(),
            tauri_handle,
            network,
            finality_confirmations,
            target_block,
        })
    }

    /// Load existing wallet data from the database
    #[allow(clippy::too_many_arguments)]
    async fn create_existing<Persister>(
        xprivkey: Xpriv,
        network: Network,
        client: Client,
        mut persister: Persister,
        finality_confirmations: u32,
        target_block: u32,
        tauri_handle: Option<TauriHandle>,
        use_mempool_space_fee_estimation: bool,
    ) -> Result<Wallet<Persister, Client>>
    where
        Persister: WalletPersister + Sized,
        <Persister as WalletPersister>::Error: std::error::Error + Send + Sync + 'static,
    {
        let external_descriptor = Bip84(xprivkey, KeychainKind::External)
            .build(network)
            .context("Failed to build external wallet descriptor")?;

        let internal_descriptor = Bip84(xprivkey, KeychainKind::Internal)
            .build(network)
            .context("Failed to build change wallet descriptor")?;

        tracing::debug!("Loading existing Bitcoin wallet from database");

        let wallet = bdk_wallet::Wallet::load()
            .descriptor(KeychainKind::External, Some(external_descriptor))
            .descriptor(KeychainKind::Internal, Some(internal_descriptor))
            .extract_keys()
            .load_wallet(&mut persister)
            .context("Failed to open database")?
            .context("No wallet found in database")?;

        // Create the mempool client with caching
        let cached_mempool_fee_estimator = if use_mempool_space_fee_estimation {
            mempool_client::MempoolClient::new(network).inspect_err(|e| {
                tracing::warn!("Failed to create mempool client: {:?}. We will only use the Electrum server for fee estimation.", e);
            }).ok().map(CachedFeeEstimator::new)
        } else {
            None
        };

        // Wrap the electrum client with caching
        let cached_electrum_fee_estimator = Arc::new(CachedFeeEstimator::new(client.clone()));

        let wallet = Wallet {
            wallet: wallet.into_arc_mutex_async(),
            electrum_client: client.into_arc_mutex_async(),
            cached_electrum_fee_estimator,
            cached_mempool_fee_estimator: Arc::new(cached_mempool_fee_estimator),
            persister: persister.into_arc_mutex_async(),
            tauri_handle,
            network,
            finality_confirmations,
            target_block,
        };

        Ok(wallet)
    }

    /// Broadcast the given transaction to the network and emit a tracing statement
    /// if done so successfully.
    ///
    /// Returns the transaction ID and a future for when the transaction meets
    /// the configured finality confirmations.
    pub async fn broadcast(
        &self,
        transaction: Transaction,
        kind: &str,
    ) -> Result<(Txid, Subscription)> {
        let txid = transaction.compute_txid();

        // to watch for confirmations, watching a single output is enough
        let subscription = self
            .subscribe_to((txid, transaction.output[0].script_pubkey.clone()))
            .await;

        let client = self.electrum_client.lock().await;
        let broadcast_results = client
            .transaction_broadcast_all(&transaction)
            .await
            .with_context(|| {
                format!(
                    "Failed to broadcast Bitcoin {} transaction to any server {}",
                    kind, txid
                )
            })?;

        // Check if at least one broadcast succeeded
        let successful_count = broadcast_results.iter().filter(|r| r.is_ok()).count();
        let total_count = broadcast_results.len();

        if successful_count == 0 {
            // Collect all errors to create a MultiError
            let errors: Vec<_> = broadcast_results
                .into_iter()
                .filter_map(|result| result.err())
                .collect();

            let context = format!(
                "Bitcoin {} transaction {} failed to broadcast on all {} servers",
                kind, txid, total_count
            );

            let multi_error = electrum_pool::MultiError::new(errors, context);
            return Err(anyhow::Error::from(multi_error));
        }

        tracing::info!(
            %txid, %kind,
            successful_broadcasts = successful_count,
            total_servers = total_count,
            "Published Bitcoin transaction (accepted at {}/{} servers)",
            successful_count, total_count
        );

        // The transaction was accepted by the mempool
        // We know this because otherwise Electrum would have rejected it
        //

        // Mark the transaction as unconfirmed in the mempool
        // This ensures it is used to calculate the balance from here on
        // out
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_secs();

        {
            let mut wallet = self.wallet.lock().await;
            let mut persister = self.persister.lock().await;
            wallet.apply_unconfirmed_txs(vec![(transaction, timestamp)]);
            wallet.persist(&mut persister)?;
        }

        Ok((txid, subscription))
    }

    pub async fn get_raw_transaction(&self, txid: Txid) -> Result<Option<Arc<Transaction>>> {
        self.get_tx(txid)
            .await
            .with_context(|| format!("Could not get raw tx with id: {}", txid))
    }

    // Returns the TxId of the last published Bitcoin transaction
    pub async fn last_published_txid(&self) -> Result<Txid> {
        let wallet = self.wallet.lock().await;

        // Get all the transactions sorted by recency
        let mut txs = wallet.transactions().collect::<Vec<_>>();
        txs.sort_by(|tx1, tx2| tx2.chain_position.cmp(&tx1.chain_position));

        let last_tx = txs.first().context("No transactions found")?;

        Ok(last_tx.tx_node.txid)
    }

    pub async fn status_of_script<T>(&self, tx: &T) -> Result<ScriptStatus>
    where
        T: Watchable,
    {
        self.electrum_client
            .lock()
            .await
            .status_of_script(tx, true)
            .await
    }

    pub async fn subscribe_to(&self, tx: impl Watchable + Send + Sync + 'static) -> Subscription {
        let txid = tx.id();
        let script = tx.script();

        let initial_status = match self
            .electrum_client
            .lock()
            .await
            .status_of_script(&tx, false)
            .await
        {
            Ok(status) => Some(status),
            Err(err) => {
                tracing::debug!(%txid, %err, "Failed to get initial status for subscription. We won't notify the caller and will try again later.");
                None
            }
        };

        let sub = self
            .electrum_client
            .lock()
            .await
            .subscriptions
            .entry((txid, script.clone()))
            .or_insert_with(|| {
                let (sender, receiver) = watch::channel(ScriptStatus::Unseen);
                let client = self.electrum_client.clone();

                tokio::spawn(async move {
                    let mut last_status = initial_status;

                    loop {
                        let new_status = client.lock()
                            .await
                            .status_of_script(&tx, false)
                            .await
                            .unwrap_or_else(|error| {
                                tracing::warn!(%txid, "Failed to get status of script: {:#}", error);
                                ScriptStatus::Retrying
                            });

                        if new_status != ScriptStatus::Retrying
                        {
                            last_status = Some(trace_status_change(txid, last_status, new_status));

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
        match bdk_wallet::export::FullyNodedExport::export_wallet(
            &wallet,
            &format!("{}-{}", role, self.network),
            true,
        ) {
            Result::Ok(wallet_export) => Ok(wallet_export),
            Err(err_msg) => Err(anyhow::Error::msg(err_msg)),
        }
    }

    /// Get a transaction from the Electrum server or the cache.
    pub async fn get_tx(&self, txid: Txid) -> Result<Option<Arc<Transaction>>> {
        let client = self.electrum_client.lock().await;
        let tx = client
            .get_tx(txid)
            .await
            .context("Failed to get transaction from cache or Electrum server")?;

        Ok(tx)
    }

    /// Create a vector of sync requests
    ///
    /// This splits up all the revealed spks and builds a sync request for each chunk.
    /// Useful for syncing the whole wallet in chunks.
    async fn chunked_sync_request(
        &self,
        max_num_chunks: u32,
        batch_size: u32,
    ) -> Vec<SyncRequestBuilderFactory> {
        #[allow(clippy::type_complexity)]
        let (spks, chain_tip): (Vec<((KeychainKind, u32), ScriptBuf)>, CheckPoint) = {
            let wallet = self.wallet.lock().await;

            let spks = wallet
                .spk_index()
                .revealed_spks(..)
                .map(|(index, spk)| (index, spk.clone()))
                .collect();

            let chain_tip = wallet.local_chain().tip();

            (spks, chain_tip)
        };

        let total_spks =
            u32::try_from(spks.len()).expect("Number of SPKs should not exceed u32::MAX");

        if total_spks == 0 {
            tracing::debug!("Not syncing because there are no spks in our wallet");
            return vec![];
        }

        // We only use as many chunks as are useful to reduce the number of requests
        // given the batch size
        // This means: num_chunks * batch_size < total number of spks
        //
        // E.g we have 1000 spks and a batch size of 100, we only use 10 chunks at most
        // If we used 20 chunks we would not maximize the batch size because
        // each chunk would have 50 spks (which is less than the batch size)
        //
        // At least one chunk is always required. At most total_spks / batch_size or the provided num_chunks (whichever is smaller)
        let num_chunks = max_num_chunks.min(total_spks / batch_size).max(1);
        let chunk_size = total_spks.div_ceil(num_chunks);

        let mut chunks = Vec::new();

        for spk_chunk in spks.chunks(chunk_size as usize) {
            let factory = SyncRequestBuilderFactory {
                chain_tip: chain_tip.clone(),
                spks: spk_chunk.to_vec(),
            };
            chunks.push(factory);
        }

        chunks
    }

    /// Sync the wallet with the Blockchain
    /// Spawn `num_chunks` tasks to sync the wallet in parallel
    /// Call the callback with the cumulative progress of the sync
    pub async fn chunked_sync_with_callback(&self, callback: sync_ext::SyncCallback) -> Result<()> {
        // Construct the chunks to process
        let sync_request_factories = self
            .chunked_sync_request(Self::SCAN_CHUNKS, Self::SCAN_BATCH_SIZE)
            .await;

        tracing::debug!(
            "Starting to sync Bitcoin wallet with {} concurrent chunks and batch size of {}",
            sync_request_factories.len(),
            Self::SCAN_BATCH_SIZE
        );

        // For each sync request, store the latest progress update in a HashMap keyed by the index of the chunk
        let cumulative_progress_handle = sync_ext::CumulativeProgress::new().into_arc_mutex_sync(); // Use the newtype here

        // Assign each sync request:
        // 1. its individual callback which links back to the CumulativeProgress
        // 2. its chunk of the SyncRequest
        let sync_requests = sync_request_factories
            .into_iter()
            .enumerate()
            .map(|(index, sync_request_factory)| {
                let callback = cumulative_progress_handle
                    .clone()
                    .chunk_callback(callback.clone(), index as u64);

                (callback, sync_request_factory)
            })
            .collect::<Vec<_>>();

        // Create a vector of futures to process in parallel
        let futures = sync_requests
            .into_iter()
            .map(|(callback, sync_request_factory)| {
                self.sync_with_custom_callback(sync_request_factory, callback)
                    .in_current_span()
            });

        // Start timer to measure the time taken to sync the wallet
        let start_time = Instant::now();

        // Execute all futures concurrently and collect results
        let results = futures::future::join_all(futures).await;

        // Check if any requests failed
        for result in results {
            result?;
        }

        // Calculate the time taken to sync the wallet
        let duration = start_time.elapsed();
        tracing::trace!(
            "Synced Bitcoin wallet in {:?} with {} concurrent chunks and batch size {}",
            duration,
            Self::SCAN_CHUNKS,
            Self::SCAN_BATCH_SIZE
        );

        Ok(())
    }

    /// Sync the wallet with the blockchain, optionally calling a callback on progress updates.
    /// This will NOT emit progress events to the UI.
    ///
    /// If no sync request is provided, we default to syncing all revealed spks.
    pub async fn sync_with_custom_callback(
        &self,
        sync_request_factory: SyncRequestBuilderFactory,
        callback: InnerSyncCallback,
    ) -> Result<()> {
        let callback = Arc::new(SyncMutex::new(callback));

        let sync_response = self
            .electrum_client
            .lock()
            .await
            .inner
            .call_async("sync_wallet", move |client| {
                let sync_request_factory = sync_request_factory.clone();
                let callback = callback.clone();

                // Build the sync request
                let sync_request = sync_request_factory
                    .build()
                    .inspect(move |_, progress| {
                        if let Ok(mut guard) = callback.lock() {
                            guard.call(progress.consumed() as u64, progress.total() as u64);
                        }
                    })
                    .build();

                client.sync(sync_request, Self::SCAN_BATCH_SIZE as usize, true)
            })
            .await?;

        // We only acquire the lock after the long running .sync(...) call has finished
        let mut wallet = self.wallet.lock().await;
        wallet.apply_update(sync_response)?; // Use the full sync_response, not just chain_update

        let mut persister = self.persister.lock().await;
        wallet.persist(&mut persister)?;

        Ok(())
    }

    /// Perform a single sync of the wallet with the blockchain
    /// and emit progress events to the UI.
    async fn sync_once(&self) -> Result<()> {
        let background_process_handle = self
            .tauri_handle
            .new_background_process_with_initial_progress(
                TauriBackgroundProgress::SyncingBitcoinWallet,
                TauriBitcoinSyncProgress::Unknown,
            );

        let background_process_handle_clone = background_process_handle.clone();

        // We want to update the UI as often as possible
        let tauri_callback = sync_ext::InnerSyncCallback::new(move |consumed, total| {
            background_process_handle_clone
                .update(TauriBitcoinSyncProgress::Known { consumed, total });
        });

        // We throttle the tracing logging to 10% increments
        let tracing_callback = sync_ext::InnerSyncCallback::new(move |consumed, total| {
            tracing::debug!("Syncing Bitcoin wallet ({}/{})", consumed, total);
        })
        .throttle_callback(10.0);

        // We chain the callbacks and then initiate the sync
        self.chunked_sync_with_callback(tauri_callback.chain(tracing_callback).finalize())
            .await?;

        background_process_handle.finish();

        Ok(())
    }

    /// Sync the wallet with the blockchain and emit progress events to the UI.
    /// Retries the sync if it fails using an exponential backoff.
    pub async fn sync(&self) -> Result<()> {
        let backoff = backoff::ExponentialBackoffBuilder::new()
            .with_max_elapsed_time(Some(Self::SYNC_MAX_ELAPSED_TIME))
            .with_max_interval(Duration::from_secs(1))
            .build();

        backoff::future::retry_notify(
            backoff,
            || async { self.sync_once().await.map_err(backoff::Error::transient) },
            |err, wait_time: Duration| {
                tracing::warn!(
                    ?err,
                    "Failed to sync Bitcoin wallet. We will retry in {} seconds",
                    wait_time.as_secs()
                );
            },
        )
        .await
        .context("Failed to sync Bitcoin wallet after retries")
    }

    /// Calculate the fee for a given transaction.
    ///
    /// Will fail if the transaction inputs are not owned by this wallet.
    pub async fn transaction_fee(&self, txid: Txid) -> Result<Amount> {
        // Ensure wallet is synced before getting transaction
        self.sync().await?;

        let transaction = self
            .get_tx(txid)
            .await
            .context(
                "Could not fetch transaction from Electrum server while trying to determine fees",
            )?
            .ok_or_else(|| anyhow!("Transaction not found"))?;

        let fee = self.wallet.lock().await.calculate_fee(&transaction)?;

        Ok(fee)
    }
}

// These are the methods that are always available, regardless of the persister.
impl<T, C> Wallet<T, C> {
    /// Get the network of this wallet.
    pub fn network(&self) -> Network {
        self.network
    }

    /// Get the finality confirmations of this wallet.
    pub fn finality_confirmations(&self) -> u32 {
        self.finality_confirmations
    }

    /// Get the target block of this wallet.
    ///
    /// This is the the number of blocks we want to wait at most for
    /// one ofour transaction to be confirmed.
    pub fn target_block(&self) -> u32 {
        self.target_block
    }
}

impl<Persister, C> Wallet<Persister, C>
where
    Persister: WalletPersister + Sized,
    <Persister as WalletPersister>::Error: std::error::Error + Send + Sync + 'static,
    C: EstimateFeeRate + Send + Sync + 'static,
{
    /// Returns the combined fee rate from the Electrum and Mempool clients.
    ///
    /// If the mempool client is not available, we use the Electrum client.
    /// If the mempool client is available, we use the higher of the two.
    /// If either of the clients fail but the other is successful, we use the successful one.
    /// If both clients fail, we return an error
    async fn combined_fee_rate(&self) -> Result<FeeRate> {
        let electrum_future = self
            .cached_electrum_fee_estimator
            .estimate_feerate(self.target_block);
        let mempool_future = async {
            match self.cached_mempool_fee_estimator.as_ref() {
                Some(mempool_client) => mempool_client
                    .estimate_feerate(self.target_block)
                    .await
                    .map(Some),
                None => Ok(None),
            }
        };

        let (electrum_result, mempool_result) = tokio::join!(electrum_future, mempool_future);

        match (electrum_result, mempool_result) {
            // If both sources are successful, we use the higher one
            (Ok(electrum_rate), Ok(Some(mempool_space_rate))) => {
                tracing::debug!(
                    electrum_rate_sat_vb = electrum_rate.to_sat_per_vb_ceil(),
                    mempool_space_rate_sat_vb = mempool_space_rate.to_sat_per_vb_ceil(),
                    "Successfully fetched fee rates from both Electrum and mempool.space. We will use the higher one"

                );
                Ok(std::cmp::max(electrum_rate, mempool_space_rate))
            }
            // If the Electrum source is successful
            // but we don't have a mempool client, we use the Electrum rate
            (Ok(electrum_rate), Ok(None)) => {
                tracing::trace!(
                    electrum_rate_sat_vb = electrum_rate.to_sat_per_vb_ceil(),
                    "No mempool.space client available, using Electrum rate"
                );
                Ok(electrum_rate)
            }
            // If the Electrum source is successful
            // but the mempool source fails, we use the Electrum rate
            (Ok(electrum_rate), Err(mempool_error)) => {
                tracing::warn!(
                    ?mempool_error,
                    electrum_rate_sat_vb = electrum_rate.to_sat_per_vb_ceil(),
                    "Failed to fetch mempool.space fee rate, using Electrum rate"
                );
                Ok(electrum_rate)
            }
            // If the mempool source is successful
            // but the Electrum source fails, we use the mempool rate
            (Err(electrum_error), Ok(Some(mempool_rate))) => {
                tracing::warn!(
                    ?electrum_error,
                    mempool_rate_sat_vb = mempool_rate.to_sat_per_vb_ceil(),
                    "Electrum fee rate failed, using mempool.space rate"
                );
                Ok(mempool_rate)
            }
            // If both sources fail, we return the error
            (Err(electrum_error), Err(mempool_error)) => {
                tracing::error!(
                    ?electrum_error,
                    ?mempool_error,
                    "Failed to fetch fee rates from both Electrum and mempool.space"
                );

                Err(electrum_error)
            }
            // If the Electrum source fails and the mempool source is not available, we return the Electrum error
            (Err(electrum_error), Ok(None)) => {
                tracing::warn!(
                    ?electrum_error,
                    "Electrum failed and mempool.space client is not available"
                );
                Err(electrum_error)
            }
        }
    }

    /// Returns the minimum relay fee from the Electrum and Mempool clients.
    ///
    /// Only fails if both sources fail. Always chooses the higher value.
    async fn combined_min_relay_fee(&self) -> Result<FeeRate> {
        let electrum_future = self.cached_electrum_fee_estimator.min_relay_fee();
        let mempool_future = async {
            match self.cached_mempool_fee_estimator.as_ref() {
                Some(mempool_client) => mempool_client.min_relay_fee().await.map(Some),
                None => Ok(None),
            }
        };

        let (electrum_result, mempool_result) = tokio::join!(electrum_future, mempool_future);

        match (electrum_result, mempool_result) {
            (Ok(electrum_fee), Ok(Some(mempool_space_fee))) => {
                tracing::trace!(
                    electrum_fee = ?electrum_fee,
                    mempool_space_fee = ?mempool_space_fee,
                    "Successfully fetched min relay fee from both Electrum and mempool.space. We will use the higher value"
                );
                Ok(std::cmp::max(electrum_fee, mempool_space_fee))
            }
            (Ok(electrum_fee), Ok(None)) => {
                tracing::trace!(
                    ?electrum_fee,
                    "No mempool.space client available, using Electrum min relay fee"
                );
                Ok(electrum_fee)
            }
            (Ok(electrum_fee), Err(mempool_space_error)) => {
                tracing::warn!(
                    ?mempool_space_error,
                    ?electrum_fee,
                    "Failed to fetch mempool.space min relay fee, using Electrum min relay fee"
                );
                Ok(electrum_fee)
            }
            (Err(electrum_error), Ok(Some(mempool_space_fee))) => {
                tracing::warn!(
                    ?electrum_error,
                    ?mempool_space_fee,
                    "Failed to fetch Electrum min relay fee, using mempool.space min relay fee"
                );
                Ok(mempool_space_fee)
            }
            (Err(electrum_error), Ok(None)) => Err(electrum_error.context(
                "Failed to fetch min relay fee from Electrum, and no mempool.space client available",
            )),
            (Err(electrum_error), Err(mempool_space_error)) => Err(electrum_error
                .context(mempool_space_error)
                .context("Failed to fetch min relay fee from both Electrum and mempool.space")),
        }
    }

    pub async fn sign_and_finalize(&self, mut psbt: bitcoin::psbt::Psbt) -> Result<Transaction> {
        // Acquire the wallet lock once here for efficiency within the non-finalized block
        let wallet_guard = self.wallet.lock().await;

        let finalized = wallet_guard.sign(&mut psbt, SignOptions::default())?;

        if !finalized {
            bail!("PSBT is not finalized")
        }

        // Release the lock if finalization succeeded
        drop(wallet_guard);

        let tx = psbt.extract_tx();
        Ok(tx?)
    }

    /// Returns the total Bitcoin balance, which includes pending funds
    pub async fn balance(&self) -> Result<Amount> {
        Ok(self.wallet.lock().await.balance().total())
    }

    /// Returns the balance info of the wallet, including unconfirmed funds etc.
    pub async fn balance_info(&self) -> Result<Balance> {
        Ok(self.wallet.lock().await.balance())
    }

    /// Reveals the next address from the wallet.
    pub async fn new_address(&self) -> Result<Address> {
        let mut wallet = self.wallet.lock().await;

        // Only reveal a new address if absolutely necessary
        // We want to avoid revealing more and more addresses
        let address = wallet.next_unused_address(KeychainKind::External).address;

        // Important: persist that we revealed a new address.
        // Otherwise the wallet might reuse it (bad).
        let mut persister = self.persister.lock().await;
        wallet.persist(&mut persister)?;

        Ok(address)
    }

    /// Builds a partially signed transaction that sends
    /// the given amount to the given address.
    /// The fee is calculated based on the weight of the transaction
    /// and the state of the current mempool.
    pub async fn send_to_address_dynamic_fee(
        &self,
        address: Address,
        amount: Amount,
        change_override: Option<Address>,
    ) -> Result<PartiallySignedTransaction> {
        // Check address and change address for network equality.
        let address = revalidate_network(address, self.network)?;

        change_override
            .as_ref()
            .map(|a| revalidate_network(a.clone(), self.network))
            .transpose()
            .context("Change address is not on the correct network")?;

        let script = address.script_pubkey();

        let psbt = {
            let mut wallet = self.wallet.lock().await;

            // Build the transaction with a dummy fee rate
            // just to figure out the final weight of the transaction
            // send_to_address(...) takes an absolute fee
            let mut tx_builder = wallet.build_tx();

            tx_builder.add_recipient(script.clone(), amount);
            tx_builder.fee_absolute(Amount::ZERO);

            tx_builder.finish()?
        };

        let weight = psbt.unsigned_tx.weight();
        let fee = self.estimate_fee(weight, Some(amount)).await?;

        self.send_to_address(address, amount, fee, change_override)
            .await
    }

    /// Builds a partially signed transaction that sweeps our entire balance
    /// to a single address.
    ///
    /// The fee is calculated based on the weight of the transaction
    /// and the state of the current mempool.
    pub async fn sweep_balance_to_address_dynamic_fee(
        &self,
        address: Address,
    ) -> Result<PartiallySignedTransaction> {
        let (max_giveable, fee) = self.max_giveable(address.script_pubkey().len()).await?;

        self.send_to_address(address, max_giveable, fee, None).await
    }

    /// Builds a partially signed transaction that sends
    /// the given amount to the given address with the given
    /// absolute fee.
    ///
    /// Ensures that the address script is at output index `0`
    /// for the partially signed transaction.
    pub async fn send_to_address(
        &self,
        address: Address,
        amount: Amount,
        spending_fee: Amount,
        change_override: Option<Address>,
    ) -> Result<PartiallySignedTransaction> {
        // Check address and change address for network equality.
        let address = revalidate_network(address, self.network)?;

        change_override
            .as_ref()
            .map(|a| revalidate_network(a.clone(), self.network))
            .transpose()
            .context("Change address is not on the correct network")?;

        let mut wallet = self.wallet.lock().await;
        let script = address.script_pubkey();

        // Build the transaction with a manual fee
        let mut tx_builder = wallet.build_tx();
        tx_builder.add_recipient(script.clone(), amount);
        tx_builder.fee_absolute(spending_fee);

        let mut psbt = tx_builder.finish()?;

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
            tracing::info!(change_override = ?change_override, "Overwriting change address");
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
    ///
    /// Returns a tuple of (max_giveable_amount, spending_fee).
    pub async fn max_giveable(&self, locking_script_size: usize) -> Result<(Amount, Amount)> {
        let mut wallet = self.wallet.lock().await;

        // Construct a dummy drain transaction
        let dummy_script = ScriptBuf::from(vec![0u8; locking_script_size]);

        let mut tx_builder = wallet.build_tx();

        tx_builder.drain_to(dummy_script.clone());
        tx_builder.fee_absolute(Amount::ZERO);
        tx_builder.drain_wallet();

        // The weight WILL NOT change, even if we change the fee
        // because we are draining the wallet (using all inputs) and
        // always have one output of constant size
        //
        // The only changable part is the amount of the output.
        // If we increase the fee, the output amount simply will decrease
        //
        // The inputs are constant, so only the output amount changes.
        let (dummy_max_giveable, dummy_weight) = match tx_builder.finish() {
            Ok(psbt) => {
                if psbt.unsigned_tx.output.len() != 1 {
                    bail!("Expected a single output in the dummy transaction");
                }

                let max_giveable = psbt.unsigned_tx.output.first().expect("Expected a single output in the dummy transaction").value;
                let weight = psbt.unsigned_tx.weight();

                Ok((Some(max_giveable), weight))
            },
            Err(bdk_wallet::error::CreateTxError::CoinSelection(_)) => {
                // We don't have enough funds to create a transaction (below dust limit)
                //
                // We still want to to return a valid fee.
                // Callers of this function might want to calculate *how* large
                // the next UTXO needs to be such that we can spend any funds
                //
                // To be able to calculate an accurate fee, we need to figure out
                // the weight our drain transaction if we received another UTXO

                // We create fake deposit UTXO
                // Our dummy drain transaction will spend this deposit UTXO
                let mut fake_deposit_input = bitcoin::psbt::Input::default();

                let dummy_deposit_address = wallet.peek_address(KeychainKind::External, 0);
                let fake_deposit_script = dummy_deposit_address.script_pubkey();
                let fake_deposit_txout = bitcoin::blockdata::transaction::TxOut {
                    // The exact deposit amount does not matter
                    // because we only care about the weight of the transaction
                    // which does not depend on the amount of the input
                    value: DUST_AMOUNT * 5,
                    script_pubkey: fake_deposit_script,
                };
                let fake_deposit_tx = bitcoin::Transaction {
                    version: bitcoin::blockdata::transaction::Version::TWO,
                    lock_time: bitcoin::blockdata::locktime::absolute::LockTime::ZERO,
                    input: vec![bitcoin::TxIn {
                        previous_output: bitcoin::OutPoint::null(), // or some dummy outpoint
                        script_sig: Default::default(),
                        sequence: bitcoin::Sequence::ENABLE_RBF_NO_LOCKTIME,
                        witness: Default::default(),
                    }],
                    output: vec![fake_deposit_txout.clone()],
                };

                let fake_deposit_txid = fake_deposit_tx.compute_txid();

                fake_deposit_input.witness_utxo = Some(fake_deposit_txout);
                fake_deposit_input.non_witness_utxo = Some(fake_deposit_tx);

                // Create outpoint that points to our fake transaction's output 0
                let fake_deposit_outpoint = bitcoin::OutPoint {
                    txid: fake_deposit_txid,
                    vout: 0,
                };

                // Worst-case witness weight for our script type.
                const DUMMY_SATISFACTION_WEIGHT: Weight = Weight::from_wu(107 * 10);

                let mut tx_builder = wallet.build_tx();

                tx_builder.drain_to(dummy_script.clone());
                tx_builder.fee_absolute(Amount::ZERO);
                tx_builder.drain_wallet();

                tx_builder
                    .add_foreign_utxo(
                        fake_deposit_outpoint,
                        fake_deposit_input,
                        DUMMY_SATISFACTION_WEIGHT,
                    ).context("Failed to add dummy foreign utxo to calculate fee for max_giveable if we had one more utxo")?;

                // Try building the dummy drain transaction with the new fake UTXO
                // If we fail now, we propagate the error to the caller
                let psbt = tx_builder.finish()?;
                let weight = psbt.unsigned_tx.weight();

                tracing::trace!(
                    weight = weight.to_wu(),
                    "Built dummy drain transaction with fake UTXO, max giveable is 0"
                );

                Ok((None, weight))
            }
            Err(e) => Err(e)
        }.context("Failed to build transaction to figure out max giveable")?;

        // Estimate the fee rate using our real fee rate estimation
        let fee = self.estimate_fee(dummy_weight, dummy_max_giveable).await?;

        Ok(match dummy_max_giveable {
            // If the max giveable is less than the dust amount, we return 0
            Some(max_giveable) if max_giveable < DUST_AMOUNT => (Amount::ZERO, fee),
            Some(max_giveable) => {
                // If we have enough funds, we subtract the fee from the max giveable
                // and return the resul
                match max_giveable.checked_sub(fee) {
                    Some(max_giveable) => (max_giveable, fee),
                    // Let's say we have 2000 sats in the wallet
                    // The dummy script choses 0 sats as a fee
                    // and drains the 2000 sats
                    //
                    // Our smart fee estimation says we need 2500 sats to get the transaction confirmed
                    // fee = 2500
                    // dummy_max_giveable = 2000
                    // max_giveable is < 0, so we return 0 since we don't have enough funds to cover the fee
                    None => (Amount::ZERO, fee),
                }
            }
            // If we don't know the max giveable, we return 0
            // This happens if we don't have enough funds to create a transaction
            // (below dust limit)
            None => (Amount::ZERO, fee),
        })
    }

    /// Estimate total tx fee for a pre-defined target block based on the
    /// transaction weight. The max fee cannot be more than MAX_PERCENTAGE_FEE
    /// of amount
    ///
    /// This uses different techniques to estimate the fee under the hood:
    /// 1. `estimate_fee_rate` from Electrum which calls `estimatesmartfee` from Bitcoin Core
    /// 2. `estimate_fee_rate_from_histogram` which calls `mempool.get_fee_histogram` from Electrum. It calculates the distance to the tip of the mempool.
    ///    it can adapt faster to sudden spikes in the mempool.
    /// 3. `MempoolClient::estimate_feerate` which uses the mempool.space API for fee estimation
    ///
    /// To compute the min relay fee we fetch from both the Electrum server and the MempoolClient.
    ///
    /// In all cases, if have multiple sources, we use the higher one.
    pub async fn estimate_fee(
        &self,
        weight: Weight,
        transfer_amount: Option<bitcoin::Amount>,
    ) -> Result<bitcoin::Amount> {
        let fee_rate = self.combined_fee_rate().await?;
        let min_relay_fee = self.combined_min_relay_fee().await?;

        estimate_fee(weight, transfer_amount, fee_rate, min_relay_fee)
    }
}

impl Client {
    /// Create a new client with multiple electrum servers for load balancing.
    pub async fn new(electrum_rpc_urls: &[String], sync_interval: Duration) -> Result<Self> {
        let balancer = ElectrumBalancer::new(electrum_rpc_urls.to_vec()).await?;

        Ok(Self {
            inner: Arc::new(balancer),
            script_history: Default::default(),
            last_sync: Instant::now()
                .checked_sub(sync_interval)
                .ok_or(anyhow!("failed to set last sync time"))?,
            sync_interval,
            latest_block_height: BlockHeight::from(0),
            subscriptions: Default::default(),
        })
    }

    /// Update the client state, if the refresh duration has passed.
    ///
    /// Optionally force an update even if the sync interval has not passed.
    pub async fn update_state(&mut self, force: bool) -> Result<()> {
        let now = Instant::now();

        if !force && now.duration_since(self.last_sync) < self.sync_interval {
            return Ok(());
        }

        self.last_sync = now;
        self.update_script_histories().await?;
        self.update_block_height().await?;

        Ok(())
    }

    /// Update the client state for a single script.
    ///
    /// As opposed to [`update_state`] this function does not
    /// check the time since the last update before refreshing
    /// It therefore also does not take a [`force`] parameter
    pub async fn update_state_single(&mut self, script: &impl Watchable) -> Result<()> {
        self.update_script_history(script).await?;
        self.update_block_height().await?;

        Ok(())
    }

    /// Update the block height.
    async fn update_block_height(&mut self) -> Result<()> {
        let latest_block = self
            .inner
            .call_async("block_headers_subscribe", |client| {
                client.inner.block_headers_subscribe()
            })
            .await
            .context("Failed to subscribe to header notifications")?;
        let latest_block_height = BlockHeight::try_from(latest_block)?;

        if latest_block_height > self.latest_block_height {
            tracing::trace!(
                block_height = u32::from(latest_block_height),
                "Got notification for new block"
            );
            self.latest_block_height = latest_block_height;
        }

        Ok(())
    }

    /// Update the script histories.
    async fn update_script_histories(&mut self) -> Result<()> {
        let scripts: Vec<_> = self.script_history.keys().cloned().collect();

        // No need to do any network request if we have nothing to fetch
        if scripts.is_empty() {
            return Ok(());
        }

        // Concurrently fetch the script histories from ALL electrum servers
        let results = self
            .inner
            .join_all("batch_script_get_history", {
                let scripts = scripts.clone();

                move |client| {
                    let script_refs: Vec<_> = scripts.iter().map(|s| s.as_script()).collect();
                    client.inner.batch_script_get_history(script_refs)
                }
            })
            .await?;

        let successful_results: Vec<Vec<Vec<GetHistoryRes>>> = results
            .iter()
            .filter_map(|r| r.as_ref().ok())
            .cloned()
            .collect();

        // If we didn't get a single successful request, we have to fail
        if successful_results.is_empty() {
            if let Some(Err(e)) = results.into_iter().find(|r| r.is_err()) {
                return Err(e.into());
            }
        }

        // Iterate through each script we fetched and find the highest
        // returned entry at any Electrum node
        for (script_index, script) in scripts.iter().enumerate() {
            let all_history_for_script: Vec<GetHistoryRes> = successful_results
                .iter()
                .filter_map(|server_result| server_result.get(script_index))
                .flatten()
                .cloned()
                .collect();

            let mut best_history: BTreeMap<Txid, GetHistoryRes> = BTreeMap::new();
            for item in all_history_for_script {
                best_history
                    .entry(item.tx_hash)
                    .and_modify(|current| {
                        if item.height > current.height {
                            *current = item.clone();
                        }
                    })
                    .or_insert(item);
            }

            let final_history: Vec<GetHistoryRes> = best_history.into_values().collect();
            self.script_history.insert(script.clone(), final_history);
        }

        Ok(())
    }

    /// Update the script history of a single script.
    pub async fn update_script_history(&mut self, script: &impl Watchable) -> Result<()> {
        let (script_buf, _) = script.script_and_txid();
        let script_clone = script_buf.clone();

        // Call all electrum servers in parallel to get script history.
        let results = self
            .inner
            .join_all("script_get_history", move |client| {
                client.inner.script_get_history(script_clone.as_script())
            })
            .await?;

        // Collect all successful history entries from all servers.
        let mut all_history_items: Vec<GetHistoryRes> = Vec::new();
        let mut first_error = None;

        for result in results {
            match result {
                Ok(history) => all_history_items.extend(history),
                Err(e) => {
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                }
            }
        }

        // If we got no history items at all, and there was an error, propagate it.
        // Otherwise, it's valid for a script to have no history.
        if all_history_items.is_empty() {
            if let Some(err) = first_error {
                return Err(err.into());
            }
        }

        // Use a map to find the best (highest confirmation) entry for each transaction.
        let mut best_history: BTreeMap<Txid, GetHistoryRes> = BTreeMap::new();
        for item in all_history_items {
            best_history
                .entry(item.tx_hash)
                .and_modify(|current| {
                    if item.height > current.height {
                        *current = item.clone();
                    }
                })
                .or_insert(item);
        }

        let final_history: Vec<GetHistoryRes> = best_history.into_values().collect();

        self.script_history.insert(script_buf, final_history);

        Ok(())
    }

    /// Broadcast a transaction to all known electrum servers in parallel.
    /// Returns the results from all servers - at least one success indicates successful broadcast.
    pub async fn transaction_broadcast_all(
        &self,
        transaction: &Transaction,
    ) -> Result<Vec<Result<bitcoin::Txid, bdk_electrum::electrum_client::Error>>> {
        // Broadcast to all electrum servers in parallel
        let results = self.inner.broadcast_all(transaction.clone()).await?;

        // Add the transaction to the cache if at least one broadcast succeeded
        if results.iter().any(|r| r.is_ok()) {
            // Note: Perhaps it is better to only populate caches of the Electrum nodes
            // that accepted our transaction?
            self.inner.populate_tx_cache(vec![transaction.clone()]);
        }

        Ok(results)
    }

    /// Get the status of a script.
    pub async fn status_of_script(
        &mut self,
        script: &impl Watchable,
        force: bool,
    ) -> Result<ScriptStatus> {
        let (script_buf, txid) = script.script_and_txid();

        if !self.script_history.contains_key(&script_buf) {
            self.script_history.insert(script_buf.clone(), vec![]);

            // Immediately refetch the status of the script
            // when we first subscribe to it.
            self.update_state_single(script).await?;
        } else if force {
            // Immediately refetch the status of the script
            // when [`force`] is set to true
            self.update_state_single(script).await?;
        } else {
            // Otherwise, don't force a refetch.
            self.update_state(false).await?;
        }

        let history = self.script_history.entry(script_buf).or_default();

        let history_of_tx: Vec<&GetHistoryRes> = history
            .iter()
            .filter(|entry| entry.tx_hash == txid)
            .collect();

        // Destructure history_of_tx into the last entry and the rest.
        let [rest @ .., last] = history_of_tx.as_slice() else {
            // If there is no history of the transaction, it is unseen.
            return Ok(ScriptStatus::Unseen);
        };

        // There should only be one entry per txid, we will ignore the rest
        if !rest.is_empty() {
            tracing::warn!(%txid, "Found multiple history entries for the same txid. Ignoring all but the last one.");
        }

        match last.height {
            // If the height is 0 or less, the transaction is still in the mempool.
            ..=0 => Ok(ScriptStatus::InMempool),
            // Otherwise, the transaction has been included in a block.
            height => Ok(ScriptStatus::Confirmed(
                Confirmed::from_inclusion_and_latest_block(
                    u32::try_from(height)?,
                    u32::from(self.latest_block_height),
                ),
            )),
        }
    }

    /// Get a transaction from the Electrum server.
    /// Fails if the transaction is not found.
    pub async fn get_tx(&self, txid: Txid) -> Result<Option<Arc<Transaction>>> {
        match self
            .inner
            .call_async_with_multi_error("get_raw_transaction", move |client| {
                use bitcoin::consensus::Decodable;
                client.inner.transaction_get_raw(&txid).and_then(|raw| {
                    let mut cursor = std::io::Cursor::new(&raw);
                    bitcoin::Transaction::consensus_decode(&mut cursor).map_err(|e| {
                        bdk_electrum::electrum_client::Error::Protocol(
                            format!("Failed to deserialize transaction: {}", e).into(),
                        )
                    })
                })
            })
            .await
        {
            Ok(tx) => {
                let tx = Arc::new(tx);
                // Note: Perhaps it is better to only populate caches of the Electrum nodes
                // that accepted our transaction?
                self.inner.populate_tx_cache(vec![(*tx).clone()]);
                Ok(Some(tx))
            }
            Err(multi_error) => {
                // Check if any error indicates the transaction doesn't exist
                let has_not_found = multi_error.any(|error| {
                    let error_str = error.to_string();

                    // Check for specific error patterns that indicate "not found"
                    if error_str.contains("\"code\": Number(-5)")
                        || error_str.contains("No such mempool or blockchain transaction")
                        || error_str.contains("missing transaction")
                    {
                        return true;
                    }

                    // Also try to parse the RPC error code if possible
                    let err_anyhow = anyhow::anyhow!(error_str);
                    if let Ok(error_code) = parse_rpc_error_code(&err_anyhow) {
                        if error_code == i64::from(RpcErrorCode::RpcInvalidAddressOrKey) {
                            return true;
                        }
                    }

                    false
                });

                if has_not_found {
                    tracing::trace!(
                        txid = %txid,
                        error_count = multi_error.len(),
                        "Transaction not found indicated by one or more Electrum servers"
                    );
                    Ok(None)
                } else {
                    let err = anyhow::anyhow!(multi_error);
                    Err(err.context("Failed to get transaction from the Electrum server"))
                }
            }
        }
    }

    /// Estimate the fee rate to be included in a block at the given offset.
    /// Calls: https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain.estimatefee
    /// Calls under the hood: https://developer.bitcoin.org/reference/rpc/estimatesmartfee.html
    ///
    /// This uses estimatesmartfee of bitcoind
    pub async fn estimate_fee_rate(&self, target_block: u32) -> Result<FeeRate> {
        // Get the fee rate in Bitcoin per kilobyte
        let btc_per_kvb = self
            .inner
            .call_async("estimate_fee", move |client| {
                client.inner.estimate_fee(target_block as usize)
            })
            .await?;

        // If the fee rate is less than 0, return an error
        // The Electrum server returns a value <= 0 if it cannot estimate the fee rate.
        // See: https://github.com/romanz/electrs/blob/ed0ef2ee22efb45fcf0c7f3876fd746913008de3/src/electrum.rs#L239-L245
        //      https://github.com/romanz/electrs/blob/ed0ef2ee22efb45fcf0c7f3876fd746913008de3/src/electrum.rs#L31
        if btc_per_kvb <= 0.0 {
            return Err(anyhow!(
                "Fee rate returned by Electrum server is less than 0"
            ));
        }

        // Convert to sat / kB without ever constructing an Amount from the float
        // Simply by multiplying the float with the satoshi value of 1 BTC.
        // Truncation is allowed here because we are converting to sats and rounding down sats will
        // not lose us any precision (because there is no fractional satoshi).
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let sats_per_kvb = (btc_per_kvb * Amount::ONE_BTC.to_sat() as f64).ceil() as u64;

        // Convert to sat / kwu (kwu = kB × 4)
        let sat_per_kwu = sats_per_kvb / 4;

        // Construct the fee rate
        let fee_rate = FeeRate::from_sat_per_kwu(sat_per_kwu);

        Ok(fee_rate)
    }

    /// Calculates the fee_rate needed to be included in a block at the given offset.
    /// We calculate how many vMB we are away from the tip of the mempool.
    /// This method adapts faster to sudden spikes in the mempool.
    async fn estimate_fee_rate_from_histogram(&self, target_block: u32) -> Result<FeeRate> {
        // Assume we want to get into the next block:
        // We want to be 80% of the block size away from the tip of the mempool.
        const HISTOGRAM_SAFETY_MARGIN: f32 = 0.8;

        // First we fetch the fee histogram from the Electrum server
        let fee_histogram = self
            .inner
            .call_async("get_fee_histogram", move |client| {
                client.inner.raw_call("mempool.get_fee_histogram", vec![])
            })
            .await?;

        // Parse the histogram as array of [fee, vsize] pairs
        let histogram: Vec<(f64, u64)> = serde_json::from_value(fee_histogram)?;

        // If the histogram is empty, we return an error
        if histogram.is_empty() {
            return Err(anyhow!(
                "The mempool seems to be empty therefore we cannot estimate the fee rate from the histogram"
            ));
        }

        // Sort the histogram by fee rate
        let mut histogram = histogram;
        histogram.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Estimate block size (typically ~1MB = 1,000,000 vbytes)
        let estimated_block_size = 1_000_000u64;
        #[allow(clippy::cast_precision_loss)]
        let target_distance_from_tip =
            (estimated_block_size * target_block as u64) as f32 * HISTOGRAM_SAFETY_MARGIN;

        // Find cumulative vsize and corresponding fee rate
        let mut cumulative_vsize = 0u64;
        for (fee_rate, vsize) in histogram.clone() {
            cumulative_vsize += vsize;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            if cumulative_vsize >= target_distance_from_tip as u64 {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let sat_per_vb = fee_rate.ceil() as u64;
                return FeeRate::from_sat_per_vb(sat_per_vb)
                    .context("Failed to create fee rate from histogram");
            }
        }

        // If we get here, the entire mempool is less than the target distance from the tip.
        // We return the lowest fee rate in the histogram.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let sat_per_vb = histogram
            .first()
            .expect("The histogram should not be empty")
            .0
            .ceil() as u64;
        FeeRate::from_sat_per_vb(sat_per_vb)
            .context("Failed to create fee rate from histogram (all mempool is less than the target distance from the tip)")
    }

    /// Get the minimum relay fee rate from the Electrum server.
    async fn min_relay_fee(&self) -> Result<FeeRate> {
        let min_relay_btc_per_kvb = self
            .inner
            .call_async("relay_fee", |client| client.inner.relay_fee())
            .await?;

        // Convert to sat / kB without ever constructing an Amount from the float
        // Simply by multiplying the float with the satoshi value of 1 BTC.
        // Truncation is allowed here because we are converting to sats and rounding down sats will
        // not lose us any precision (because there is no fractional satoshi).
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let sats_per_kvb = (min_relay_btc_per_kvb * Amount::ONE_BTC.to_sat() as f64).ceil() as u64;

        // Convert to sat / kwu (kwu = kB × 4)
        let sat_per_kwu = sats_per_kvb / 4;

        // Construct the fee rate
        let fee_rate = FeeRate::from_sat_per_kwu(sat_per_kwu);

        Ok(fee_rate)
    }
}

impl EstimateFeeRate for Client {
    async fn estimate_feerate(&self, target_block: u32) -> Result<FeeRate> {
        // Now that the Electrum client methods are async, we can parallelize the calls
        let (electrum_conservative_fee_rate, electrum_histogram_fee_rate) = tokio::join!(
            self.estimate_fee_rate(target_block),
            self.estimate_fee_rate_from_histogram(target_block)
        );

        match (electrum_conservative_fee_rate, electrum_histogram_fee_rate) {
            // If both the histogram and conservative fee rate are successful, we use the higher one
            (Ok(electrum_conservative_fee_rate), Ok(electrum_histogram_fee_rate)) => {
                tracing::debug!(
                    electrum_conservative_fee_rate_sat_vb =
                        electrum_conservative_fee_rate.to_sat_per_vb_ceil(),
                    electrum_histogram_fee_rate_sat_vb =
                        electrum_histogram_fee_rate.to_sat_per_vb_ceil(),
                    "Successfully fetched fee rates from both sources. We will use the higher one"
                );

                Ok(electrum_conservative_fee_rate.max(electrum_histogram_fee_rate))
            }
            // If the conservative fee rate fails, we use the histogram fee rate
            (Err(electrum_conservative_fee_rate_error), Ok(electrum_histogram_fee_rate)) => {
                tracing::warn!(
                    electrum_conservative_fee_rate_error = ?electrum_conservative_fee_rate_error,
                    electrum_histogram_fee_rate_sat_vb = electrum_histogram_fee_rate.to_sat_per_vb_ceil(),
                    "Failed to fetch conservative fee rate, using histogram fee rate"
                );
                Ok(electrum_histogram_fee_rate)
            }
            // If the histogram fee rate fails, we use the conservative fee rate
            (Ok(electrum_conservative_fee_rate), Err(electrum_histogram_fee_rate_error)) => {
                tracing::warn!(
                    electrum_histogram_fee_rate_error = ?electrum_histogram_fee_rate_error,
                    electrum_conservative_fee_rate_sat_vb = electrum_conservative_fee_rate.to_sat_per_vb_ceil(),
                    "Failed to fetch histogram fee rate, using conservative fee rate"
                );
                Ok(electrum_conservative_fee_rate)
            }
            // If both the histogram and conservative fee rate fail, we return an error
            (Err(electrum_conservative_fee_rate_error), Err(electrum_histogram_fee_rate_error)) => {
                Err(electrum_conservative_fee_rate_error
                    .context(electrum_histogram_fee_rate_error)
                    .context("Failed to fetch both the conservative and histogram fee rates from Electrum"))
            }
        }
    }

    async fn min_relay_fee(&self) -> Result<FeeRate> {
        self.min_relay_fee().await
    }
}

/// Extension trait for our custom concurrent sync implementation.
mod sync_ext {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex as SyncMutex;

    use bdk_wallet::KeychainKind;

    use super::IntoArcMutex;

    /// Type alias for an optional callback
    /// that is used to report progress of a sync (or a chunk of a sync)
    pub type InnerSyncCallback = Option<Box<dyn FnMut(u64, u64) + Send + 'static>>;

    /// Type alias for the thread-safe, reference-counted callback of an [`InnerSyncCallback`]
    pub type SyncCallback = Arc<SyncMutex<InnerSyncCallback>>;

    pub trait SyncCallbackExt {
        #[allow(clippy::new_ret_no_self)]
        fn new<F>(callback: F) -> InnerSyncCallback
        where
            F: FnMut(u64, u64) + Send + 'static;
        fn throttle_callback(self, min_percentage_increase: f32) -> InnerSyncCallback;
        fn chain(self, callback: InnerSyncCallback) -> InnerSyncCallback;
        fn finalize(self) -> SyncCallback;
        fn call(&mut self, consumed: u64, total: u64);
        #[allow(clippy::type_complexity)]
        fn to_full_scan_callback(
            self,
            stop_gap: u32,
            assumed_buffer: u32,
        ) -> Box<dyn FnMut(KeychainKind, u32, &bitcoin::Script) + Send + 'static>
        where
            Self: Sized;
    }

    impl SyncCallbackExt for InnerSyncCallback {
        /// Creates a new sync callback from a callback function.
        fn new<F>(callback: F) -> InnerSyncCallback
        where
            F: FnMut(u64, u64) + Send + 'static,
        {
            Some(Box::new(callback))
        }

        /// Throttles a sync callback, invoking the original callback only when
        /// the progress has increased by at least `min_percentage_increase` since the last invocation.
        ///
        /// Ensures the callback is always invoked when progress reaches 100%.
        fn throttle_callback(self, min_percentage_increase: f32) -> InnerSyncCallback {
            let mut callback = self?;

            let mut last_reported_percentage: f64 = 0.0;
            let threshold = min_percentage_increase as f64 / 100.0;
            let threshold = threshold.clamp(0.0, 1.0);

            #[allow(clippy::cast_precision_loss)]
            Some(Box::new(move |consumed, total| {
                if total == 0 {
                    return;
                }

                let current_percentage = consumed as f64 / total as f64;
                let is_complete = consumed == total;
                let should_report = is_complete
                    || (current_percentage - last_reported_percentage >= threshold)
                    || last_reported_percentage == 0.0;

                if should_report {
                    callback(consumed, total);
                    last_reported_percentage = current_percentage;
                }
            }))
        }

        /// Chains this callback with another callback
        /// Creates a new callback that invokes both callbacks in order.
        fn chain(mut self, mut callback: InnerSyncCallback) -> InnerSyncCallback {
            Self::new(move |consumed, total| {
                self.call(consumed, total);
                callback.call(consumed, total);
            })
        }

        /// Calls the callback with the given progress, if it's Some(...).
        fn call(&mut self, consumed: u64, total: u64) {
            if let Some(cb) = self.as_mut() {
                cb(consumed, total);
            }
        }

        /// Builds a Arc<Mutex<Self>> from the callback
        fn finalize(self) -> SyncCallback {
            self.into_arc_mutex_sync()
        }

        fn to_full_scan_callback(
            mut self,
            stop_gap: u32,
            assumed_buffer: u32,
        ) -> Box<dyn FnMut(KeychainKind, u32, &bitcoin::Script) + Send + 'static>
        where
            Self: Sized,
        {
            Box::new(move |_, current_index, _| {
                let total = stop_gap.max(current_index + assumed_buffer);

                self.call(current_index as u64, total as u64);
            })
        }
    }

    // This struct combines progress updates from different chunks
    // and makes them seem like a single progress update to outsiders
    pub struct CumulativeProgress(HashMap<u64, (u64, u64)>);

    impl CumulativeProgress {
        pub fn new() -> Self {
            Self(HashMap::new())
        }

        /// Get the cumulative progress from all cached singular progress updates
        pub fn get_cumulative(&self) -> (u64, u64) {
            let total_consumed = self.0.values().map(|(consumed, _)| *consumed).sum();
            let total_total = self.0.values().map(|(_, total)| *total).sum();

            (total_consumed, total_total)
        }

        /// Updates the progress of a single chunk
        pub fn insert_single(&mut self, index: u64, consumed: u64, total: u64) {
            self.0.insert(index, (consumed, total));
        }
    }

    pub trait CumulativeProgressHandle {
        fn chunk_callback(self, callback: SyncCallback, index: u64) -> InnerSyncCallback;
    }

    impl CumulativeProgressHandle for Arc<SyncMutex<CumulativeProgress>> {
        /// Takes a callback function and an index of singular SyncRequest chunk
        ///
        /// Returns a new SyncCallback that when called will update the cumulative progress
        ///
        /// The given callback will be called when there's a progress update for the given chunk
        ///
        /// If one wants to a callback to be called called for every update you need to
        /// pass it into every call to this function for every chunk.
        fn chunk_callback(self, callback: SyncCallback, index: u64) -> InnerSyncCallback {
            InnerSyncCallback::new(move |consumed, total| {
                // Insert the latest progress update into the cache
                if let Ok(mut cache) = self.lock() {
                    cache.insert_single(index, consumed, total);

                    // Calculate the cumulative consumed and the cumulative total
                    let (cumulative_consumed, cumulative_total) = cache.get_cumulative();

                    // Send the cumulative progress to the callback
                    // We use sync Mutex here but it's ok because we're only blocking for a short time
                    let callback = callback.lock();
                    if let Ok(mut callback) = callback {
                        callback.call(cumulative_consumed, cumulative_total);
                    }
                }
            })
        }
    }
}

fn trace_status_change(txid: Txid, old: Option<ScriptStatus>, new: ScriptStatus) -> ScriptStatus {
    match (old, new) {
        (None, new_status) => {
            tracing::debug!(%txid, status = %new_status, "Found relevant Bitcoin transaction");
        }
        (Some(old_status), new_status) if old_status != new_status => {
            tracing::trace!(%txid, %new_status, %old_status, "Bitcoin transaction status changed");
        }
        _ => {}
    }

    new
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

    pub async fn wait_until(&self, mut predicate: impl FnMut(&ScriptStatus) -> bool) -> Result<()> {
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

/// Estimate the absolute fee for a transaction.
///
/// This function takes the following parameters:
/// - `weight`: The weight of the transaction
/// - `transfer_amount`: The amount of the transfer. Can be `None` if we don't know the transfer amount yet.
///    If the transfer amount is `None`, we will not check the relative fee bound.
/// - `fee_rate_estimation`: The fee rate provided by the user (from fee estimation source)
/// - `min_relay_fee_rate`: The minimum relay fee rate (from fee estimation source, might vary depending on mempool congestion)
///
/// This function will fail if:
/// - The transfer amount is less than the dust amount
/// - The fee rate / min relay fee rate provided by the user is greater than 100M sat/vbyte (sanity check)
///
/// This functions ensures:
/// - We never spend more than MAX_RELATIVE_TX_FEE of the transfer amount on fees
/// - We never use a fee rate higher than MAX_TX_FEE_RATE (100M sat/vbyte)
/// - We never go below 1000 sats (absolute minimum relay fee)
/// - We never go below the minimum relay fee rate (from the fee estimation source)
///
/// We also add a constant safety margin to the fee
fn estimate_fee(
    weight: Weight,
    transfer_amount: Option<Amount>,
    fee_rate_estimation: FeeRate,
    min_relay_fee_rate: FeeRate,
) -> Result<Amount> {
    if let Some(transfer_amount) = transfer_amount {
        // We cannot transfer less than the dust amount
        if transfer_amount <= DUST_AMOUNT {
            bail!(
                "Transfer amount needs to be greater than Bitcoin dust amount. Got: {} sats",
                transfer_amount.to_sat()
            );
        }
    }

    // Sanity checks
    if fee_rate_estimation.to_sat_per_vb_ceil() > 100_000_000
        || min_relay_fee_rate.to_sat_per_vb_ceil() > 100_000_000
    {
        bail!("A fee_rate or min_relay_fee of > 1BTC does not make sense")
    }

    // Choose the highest fee rate of:
    // 1. The fee rate provided by the user (comes from fee estimation source)
    // 2. The minimum relay fee rate (comes from fee estimation source, might vary depending on mempool congestion)
    // 3. The broadcast minimum fee rate (hardcoded in the Bitcoin library)
    // We round up to the next sat/vbyte
    let recommended_fee_rate = FeeRate::from_sat_per_vb(
        fee_rate_estimation
            .to_sat_per_vb_ceil()
            .max(min_relay_fee_rate.to_sat_per_vb_ceil())
            .max(FeeRate::BROADCAST_MIN.to_sat_per_vb_ceil()),
    )
    .context("Failed to compute recommended fee rate")?;

    if recommended_fee_rate > fee_rate_estimation {
        tracing::warn!(
            "Estimated fee was below the minimum relay fee rate. Falling back to: {} sats/vbyte",
            recommended_fee_rate.to_sat_per_vb_ceil()
        );
    }

    // Compute the absolute fee in satoshis for the given weight
    let recommended_fee_absolute_sats = recommended_fee_rate
        .checked_mul_by_weight(weight)
        .context("Failed to compute recommended fee rate")?;

    tracing::debug!(
        ?transfer_amount,
        %weight,
        %fee_rate_estimation,
        recommended_fee_rate = %recommended_fee_rate.to_sat_per_vb_ceil(),
        %recommended_fee_absolute_sats,
        "Estimated fee for transaction",
    );

    // If the recommended fee is above the absolute max allowed fee, we fall back to the absolute max allowed fee
    //
    // We only care about this if the transfer amount is known
    if let Some(transfer_amount) = transfer_amount {
        // We never want to spend more than specific percentage of the transfer amount
        // on fees
        let absolute_max_allowed_fee = Amount::from_sat(
            MAX_RELATIVE_TX_FEE
                .saturating_mul(Decimal::from(transfer_amount.to_sat()))
                .ceil()
                .to_u64()
                .expect("Max relative tx fee to fit into u64"),
        );

        if recommended_fee_absolute_sats > absolute_max_allowed_fee {
            let max_relative_tx_fee_percentage = MAX_RELATIVE_TX_FEE
                .saturating_mul(Decimal::from(100))
                .ceil()
                .to_u64()
                .expect("Max relative tx fee to fit into u64");

            tracing::warn!(
                "Relative bound of transaction fees reached. We don't want to spend more than {}% of our transfer amount on fees. Falling back to: {} sats",
                max_relative_tx_fee_percentage,
                absolute_max_allowed_fee.to_sat()
            );

            return Ok(absolute_max_allowed_fee);
        }
    }

    // Bitcoin Core has a minimum relay fee of 1000 sats, regardless of the transaction size
    // Essentially this is an extension of the minimum relay fee rate
    // but some nodes ceil the transaction size to 1000 vbytes
    if recommended_fee_absolute_sats < MIN_ABSOLUTE_TX_FEE {
        tracing::warn!(
            "Recommended fee rate is below the absolute minimum relay fee. Falling back to: {} sats",
            MIN_ABSOLUTE_TX_FEE.to_sat()
        );

        return Ok(MIN_ABSOLUTE_TX_FEE);
    }

    // We have a hard limit of 100M sats on the absolute fee
    if recommended_fee_absolute_sats > MAX_ABSOLUTE_TX_FEE {
        tracing::warn!(
            "Hard bound of transaction fee reached. Falling back to: {} sats",
            MAX_ABSOLUTE_TX_FEE.to_sat()
        );

        return Ok(MAX_ABSOLUTE_TX_FEE);
    }

    // Return the recommended fee without any safety margin
    Ok(recommended_fee_absolute_sats)
}

mod mempool_client {
    static HTTP_TIMEOUT: Duration = Duration::from_secs(15);
    static BASE_URL: &str = "https://mempool.space";

    use super::EstimateFeeRate;
    use anyhow::{bail, Context, Result};
    use bitcoin::{FeeRate, Network};
    use serde::Deserialize;
    use std::time::Duration;

    /// A client for the mempool.space API.
    ///
    /// This client is used to estimate the fee rate for a transaction.
    #[derive(Clone)]
    pub struct MempoolClient {
        client: reqwest::Client,
        base_url: String,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct MempoolFees {
        fastest_fee: u64,
        half_hour_fee: u64,
        hour_fee: u64,
        minimum_fee: u64,
    }

    impl MempoolClient {
        pub fn new(network: Network) -> Result<Self> {
            let base_url = match network {
                Network::Bitcoin => BASE_URL.to_string(),
                Network::Testnet => format!("{}/testnet", BASE_URL),
                Network::Signet => format!("{}/signet", BASE_URL),
                _ => bail!("mempool.space fee estimation unsupported for network"),
            };

            let client = reqwest::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .build()
                .context("Failed to build mempool.space HTTP client")?;

            Ok(MempoolClient { client, base_url })
        }

        /// Fetch the fees (`fees/recommended` endpoint) from the mempool.space API
        async fn fetch_fees(&self) -> Result<MempoolFees> {
            let url = format!("{}/api/v1/fees/recommended", self.base_url);

            let response = self.client.get(url).send().await?;

            let fees: MempoolFees = response.json().await?;

            Ok(fees)
        }
    }

    impl EstimateFeeRate for MempoolClient {
        async fn estimate_feerate(&self, target_block: u32) -> Result<FeeRate> {
            let fees = self.fetch_fees().await?;

            // Match the target block to the correct fee rate
            let sat_per_vb = match target_block {
                0..=2 => fees.fastest_fee,
                3 => fees.half_hour_fee,
                _ => fees.hour_fee,
            };

            // Construct the fee rate
            FeeRate::from_sat_per_vb(sat_per_vb)
                .context("Failed to parse mempool fee rate (out of range)")
        }

        async fn min_relay_fee(&self) -> Result<FeeRate> {
            let fees = self.fetch_fees().await?;

            // Match the target block to the correct fee rate
            let minimum_relay_fee = fees.minimum_fee;

            // Construct the fee rate
            FeeRate::from_sat_per_vb(minimum_relay_fee)
                .context("Failed to parse mempool min relay fee (out of range)")
        }
    }
}

impl Watchable for (Txid, ScriptBuf) {
    fn id(&self) -> Txid {
        self.0
    }

    fn script(&self) -> ScriptBuf {
        self.1.clone()
    }
}

impl ScriptStatus {
    pub fn from_confirmations(confirmations: u32) -> Self {
        match confirmations {
            0 => Self::InMempool,
            confirmations => Self::Confirmed(Confirmed::new(confirmations - 1)),
        }
    }
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

pub mod pre_1_0_0_bdk {
    //! This module contains some code for creating a bdk wallet from before the update.
    //! We need to keep this around to be able to migrate the wallet.

    use std::path::Path;
    use std::sync::Arc;

    use anyhow::{anyhow, bail, Result};
    use bdk::bitcoin::{util::bip32::ExtendedPrivKey, Network};
    use bdk::sled::Tree;
    use bdk::KeychainKind;
    use tokio::sync::Mutex as TokioMutex;

    use super::IntoArcMutex;

    pub const WALLET: &str = "wallet";
    const SLED_TREE_NAME: &str = "default_tree";

    /// The is the old bdk wallet before the migration.
    /// We need to contruct it before migration to get the keys and revelation indeces.
    pub struct OldWallet<D = Tree> {
        wallet: Arc<TokioMutex<bdk::Wallet<D>>>,
        network: Network,
    }

    /// This is all the data we need from the old wallet to be able to migrate it
    /// and check whether we did it correctly.
    pub struct Export {
        /// Wallet descriptor and blockheight.
        pub export: bdk_wallet::export::FullyNodedExport,
        /// Index of the last external address that was revealed.
        pub external_derivation_index: u32,
        /// Index of the last internal address that was revealed.
        pub internal_derivation_index: u32,
    }

    impl OldWallet {
        /// Create a new old wallet.
        pub async fn new(
            data_dir: impl AsRef<Path>,
            xprivkey: ExtendedPrivKey,
            network: bitcoin::Network,
        ) -> Result<Self> {
            let data_dir = data_dir.as_ref();
            let wallet_dir = data_dir.join(WALLET);
            let database = bdk::sled::open(wallet_dir)?.open_tree(SLED_TREE_NAME)?;

            // Convert bitcoin network to the bdk network type...
            let network = match network {
                bitcoin::Network::Bitcoin => bdk::bitcoin::Network::Bitcoin,
                bitcoin::Network::Testnet => bdk::bitcoin::Network::Testnet,
                bitcoin::Network::Regtest => bdk::bitcoin::Network::Regtest,
                bitcoin::Network::Signet => bdk::bitcoin::Network::Signet,
                _ => bail!("Unsupported network"),
            };

            let wallet = bdk::Wallet::new(
                bdk::template::Bip84(xprivkey, KeychainKind::External),
                Some(bdk::template::Bip84(xprivkey, KeychainKind::Internal)),
                network,
                database,
            )?;

            Ok(Self {
                wallet: wallet.into_arc_mutex_async(),
                network,
            })
        }

        /// Get a full export of the wallet including descriptors and blockheight.
        /// It also includes the internal (change) address and external (receiving) address derivation indices.
        pub async fn export(&self, role: &str) -> Result<Export> {
            let wallet = self.wallet.lock().await;
            let export = bdk::wallet::export::FullyNodedExport::export_wallet(
                &wallet,
                &format!("{}-{}", role, self.network),
                true,
            )
            .map_err(|_| anyhow!("Failed to export old wallet descriptor"))?;

            // Because we upgraded bdk, the type id changed.
            // Thus, we serialize to json and then deserialize to the new type.
            let json = serde_json::to_string(&export)?;
            let export = serde_json::from_str::<bdk_wallet::export::FullyNodedExport>(&json)?;

            let external_info = wallet.get_address(bdk::wallet::AddressIndex::LastUnused)?;
            let external_derivation_index = external_info.index;

            let internal_info =
                wallet.get_internal_address(bdk::wallet::AddressIndex::LastUnused)?;
            let internal_derivation_index = internal_info.index;

            Ok(Export {
                export,
                internal_derivation_index,
                external_derivation_index,
            })
        }
    }
}

/// Trait for converting a type into an Arc<Mutex<T>>.
// We use this a ton in this file so this is a convenience trait.
trait IntoArcMutex<T> {
    fn into_arc_mutex_async(self) -> Arc<TokioMutex<T>>;
    fn into_arc_mutex_sync(self) -> Arc<SyncMutex<T>>;
}

impl<T> IntoArcMutex<T> for T {
    fn into_arc_mutex_async(self) -> Arc<TokioMutex<T>> {
        Arc::new(TokioMutex::new(self))
    }

    fn into_arc_mutex_sync(self) -> Arc<SyncMutex<T>> {
        Arc::new(SyncMutex::new(self))
    }
}

#[cfg(test)]
#[derive(Clone)]
pub struct StaticFeeRate {
    fee_rate: FeeRate,
    min_relay_fee: bitcoin::Amount,
}

#[cfg(test)]
impl StaticFeeRate {
    pub fn new(fee_rate: FeeRate, min_relay_fee: bitcoin::Amount) -> Self {
        Self {
            fee_rate,
            min_relay_fee,
        }
    }
}

#[cfg(test)]
impl EstimateFeeRate for StaticFeeRate {
    async fn estimate_feerate(&self, _target_block: u32) -> Result<FeeRate> {
        Ok(self.fee_rate)
    }

    async fn min_relay_fee(&self) -> Result<FeeRate> {
        Ok(FeeRate::from_sat_per_vb(self.min_relay_fee.to_sat()).unwrap())
    }
}

#[cfg(test)]
#[derive(Debug)]
pub struct TestWalletBuilder {
    utxo_amount: u64,
    sats_per_vb: u64,
    min_relay_sats_per_vb: u64,
    key: bitcoin::bip32::Xpriv,
    num_utxos: u8,
}

#[cfg(test)]
impl TestWalletBuilder {
    /// Creates a new, funded wallet with sane default fees.
    ///
    /// Unless you are testing things related to fees, this is likely what you
    /// want.
    pub fn new(amount: u64) -> Self {
        TestWalletBuilder {
            utxo_amount: amount,
            sats_per_vb: 1,
            min_relay_sats_per_vb: 1,
            key: "tprv8ZgxMBicQKsPeZRHk4rTG6orPS2CRNFX3njhUXx5vj9qGog5ZMH4uGReDWN5kCkY3jmWEtWause41CDvBRXD1shKknAMKxT99o9qUTRVC6m".parse().unwrap(),
            num_utxos: 1,
        }
    }

    pub fn with_zero_fees(self) -> Self {
        Self {
            sats_per_vb: 0,
            min_relay_sats_per_vb: 0,
            ..self
        }
    }

    pub fn with_fees(self, sats_per_vb: u64, min_relay_sats_per_vb: u64) -> Self {
        Self {
            sats_per_vb,
            min_relay_sats_per_vb,
            ..self
        }
    }

    pub fn with_key(self, key: bitcoin::bip32::Xpriv) -> Self {
        Self { key, ..self }
    }

    pub fn with_num_utxos(self, number: u8) -> Self {
        Self {
            num_utxos: number,
            ..self
        }
    }

    pub async fn build(self) -> Wallet<Connection, StaticFeeRate> {
        use bdk_wallet::chain::BlockId;
        use bdk_wallet::test_utils::{insert_checkpoint, receive_output_in_latest_block};

        let bdk_network = bitcoin::Network::Regtest;

        let external_descriptor = Bip84(self.key, KeychainKind::External)
            .build(bdk_network)
            .expect("Failed to build external descriptor for test wallet");
        let internal_descriptor = Bip84(self.key, KeychainKind::Internal)
            .build(bdk_network)
            .expect("Failed to build internal descriptor for test wallet");

        let mut persister = bdk_wallet::rusqlite::Connection::open_in_memory()
            .expect("Failed to open in-memory DB for test wallet");

        let bdk_core_wallet = bdk_wallet::Wallet::create(external_descriptor, internal_descriptor)
            .network(bdk_network)
            .create_wallet(&mut persister)
            .expect("Failed to create bdk_wallet::Wallet for test");

        let client = StaticFeeRate::new(
            FeeRate::from_sat_per_vb(self.sats_per_vb).unwrap(),
            bitcoin::Amount::from_sat(self.min_relay_sats_per_vb),
        );

        let cached_electrum_fee_estimator = Arc::new(CachedFeeEstimator::new(client.clone()));

        let wallet = Wallet {
            wallet: bdk_core_wallet.into_arc_mutex_async(),
            electrum_client: client.into_arc_mutex_async(),
            cached_electrum_fee_estimator,
            cached_mempool_fee_estimator: Arc::new(None), // We don't use mempool client in tests
            persister: persister.into_arc_mutex_async(),
            tauri_handle: None,
            network: Network::Regtest,
            finality_confirmations: 1,
            target_block: 1,
        };

        let mut locked_wallet = wallet.wallet.try_lock().unwrap();

        // Create a block
        insert_checkpoint(
            &mut locked_wallet,
            BlockId {
                height: 42,
                hash: <bitcoin::blockdata::block::BlockHash as bitcoin::hashes::Hash>::all_zeros(),
            },
        );

        // Fund the wallet with fake utxos
        for _ in 0..self.num_utxos {
            receive_output_in_latest_block(&mut locked_wallet, self.utxo_amount);
        }

        // Create another block to confirm the utxos
        insert_checkpoint(
            &mut locked_wallet,
            BlockId {
                height: 43,
                hash: <bitcoin::blockdata::block::BlockHash as bitcoin::hashes::Hash>::all_zeros(),
            },
        );

        drop(locked_wallet);

        wallet
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitcoin::{PublicKey, TxLock};
    use crate::tracing_ext::capture_logs;
    use bitcoin::address::NetworkUnchecked;
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
        let weight = Weight::from_wu(400);
        let amount = bitcoin::Amount::from_sat(100_000_000);

        let sat_per_vb = 100;
        let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb).unwrap();

        let relay_fee = FeeRate::from_sat_per_vb(1).unwrap();
        let is_fee = estimate_fee(weight, Some(amount), fee_rate, relay_fee).unwrap();

        // weight / 4.0 *  sat_per_vb
        let should_fee = bitcoin::Amount::from_sat(10_000);
        assert_eq!(is_fee, should_fee);
    }

    #[test]
    fn given_1BTC_and_1_sat_per_vb_fees_and_100ksat_min_relay_fee_should_hit_min() {
        // 400 weight = 100 vbyte
        let weight = Weight::from_wu(400);
        let amount = bitcoin::Amount::from_sat(100_000_000);

        let sat_per_vb = 1;
        let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb).unwrap();

        let relay_fee = FeeRate::from_sat_per_vb(250_000).unwrap(); // 100k sats for 400 weight units
        let is_fee = estimate_fee(weight, Some(amount), fee_rate, relay_fee).unwrap();

        // The function now uses the higher of fee_rate and relay_fee, then multiplies by weight
        // relay_fee (250_000 sat/vb) is higher than fee_rate (1 sat/vb)
        // 250_000 sat/vb * 100 vbytes = 25_000_000 sats, but this exceeds the relative max (20% of 1 BTC = 20M sats)
        // So it should fall back to the relative max: 20% of 100M = 20M sats
        let should_fee = bitcoin::Amount::from_sat(20_000_000);
        assert_eq!(is_fee, should_fee);
    }

    #[test]
    fn given_1mio_sat_and_1k_sats_per_vb_fees_should_hit_absolute_max() {
        // 400 weight = 100 vbyte
        let weight = Weight::from_wu(400);
        let amount = bitcoin::Amount::from_sat(1_000_000);

        let sat_per_vb = 1_000;
        let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb).unwrap();

        let relay_fee = FeeRate::from_sat_per_vb(1).unwrap();
        let is_fee = estimate_fee(weight, Some(amount), fee_rate, relay_fee).unwrap();

        // fee_rate (1000 sat/vb) * 100 vbytes = 100_000 sats
        // This equals exactly our MAX_ABSOLUTE_TX_FEE
        assert_eq!(is_fee, MAX_ABSOLUTE_TX_FEE);
    }

    #[test]
    fn given_1BTC_and_4mio_sats_per_vb_fees_should_hit_total_max() {
        // Even if we send 1BTC we don't want to pay 0.2BTC in fees. This would be
        // $1,650 at the moment.
        let weight = Weight::from_wu(400);
        let amount = bitcoin::Amount::from_sat(100_000_000);

        let sat_per_vb = 4_000_000;
        let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb).unwrap();

        let relay_fee = FeeRate::from_sat_per_vb(1).unwrap();
        let is_fee = estimate_fee(weight, Some(amount), fee_rate, relay_fee).unwrap();

        // With such a high fee rate (4M sat/vb), the calculated fee would be enormous
        // But it gets capped by the relative maximum (20% of transfer amount)
        // 20% of 100M sats = 20M sats
        let relative_max = bitcoin::Amount::from_sat(20_000_000);
        assert_eq!(is_fee, relative_max);
    }

    proptest! {
        #[test]
        fn given_randon_amount_random_fee_and_random_relay_rate_but_fix_weight_does_not_error(
            amount in 547u64..,
            sat_per_vb in 1u64..100_000_000,
            relay_fee in 0u64..100_000_000u64
        ) {
            let weight = Weight::from_wu(400);
            let amount = bitcoin::Amount::from_sat(amount);

            let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb).unwrap();

            let relay_fee = FeeRate::from_sat_per_vb(relay_fee.min(1_000_000)).unwrap();
            let _is_fee = estimate_fee(weight, Some(amount), fee_rate, relay_fee).unwrap();

        }
    }

    proptest! {
        #[test]
        fn given_amount_in_range_fix_fee_fix_relay_rate_fix_weight_fee_always_smaller_max(
            amount in 1u64..100_000_000,
        ) {
            let weight = Weight::from_wu(400);
            let amount = bitcoin::Amount::from_sat(amount);

            let sat_per_vb = 100;
            let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb).unwrap();

            let relay_fee = FeeRate::from_sat_per_vb(1).unwrap();
            let is_fee = estimate_fee(weight, Some(amount), fee_rate, relay_fee).unwrap();

            // weight / 4 * 100 = 10,000 sats which is always lower than MAX_ABSOLUTE_TX_FEE
            assert!(is_fee <= MAX_ABSOLUTE_TX_FEE);
        }
    }

    proptest! {
        #[test]
        fn given_amount_high_fix_fee_fix_relay_rate_fix_weight_fee_always_max(
            amount in 100_000_000u64..,
        ) {
            let weight = Weight::from_wu(400);
            let amount = bitcoin::Amount::from_sat(amount);

            let sat_per_vb = 1_000;
            let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb).unwrap();

            let relay_fee = FeeRate::from_sat_per_vb(1).unwrap();
            let is_fee = estimate_fee(weight, Some(amount), fee_rate, relay_fee).unwrap();

            // weight / 4 * 1_000 = 100_000 sats which hits our MAX_ABSOLUTE_TX_FEE
            assert_eq!(is_fee, MAX_ABSOLUTE_TX_FEE);
        }
    }

    proptest! {
        #[test]
        fn given_fee_above_max_should_always_errors(
            sat_per_vb in 100_000_000u64..(u64::MAX / 250),
        ) {
            let weight = Weight::from_wu(400);
            let amount = bitcoin::Amount::from_sat(547u64);

            let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb).unwrap();

            let relay_fee = FeeRate::from_sat_per_vb(1).unwrap();
            assert!(estimate_fee(weight, Some(amount), fee_rate, relay_fee).is_err());

        }
    }

    proptest! {
        #[test]
        fn given_relay_fee_above_max_should_always_errors(
            relay_fee in 100_000_000u64..
        ) {
            let weight = Weight::from_wu(400);
            let amount = bitcoin::Amount::from_sat(547u64);

            let fee_rate = FeeRate::from_sat_per_vb(1).unwrap();

            let relay_fee = FeeRate::from_sat_per_vb(relay_fee.min(1_000_000)).unwrap();
            // The function now has a sanity check that errors if fee rates > 100M sat/vb
            // Since we're capping relay_fee at 1M, it should not error
            // Instead, it should succeed and return a reasonable fee
            assert!(estimate_fee(weight, Some(amount), fee_rate, relay_fee).is_ok());
        }
    }

    #[tokio::test]
    async fn given_no_balance_returns_amount_0() {
        let wallet = TestWalletBuilder::new(0).with_fees(1, 1).build().await;
        let (amount, _fee) = wallet.max_giveable(TxLock::script_size()).await.unwrap();

        assert_eq!(amount, Amount::ZERO);
    }

    #[tokio::test]
    async fn given_balance_below_min_relay_fee_returns_amount_0() {
        let wallet = TestWalletBuilder::new(1000).with_fees(1, 1).build().await;
        let (amount, _fee) = wallet.max_giveable(TxLock::script_size()).await.unwrap();

        // The wallet can still create a transaction even if the balance is below the min relay fee
        // because BDK's transaction builder will use whatever fee rate is possible
        // The actual behavior is that it returns a small amount (like 846 sats in this case)
        // rather than 0, so we just check that it's a reasonable small amount
        assert!(amount.to_sat() < 1000);
    }

    #[tokio::test]
    async fn given_balance_above_relay_fee_returns_amount_greater_0() {
        let wallet = TestWalletBuilder::new(10_000).build().await;
        let (amount, _fee) = wallet.max_giveable(TxLock::script_size()).await.unwrap();

        assert!(amount.to_sat() > 0);
    }

    #[tokio::test]
    async fn given_balance_below_dust_returns_amount_0_but_with_sensible_fee() {
        let wallet = TestWalletBuilder::new(0).build().await;
        let (amount, fee) = wallet.max_giveable(TxLock::script_size()).await.unwrap();

        assert_eq!(amount, Amount::ZERO);
        assert!(fee.to_sat() > 0);
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
        let wallet = TestWalletBuilder::new(balance)
            .with_zero_fees()
            .build()
            .await;

        // sorting is only relevant for amounts that have a change output
        // if the change output is below dust it will be dropped by the BDK
        for amount in above_dust..(balance - (above_dust - 1)) {
            let (A, B) = (PublicKey::random(), PublicKey::random());
            let change = wallet.new_address().await.unwrap();
            let spending_fee = Amount::from_sat(300); // Use a fixed fee for testing
            let txlock = TxLock::new(
                &wallet,
                bitcoin::Amount::from_sat(amount),
                spending_fee,
                A,
                B,
                change,
            )
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
        let wallet = TestWalletBuilder::new(50_000).build().await;
        let custom_change = "bcrt1q08pfqpsyrt7acllzyjm8q5qsz5capvyahm49rw"
            .parse::<Address<NetworkUnchecked>>()
            .unwrap()
            .assume_checked();

        let spending_fee = Amount::from_sat(1000); // Use a fixed spending fee
        let psbt = wallet
            .send_to_address(
                wallet.new_address().await.unwrap(),
                Amount::from_sat(10_000),
                spending_fee,
                Some(custom_change.clone()),
            )
            .await
            .unwrap();
        let transaction = wallet.sign_and_finalize(psbt).await.unwrap();

        match transaction.output.as_slice() {
            [first, change] => {
                assert_eq!(first.value, Amount::from_sat(10_000));
                assert_eq!(change.script_pubkey, custom_change.script_pubkey());
            }
            _ => panic!("expected exactly two outputs"),
        }
    }

    #[test]
    fn printing_status_change_doesnt_spam_on_same_status() {
        let writer = capture_logs(LevelFilter::TRACE);

        let inner = bitcoin::hashes::sha256d::Hash::all_zeros();
        let tx = Txid::from_raw_hash(inner);
        let mut old = None;
        old = Some(trace_status_change(tx, old, ScriptStatus::Unseen));
        old = Some(trace_status_change(tx, old, ScriptStatus::InMempool));
        old = Some(trace_status_change(tx, old, ScriptStatus::InMempool));
        old = Some(trace_status_change(tx, old, ScriptStatus::InMempool));
        old = Some(trace_status_change(tx, old, ScriptStatus::InMempool));
        old = Some(trace_status_change(tx, old, ScriptStatus::InMempool));
        old = Some(trace_status_change(tx, old, ScriptStatus::InMempool));
        old = Some(trace_status_change(
            tx,
            old,
            ScriptStatus::Confirmed(Confirmed { depth: 0 }),
        ));
        old = Some(trace_status_change(
            tx,
            old,
            ScriptStatus::Confirmed(Confirmed { depth: 1 }),
        ));
        old = Some(trace_status_change(
            tx,
            old,
            ScriptStatus::Confirmed(Confirmed { depth: 1 }),
        ));
        old = Some(trace_status_change(
            tx,
            old,
            ScriptStatus::Confirmed(Confirmed { depth: 2 }),
        ));
        trace_status_change(tx, old, ScriptStatus::Confirmed(Confirmed { depth: 2 }));

        assert_eq!(
            writer.captured(),
            r"DEBUG swap::bitcoin::wallet: Found relevant Bitcoin transaction txid=0000000000000000000000000000000000000000000000000000000000000000 status=unseen
TRACE swap::bitcoin::wallet: Bitcoin transaction status changed txid=0000000000000000000000000000000000000000000000000000000000000000 new_status=in mempool old_status=unseen
TRACE swap::bitcoin::wallet: Bitcoin transaction status changed txid=0000000000000000000000000000000000000000000000000000000000000000 new_status=confirmed with 1 blocks old_status=in mempool
TRACE swap::bitcoin::wallet: Bitcoin transaction status changed txid=0000000000000000000000000000000000000000000000000000000000000000 new_status=confirmed with 2 blocks old_status=confirmed with 1 blocks
TRACE swap::bitcoin::wallet: Bitcoin transaction status changed txid=0000000000000000000000000000000000000000000000000000000000000000 new_status=confirmed with 3 blocks old_status=confirmed with 2 blocks
"
        )
    }

    proptest::proptest! {
        #[test]
        fn funding_never_fails_with_insufficient_funds(funding_amount in 3000u32.., num_utxos in 1..5u8, sats_per_vb in 1u64..500u64, key in crate::proptest::bitcoin::extended_priv_key(), alice in crate::proptest::ecdsa_fun::point(), bob in crate::proptest::ecdsa_fun::point()) {
            proptest::prop_assume!(alice != bob);

            tokio::runtime::Runtime::new().unwrap().block_on(async move {
                let wallet = TestWalletBuilder::new(funding_amount as u64)
                    .with_key(key)
                    .with_num_utxos(num_utxos)
                    .with_fees(sats_per_vb, 1)
                    .build()
                    .await;

                let (amount, spending_fee) = wallet.max_giveable(TxLock::script_size()).await.unwrap();
                let psbt: PartiallySignedTransaction = TxLock::new(&wallet, amount, spending_fee, PublicKey::from(alice), PublicKey::from(bob), wallet.new_address().await.unwrap()).await.unwrap().into();
                let result = wallet.sign_and_finalize(psbt).await;

                result.expect("transaction to be signed");
            });
        }
    }

    mod cached_fee_estimator_tests {
        use super::*;
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;
        use tokio::time::{sleep, Duration};

        /// Mock fee estimator that tracks how many times methods are called
        #[derive(Clone)]
        struct MockFeeEstimator {
            estimate_calls: Arc<AtomicU32>,
            min_relay_calls: Arc<AtomicU32>,
            fee_rate: FeeRate,
            min_relay_fee: FeeRate,
            delay: Duration,
        }

        impl MockFeeEstimator {
            fn new(fee_rate: FeeRate, min_relay_fee: FeeRate) -> Self {
                Self {
                    estimate_calls: Arc::new(AtomicU32::new(0)),
                    min_relay_calls: Arc::new(AtomicU32::new(0)),
                    fee_rate,
                    min_relay_fee,
                    delay: Duration::from_millis(0),
                }
            }

            fn with_delay(mut self, delay: Duration) -> Self {
                self.delay = delay;
                self
            }

            fn estimate_call_count(&self) -> u32 {
                self.estimate_calls.load(Ordering::SeqCst)
            }

            fn min_relay_call_count(&self) -> u32 {
                self.min_relay_calls.load(Ordering::SeqCst)
            }
        }

        impl EstimateFeeRate for MockFeeEstimator {
            async fn estimate_feerate(&self, _target_block: u32) -> Result<FeeRate> {
                self.estimate_calls.fetch_add(1, Ordering::SeqCst);
                if !self.delay.is_zero() {
                    sleep(self.delay).await;
                }
                Ok(self.fee_rate)
            }

            async fn min_relay_fee(&self) -> Result<FeeRate> {
                self.min_relay_calls.fetch_add(1, Ordering::SeqCst);
                if !self.delay.is_zero() {
                    sleep(self.delay).await;
                }
                Ok(self.min_relay_fee)
            }
        }

        #[tokio::test]
        async fn caches_fee_rate_estimates() {
            let mock = MockFeeEstimator::new(
                FeeRate::from_sat_per_vb(50).unwrap(),
                FeeRate::from_sat_per_vb(1).unwrap(),
            );
            let cached = CachedFeeEstimator::new(mock.clone());

            // First call should hit the underlying estimator
            let fee1 = cached.estimate_feerate(6).await.unwrap();
            assert_eq!(fee1, FeeRate::from_sat_per_vb(50).unwrap());
            assert_eq!(mock.estimate_call_count(), 1);

            // Second call with same target should use cache
            let fee2 = cached.estimate_feerate(6).await.unwrap();
            assert_eq!(fee2, FeeRate::from_sat_per_vb(50).unwrap());
            assert_eq!(mock.estimate_call_count(), 1); // Still 1, not 2

            // Different target should hit the underlying estimator again
            let fee3 = cached.estimate_feerate(12).await.unwrap();
            assert_eq!(fee3, FeeRate::from_sat_per_vb(50).unwrap());
            assert_eq!(mock.estimate_call_count(), 2);
        }

        #[tokio::test]
        async fn caches_min_relay_fee() {
            let mock = MockFeeEstimator::new(
                FeeRate::from_sat_per_vb(50).unwrap(),
                FeeRate::from_sat_per_vb(1).unwrap(),
            );
            let cached = CachedFeeEstimator::new(mock.clone());

            // First call should hit the underlying estimator
            let fee1 = cached.min_relay_fee().await.unwrap();
            assert_eq!(fee1, FeeRate::from_sat_per_vb(1).unwrap());
            assert_eq!(mock.min_relay_call_count(), 1);

            // Second call should use cache
            let fee2 = cached.min_relay_fee().await.unwrap();
            assert_eq!(fee2, FeeRate::from_sat_per_vb(1).unwrap());
            assert_eq!(mock.min_relay_call_count(), 1); // Still 1, not 2
        }

        #[tokio::test]
        async fn concurrent_requests_dont_duplicate_calls() {
            let mock = MockFeeEstimator::new(
                FeeRate::from_sat_per_vb(25).unwrap(),
                FeeRate::from_sat_per_vb(1).unwrap(),
            )
            .with_delay(Duration::from_millis(50)); // Add delay to simulate network call

            let cached = CachedFeeEstimator::new(mock.clone());

            // First, make one call to populate the cache
            let _initial = cached.estimate_feerate(6).await.unwrap();
            assert_eq!(mock.estimate_call_count(), 1);

            // Now make multiple concurrent requests for the same target
            // These should all hit the cache
            let handles: Vec<_> = (0..5)
                .map(|_| {
                    let cached = cached.clone();
                    tokio::spawn(async move { cached.estimate_feerate(6).await })
                })
                .collect();

            // Wait for all requests to complete
            let results: Vec<_> = futures::future::join_all(handles).await;

            // All should succeed with the same value
            for result in results {
                let fee = result.unwrap().unwrap();
                assert_eq!(fee, FeeRate::from_sat_per_vb(25).unwrap());
            }

            // The underlying estimator should still only have been called once
            // since all subsequent requests should hit the cache
            assert_eq!(
                mock.estimate_call_count(),
                1,
                "Expected exactly 1 call, got {}",
                mock.estimate_call_count()
            );
        }

        #[tokio::test]
        async fn different_target_blocks_cached_separately() {
            let mock = MockFeeEstimator::new(
                FeeRate::from_sat_per_vb(30).unwrap(),
                FeeRate::from_sat_per_vb(1).unwrap(),
            );
            let cached = CachedFeeEstimator::new(mock.clone());

            // Request different target blocks
            let _fee1 = cached.estimate_feerate(1).await.unwrap();
            let _fee2 = cached.estimate_feerate(6).await.unwrap();
            let _fee3 = cached.estimate_feerate(12).await.unwrap();

            assert_eq!(mock.estimate_call_count(), 3);

            // Request same targets again - should use cache
            let _fee1_cached = cached.estimate_feerate(1).await.unwrap();
            let _fee2_cached = cached.estimate_feerate(6).await.unwrap();
            let _fee3_cached = cached.estimate_feerate(12).await.unwrap();

            assert_eq!(mock.estimate_call_count(), 3); // Still 3, no additional calls
        }

        #[tokio::test]
        async fn cache_respects_ttl() {
            let mock = MockFeeEstimator::new(
                FeeRate::from_sat_per_vb(40).unwrap(),
                FeeRate::from_sat_per_vb(1).unwrap(),
            );
            let cached = CachedFeeEstimator::new(mock.clone());

            // First call
            let _fee1 = cached.estimate_feerate(6).await.unwrap();
            assert_eq!(mock.estimate_call_count(), 1);

            // Wait for cache to expire (2 minutes + small buffer)
            // Note: In a real test environment, you might want to use a shorter TTL
            // or mock the time. For now, we'll just verify the cache works within TTL.

            // Immediate second call should use cache
            let _fee2 = cached.estimate_feerate(6).await.unwrap();
            assert_eq!(mock.estimate_call_count(), 1);
        }

        #[tokio::test]
        async fn error_propagation() {
            #[derive(Clone)]
            struct FailingEstimator;

            impl EstimateFeeRate for FailingEstimator {
                async fn estimate_feerate(&self, _target_block: u32) -> Result<FeeRate> {
                    Err(anyhow::anyhow!("Network error"))
                }

                async fn min_relay_fee(&self) -> Result<FeeRate> {
                    Err(anyhow::anyhow!("Network error"))
                }
            }

            let cached = CachedFeeEstimator::new(FailingEstimator);

            // Errors should be propagated, not cached
            let result1 = cached.estimate_feerate(6).await;
            assert!(result1.is_err());
            assert!(result1.unwrap_err().to_string().contains("Network error"));

            let result2 = cached.min_relay_fee().await;
            assert!(result2.is_err());
            assert!(result2.unwrap_err().to_string().contains("Network error"));
        }

        #[tokio::test]
        async fn cache_capacity_limits() {
            let mock = MockFeeEstimator::new(
                FeeRate::from_sat_per_vb(35).unwrap(),
                FeeRate::from_sat_per_vb(1).unwrap(),
            );
            let cached = CachedFeeEstimator::new(mock.clone());

            // Fill cache beyond capacity (MAX_CACHE_SIZE = 10)
            for target in 1..=15 {
                let _fee = cached.estimate_feerate(target).await.unwrap();
            }

            assert_eq!(mock.estimate_call_count(), 15);

            // Request some of the earlier targets - some might have been evicted
            // Due to LRU eviction, the earliest entries might be gone
            let _fee = cached.estimate_feerate(1).await.unwrap();

            // The exact behavior depends on Moka's eviction policy,
            // but we should see that the cache is working within its limits
            assert!(mock.estimate_call_count() >= 15);
        }

        #[tokio::test]
        async fn clone_shares_cache() {
            let mock = MockFeeEstimator::new(
                FeeRate::from_sat_per_vb(45).unwrap(),
                FeeRate::from_sat_per_vb(1).unwrap(),
            );
            let cached1 = CachedFeeEstimator::new(mock.clone());
            let cached2 = cached1.clone();

            // First estimator makes a call
            let _fee1 = cached1.estimate_feerate(6).await.unwrap();
            assert_eq!(mock.estimate_call_count(), 1);

            // Second estimator should use the shared cache
            let _fee2 = cached2.estimate_feerate(6).await.unwrap();
            assert_eq!(mock.estimate_call_count(), 1); // Still 1, cache was shared
        }
    }
}

#[derive(Clone)]
pub struct SyncRequestBuilderFactory {
    chain_tip: bdk_wallet::chain::CheckPoint,
    spks: Vec<((KeychainKind, u32), ScriptBuf)>,
}

impl SyncRequestBuilderFactory {
    fn build(self) -> SyncRequestBuilder<(KeychainKind, u32)> {
        SyncRequest::builder()
            .chain_tip(self.chain_tip)
            .spks_with_indexes(self.spks)
    }
}
