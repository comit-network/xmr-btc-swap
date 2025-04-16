pub mod request;
pub mod tauri_bindings;

use crate::cli::command::{Bitcoin, Monero};
use crate::common::tor::init_tor_client;
use crate::common::tracing_util::Format;
use crate::database::{open_db, AccessMode};
use crate::env::{Config as EnvConfig, GetConfig, Mainnet, Testnet};
use crate::fs::system_data_dir;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::protocol::Database;
use crate::seed::Seed;
use crate::{bitcoin, common, monero};
use anyhow::anyhow;
use anyhow::{bail, Context as AnyContext, Error, Result};
use arti_client::TorClient;
use futures::future::try_join_all;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as SyncMutex, Once};
use tauri_bindings::{
    PendingCompleted, TauriContextStatusEvent, TauriEmitter, TauriHandle, TauriPartialInitProgress,
};
use tokio::sync::{broadcast, broadcast::Sender, Mutex as TokioMutex, RwLock};
use tokio::task::JoinHandle;
use tor_rtcompat::tokio::TokioRustlsRuntime;
use tracing::level_filters::LevelFilter;
use tracing::Level;
use url::Url;
use uuid::Uuid;

use super::watcher::Watcher;

static START: Once = Once::new();

#[derive(Clone, PartialEq, Debug)]
pub struct Config {
    namespace: XmrBtcNamespace,
    pub env_config: EnvConfig,
    seed: Option<Seed>,
    debug: bool,
    json: bool,
    data_dir: PathBuf,
    is_testnet: bool,
}

#[derive(Default)]
pub struct PendingTaskList(TokioMutex<Vec<JoinHandle<()>>>);

impl PendingTaskList {
    pub async fn spawn<F, T>(&self, future: F)
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let handle = tokio::spawn(async move {
            let _ = future.await;
        });

        self.0.lock().await.push(handle);
    }

    pub async fn wait_for_tasks(&self) -> Result<()> {
        let tasks = {
            // Scope for the lock, to avoid holding it for the entire duration of the async block
            let mut guard = self.0.lock().await;
            guard.drain(..).collect::<Vec<_>>()
        };

        try_join_all(tasks).await?;

        Ok(())
    }
}

/// The `SwapLock` manages the state of the current swap, ensuring that only one swap can be active at a time.
/// It includes:
/// - A lock for the current swap (`current_swap`)
/// - A broadcast channel for suspension signals (`suspension_trigger`)
///
/// The `SwapLock` provides methods to acquire and release the swap lock, and to listen for suspension signals.
/// This ensures that swap operations do not overlap and can be safely suspended if needed.
pub struct SwapLock {
    current_swap: RwLock<Option<Uuid>>,
    suspension_trigger: Sender<()>,
}

impl SwapLock {
    pub fn new() -> Self {
        let (suspension_trigger, _) = broadcast::channel(10);
        SwapLock {
            current_swap: RwLock::new(None),
            suspension_trigger,
        }
    }

    pub async fn listen_for_swap_force_suspension(&self) -> Result<(), Error> {
        let mut listener = self.suspension_trigger.subscribe();
        let event = listener.recv().await;
        match event {
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::error!("Error receiving swap suspension signal: {}", e);
                bail!(e)
            }
        }
    }

    pub async fn acquire_swap_lock(&self, swap_id: Uuid) -> Result<(), Error> {
        let mut current_swap = self.current_swap.write().await;
        if current_swap.is_some() {
            bail!("There already exists an active swap lock");
        }

        tracing::debug!(swap_id = %swap_id, "Acquiring swap lock");
        *current_swap = Some(swap_id);
        Ok(())
    }

    pub async fn get_current_swap_id(&self) -> Option<Uuid> {
        *self.current_swap.read().await
    }

    /// Sends a signal to suspend all ongoing swap processes.
    ///
    /// This function performs the following steps:
    /// 1. Triggers the suspension by sending a unit `()` signal to all listeners via `self.suspension_trigger`.
    /// 2. Polls the `current_swap` state every 50 milliseconds to check if it has been set to `None`, indicating that the swap processes have been suspended and the lock released.
    /// 3. If the lock is not released within 10 seconds, the function returns an error.
    ///
    /// If we send a suspend signal while no swap is in progress, the function will not fail, but will return immediately.
    ///
    /// # Returns
    /// - `Ok(())` if the swap lock is successfully released.
    /// - `Err(Error)` if the function times out waiting for the swap lock to be released.
    ///
    /// # Notes
    /// The 50ms polling interval is considered negligible overhead compared to the typical time required to suspend ongoing swap processes.
    pub async fn send_suspend_signal(&self) -> Result<(), Error> {
        const TIMEOUT: u64 = 10_000;
        const INTERVAL: u64 = 50;

        let _ = self.suspension_trigger.send(())?;

        for _ in 0..(TIMEOUT / INTERVAL) {
            if self.get_current_swap_id().await.is_none() {
                return Ok(());
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(INTERVAL)).await;
        }

        bail!("Timed out waiting for swap lock to be released");
    }

    pub async fn release_swap_lock(&self) -> Result<Uuid, Error> {
        let mut current_swap = self.current_swap.write().await;
        if let Some(swap_id) = current_swap.as_ref() {
            tracing::debug!(swap_id = %swap_id, "Releasing swap lock");

            let prev_swap_id = *swap_id;
            *current_swap = None;
            drop(current_swap);
            Ok(prev_swap_id)
        } else {
            bail!("There is no current swap lock to release");
        }
    }
}

impl Default for SwapLock {
    fn default() -> Self {
        Self::new()
    }
}

/// Holds shared data for different parts of the CLI.
///
/// Some components are optional, allowing initialization of only necessary parts.
/// For example, the `history` command doesn't require wallet initialization.
///
/// Many fields are wrapped in `Arc` for thread-safe sharing.
#[derive(Clone)]
pub struct Context {
    pub db: Arc<dyn Database + Send + Sync>,
    pub swap_lock: Arc<SwapLock>,
    pub config: Config,
    pub tasks: Arc<PendingTaskList>,
    tauri_handle: Option<TauriHandle>,
    bitcoin_wallet: Option<Arc<bitcoin::Wallet>>,
    monero_wallet: Option<Arc<monero::Wallet>>,
    monero_rpc_process: Option<Arc<SyncMutex<monero::WalletRpcProcess>>>,
    tor_client: Option<Arc<TorClient<TokioRustlsRuntime>>>,
}

/// A conveniant builder struct for [`Context`].
#[derive(Debug)]
#[must_use = "ContextBuilder must be built to be useful"]
pub struct ContextBuilder {
    monero: Option<Monero>,
    bitcoin: Option<Bitcoin>,
    data: Option<PathBuf>,
    is_testnet: bool,
    debug: bool,
    json: bool,
    tor: bool,
    tauri_handle: Option<TauriHandle>,
}

impl ContextBuilder {
    /// Start building a context
    pub fn new(is_testnet: bool) -> Self {
        if is_testnet {
            Self::testnet()
        } else {
            Self::mainnet()
        }
    }

    /// Basic builder with default options for mainnet
    pub fn mainnet() -> Self {
        ContextBuilder {
            monero: None,
            bitcoin: None,
            data: None,
            is_testnet: false,
            debug: false,
            json: false,
            tor: false,
            tauri_handle: None,
        }
    }

    /// Basic builder with default options for testnet
    pub fn testnet() -> Self {
        let mut builder = Self::mainnet();
        builder.is_testnet = true;
        builder
    }

    /// Configures the Context to initialize a Monero wallet with the given configuration.
    pub fn with_monero(mut self, monero: impl Into<Option<Monero>>) -> Self {
        self.monero = monero.into();
        self
    }

    /// Configures the Context to initialize a Bitcoin wallet with the given configuration.
    pub fn with_bitcoin(mut self, bitcoin: impl Into<Option<Bitcoin>>) -> Self {
        self.bitcoin = bitcoin.into();
        self
    }

    /// Attach a handle to Tauri to the Context for emitting events etc.
    pub fn with_tauri(mut self, tauri: impl Into<Option<TauriHandle>>) -> Self {
        self.tauri_handle = tauri.into();
        self
    }

    /// Configures where the data and logs are saved in the filesystem
    pub fn with_data_dir(mut self, data: impl Into<Option<PathBuf>>) -> Self {
        self.data = data.into();
        self
    }

    /// Whether to include debug level logging messages (default false)
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Set logging format to json (default false)
    pub fn with_json(mut self, json: bool) -> Self {
        self.json = json;
        self
    }

    /// Whether to initialize a Tor client (default false)
    pub fn with_tor(mut self, tor: bool) -> Self {
        self.tor = tor;
        self
    }

    /// Takes the builder, initializes the context by initializing the wallets and other components and returns the Context.
    pub async fn build(self) -> Result<Context> {
        // These are needed for everything else, and are blocking calls
        let data_dir = data::data_dir_from(self.data, self.is_testnet)?;
        let env_config = env_config_from(self.is_testnet);
        let seed = Seed::from_file_or_generate(data_dir.as_path())
            .context("Failed to read seed in file")?;

        // Initialize logging
        let format = if self.json { Format::Json } else { Format::Raw };
        let level_filter = if self.debug {
            LevelFilter::from_level(Level::DEBUG)
        } else {
            LevelFilter::from_level(Level::INFO)
        };

        START.call_once(|| {
            let _ = common::tracing_util::init(
                level_filter,
                format,
                data_dir.join("logs"),
                self.tauri_handle.clone(),
            );
        });

        // Create the data structure we use to manage the swap lock
        let swap_lock = Arc::new(SwapLock::new());
        let tasks = PendingTaskList::default().into();

        // Initialize the database
        self.tauri_handle
            .emit_context_init_progress_event(TauriContextStatusEvent::Initializing(vec![
                TauriPartialInitProgress::OpeningDatabase(PendingCompleted::Pending(())),
            ]));

        let db = open_db(
            data_dir.join("sqlite"),
            AccessMode::ReadWrite,
            self.tauri_handle.clone(),
        )
        .await?;

        self.tauri_handle
            .emit_context_init_progress_event(TauriContextStatusEvent::Initializing(vec![
                TauriPartialInitProgress::OpeningDatabase(PendingCompleted::Completed),
            ]));

        // Initialize these components concurrently
        let initialize_bitcoin_wallet = async {
            match self.bitcoin {
                Some(bitcoin) => {
                    let (url, target_block) = bitcoin.apply_defaults(self.is_testnet)?;

                    self.tauri_handle.emit_context_init_progress_event(
                        TauriContextStatusEvent::Initializing(vec![
                            TauriPartialInitProgress::OpeningBitcoinWallet(
                                PendingCompleted::Pending(()),
                            ),
                        ]),
                    );

                    let wallet =
                        init_bitcoin_wallet(url, &seed, data_dir.clone(), env_config, target_block)
                            .await?;

                    self.tauri_handle.emit_context_init_progress_event(
                        TauriContextStatusEvent::Initializing(vec![
                            TauriPartialInitProgress::OpeningBitcoinWallet(
                                PendingCompleted::Completed,
                            ),
                        ]),
                    );

                    Ok::<std::option::Option<Arc<bitcoin::wallet::Wallet>>, Error>(Some(Arc::new(
                        wallet,
                    )))
                }
                None => Ok(None),
            }
        };

        let initialize_monero_wallet = async {
            match self.monero {
                Some(monero) => {
                    self.tauri_handle.emit_context_init_progress_event(
                        TauriContextStatusEvent::Initializing(vec![
                            TauriPartialInitProgress::OpeningMoneroWallet(
                                PendingCompleted::Pending(()),
                            ),
                        ]),
                    );

                    let (wlt, prc) = init_monero_wallet(
                        data_dir.clone(),
                        monero.monero_daemon_address,
                        env_config,
                        self.tauri_handle.clone(),
                    )
                    .await?;

                    self.tauri_handle.emit_context_init_progress_event(
                        TauriContextStatusEvent::Initializing(vec![
                            TauriPartialInitProgress::OpeningMoneroWallet(
                                PendingCompleted::Completed,
                            ),
                        ]),
                    );

                    Ok((Some(Arc::new(wlt)), Some(Arc::new(SyncMutex::new(prc)))))
                }
                None => Ok((None, None)),
            }
        };

        let initialize_tor_client = async {
            self.tauri_handle.emit_context_init_progress_event(
                TauriContextStatusEvent::Initializing(vec![
                    TauriPartialInitProgress::EstablishingTorCircuits(
                        PendingCompleted::Pending(()),
                    ),
                ]),
            );

            let maybe_tor_client = init_tor_client(&data_dir)
                .await
                .inspect_err(|err| {
                    tracing::warn!(%err, "Failed to create Tor client. We will continue without Tor");
                })
                .ok();

            self.tauri_handle.emit_context_init_progress_event(
                TauriContextStatusEvent::Initializing(vec![
                    TauriPartialInitProgress::EstablishingTorCircuits(PendingCompleted::Completed),
                ]),
            );

            Ok(maybe_tor_client)
        };

        let (bitcoin_wallet, (monero_wallet, monero_rpc_process), tor) = tokio::try_join!(
            initialize_bitcoin_wallet,
            initialize_monero_wallet,
            initialize_tor_client,
        )?;

        // If we have a bitcoin wallet and a tauri handle, we start a background task
        if let Some(wallet) = bitcoin_wallet.clone() {
            if self.tauri_handle.is_some() {
                let watcher = Watcher::new(
                    wallet,
                    db.clone(),
                    self.tauri_handle.clone(),
                    swap_lock.clone(),
                );
                tokio::spawn(watcher.run());
            }
        }

        self.tauri_handle
            .emit_context_init_progress_event(TauriContextStatusEvent::Available);

        let context = Context {
            db,
            bitcoin_wallet,
            monero_wallet,
            monero_rpc_process,
            config: Config {
                namespace: XmrBtcNamespace::from_is_testnet(self.is_testnet),
                env_config,
                seed: seed.into(),
                debug: self.debug,
                json: self.json,
                is_testnet: self.is_testnet,
                data_dir,
            },
            swap_lock,
            tasks,
            tauri_handle: self.tauri_handle,
            tor_client: tor,
        };

        Ok(context)
    }
}

impl Context {
    pub fn with_tauri_handle(mut self, tauri_handle: impl Into<Option<TauriHandle>>) -> Self {
        self.tauri_handle = tauri_handle.into();

        self
    }

    pub async fn for_harness(
        seed: Seed,
        env_config: EnvConfig,
        db_path: PathBuf,
        bob_bitcoin_wallet: Arc<bitcoin::Wallet>,
        bob_monero_wallet: Arc<monero::Wallet>,
    ) -> Self {
        let config = Config::for_harness(seed, env_config);

        Self {
            bitcoin_wallet: bob_bitcoin_wallet.into(),
            monero_wallet: bob_monero_wallet.into(),
            config,
            db: open_db(db_path, AccessMode::ReadWrite, None)
                .await
                .expect("Could not open sqlite database"),
            monero_rpc_process: None,
            swap_lock: SwapLock::new().into(),
            tasks: PendingTaskList::default().into(),
            tauri_handle: None,
            tor_client: None,
        }
    }

    pub fn cleanup(&self) -> Result<()> {
        if let Some(ref monero_rpc_process) = self.monero_rpc_process {
            let mut process = monero_rpc_process
                .lock()
                .map_err(|_| anyhow!("Failed to lock monero_rpc_process for cleanup"))?;

            process.kill()?;
            println!("Killed monero-wallet-rpc process");
        }

        Ok(())
    }

    pub fn bitcoin_wallet(&self) -> Option<Arc<bitcoin::Wallet>> {
        self.bitcoin_wallet.clone()
    }

    pub fn tauri_handle(&self) -> Option<TauriHandle> {
        self.tauri_handle.clone()
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "")
    }
}

async fn init_bitcoin_wallet(
    electrum_rpc_url: Url,
    seed: &Seed,
    data_dir: PathBuf,
    env_config: EnvConfig,
    bitcoin_target_block: u16,
) -> Result<bitcoin::Wallet> {
    let wallet_dir = data_dir.join("wallet");

    let wallet = bitcoin::Wallet::new(
        electrum_rpc_url.clone(),
        &wallet_dir,
        seed.derive_extended_private_key(env_config.bitcoin_network)?,
        env_config,
        bitcoin_target_block,
    )
    .await
    .context("Failed to initialize Bitcoin wallet")?;

    wallet.sync().await?;

    Ok(wallet)
}

async fn init_monero_wallet(
    data_dir: PathBuf,
    monero_daemon_address: impl Into<Option<String>> + Clone,
    env_config: EnvConfig,
    tauri_handle: Option<TauriHandle>,
) -> Result<(monero::Wallet, monero::WalletRpcProcess)> {
    let network = env_config.monero_network;

    const MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME: &str = "swap-tool-blockchain-monitoring-wallet";

    let monero_wallet_rpc = monero::WalletRpc::new(data_dir.join("monero"), tauri_handle).await?;

    tracing::debug!(
        override_monero_daemon_address = monero_daemon_address.clone().into(),
        "Attempting to start monero-wallet-rpc process"
    );

    let monero_wallet_rpc_process = monero_wallet_rpc
        .run(network, monero_daemon_address.into())
        .await
        .context("Failed to start monero-wallet-rpc process")?;

    let monero_wallet = monero::Wallet::open_or_create(
        monero_wallet_rpc_process.endpoint(),
        MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME.to_string(),
        env_config,
    )
    .await
    .context("Failed to open or create Monero wallet")?;

    Ok((monero_wallet, monero_wallet_rpc_process))
}

pub mod data {
    use super::*;

    pub fn data_dir_from(arg_dir: Option<PathBuf>, testnet: bool) -> Result<PathBuf> {
        let base_dir = match arg_dir {
            Some(custom_base_dir) => custom_base_dir,
            None => os_default()?,
        };

        let sub_directory = if testnet { "testnet" } else { "mainnet" };

        Ok(base_dir.join(sub_directory))
    }

    fn os_default() -> Result<PathBuf> {
        Ok(system_data_dir()?.join("cli"))
    }
}

fn env_config_from(testnet: bool) -> EnvConfig {
    if testnet {
        Testnet::get_config()
    } else {
        Mainnet::get_config()
    }
}

impl Config {
    pub fn for_harness(seed: Seed, env_config: EnvConfig) -> Self {
        let data_dir = data::data_dir_from(None, false).expect("Could not find data directory");

        Self {
            namespace: XmrBtcNamespace::from_is_testnet(false),
            env_config,
            seed: seed.into(),
            debug: false,
            json: false,
            is_testnet: false,
            data_dir,
        }
    }
}

#[cfg(test)]
pub mod api_test {
    use super::*;

    pub const MULTI_ADDRESS: &str =
        "/ip4/127.0.0.1/tcp/9939/p2p/12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi";
    pub const MONERO_STAGENET_ADDRESS: &str = "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a";
    pub const BITCOIN_TESTNET_ADDRESS: &str = "tb1qr3em6k3gfnyl8r7q0v7t4tlnyxzgxma3lressv";
    pub const MONERO_MAINNET_ADDRESS: &str = "44Ato7HveWidJYUAVw5QffEcEtSH1DwzSP3FPPkHxNAS4LX9CqgucphTisH978FLHE34YNEx7FcbBfQLQUU8m3NUC4VqsRa";
    pub const BITCOIN_MAINNET_ADDRESS: &str = "bc1qe4epnfklcaa0mun26yz5g8k24em5u9f92hy325";
    pub const SWAP_ID: &str = "ea030832-3be9-454f-bb98-5ea9a788406b";

    impl Config {
        pub fn default(
            is_testnet: bool,
            data_dir: Option<PathBuf>,
            debug: bool,
            json: bool,
        ) -> Self {
            let data_dir = data::data_dir_from(data_dir, is_testnet).unwrap();
            let seed = Seed::from_file_or_generate(data_dir.as_path()).unwrap();
            let env_config = env_config_from(is_testnet);

            Self {
                namespace: XmrBtcNamespace::from_is_testnet(is_testnet),
                env_config,
                seed: seed.into(),
                debug,
                json,
                is_testnet,
                data_dir,
            }
        }
    }
}
