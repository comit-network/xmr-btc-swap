pub mod request;
use crate::cli::command::{Bitcoin, Monero, Tor};
use crate::database::open_db;
use crate::env::{Config as EnvConfig, GetConfig, Mainnet, Testnet};
use crate::fs::system_data_dir;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::protocol::Database;
use crate::seed::Seed;
use crate::{bitcoin, cli, monero};
use anyhow::{bail, Context as AnyContext, Error, Result};
use futures::future::try_join_all;
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Once};
use tokio::sync::{broadcast, broadcast::Sender, Mutex, RwLock};
use tokio::task::JoinHandle;
use url::Url;

static START: Once = Once::new();

#[derive(Clone, PartialEq, Debug)]
pub struct Config {
    tor_socks5_port: u16,
    namespace: XmrBtcNamespace,
    server_address: Option<SocketAddr>,
    pub env_config: EnvConfig,
    seed: Option<Seed>,
    debug: bool,
    json: bool,
    data_dir: PathBuf,
    is_testnet: bool,
}

use uuid::Uuid;

#[derive(Default)]
pub struct PendingTaskList(Mutex<Vec<JoinHandle<()>>>);

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

// workaround for warning over monero_rpc_process which we must own but not read
#[allow(dead_code)]
pub struct Context {
    pub db: Arc<dyn Database + Send + Sync>,
    bitcoin_wallet: Option<Arc<bitcoin::Wallet>>,
    monero_wallet: Option<Arc<monero::Wallet>>,
    monero_rpc_process: Option<monero::WalletRpcProcess>,
    pub swap_lock: Arc<SwapLock>,
    pub config: Config,
    pub tasks: Arc<PendingTaskList>,
}

#[allow(clippy::too_many_arguments)]
impl Context {
    pub async fn build(
        bitcoin: Option<Bitcoin>,
        monero: Option<Monero>,
        tor: Option<Tor>,
        data: Option<PathBuf>,
        is_testnet: bool,
        debug: bool,
        json: bool,
        server_address: Option<SocketAddr>,
    ) -> Result<Context> {
        let data_dir = data::data_dir_from(data, is_testnet)?;
        let env_config = env_config_from(is_testnet);

        START.call_once(|| {
            let _ = cli::tracing::init(debug, json, data_dir.join("logs"));
        });
        
        let seed = Seed::from_file_or_generate(data_dir.as_path())
            .context("Failed to read seed in file")?;

        let bitcoin_wallet = {
            if let Some(bitcoin) = bitcoin {
                let (bitcoin_electrum_rpc_url, bitcoin_target_block) =
                    bitcoin.apply_defaults(is_testnet)?;
                Some(Arc::new(
                    init_bitcoin_wallet(
                        bitcoin_electrum_rpc_url,
                        &seed,
                        data_dir.clone(),
                        env_config,
                        bitcoin_target_block,
                    )
                    .await?,
                ))
            } else {
                None
            }
        };

        let (monero_wallet, monero_rpc_process) = {
            if let Some(monero) = monero {
                let monero_daemon_address = monero.apply_defaults(is_testnet);
                let (wlt, prc) =
                    init_monero_wallet(data_dir.clone(), monero_daemon_address, env_config).await?;
                (Some(Arc::new(wlt)), Some(prc))
            } else {
                (None, None)
            }
        };

        let tor_socks5_port = tor.map_or(9050, |tor| tor.tor_socks5_port);

        let context = Context {
            db: open_db(data_dir.join("sqlite")).await?,
            bitcoin_wallet,
            monero_wallet,
            monero_rpc_process,
            config: Config {
                tor_socks5_port,
                namespace: XmrBtcNamespace::from_is_testnet(is_testnet),
                env_config,
                seed: Some(seed),
                server_address,
                debug,
                json,
                is_testnet,
                data_dir,
            },
            swap_lock: Arc::new(SwapLock::new()),
            tasks: Arc::new(PendingTaskList::default()),
        };

        Ok(context)
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
            bitcoin_wallet: Some(bob_bitcoin_wallet),
            monero_wallet: Some(bob_monero_wallet),
            config,
            db: open_db(db_path)
                .await
                .expect("Could not open sqlite database"),
            monero_rpc_process: None,
            swap_lock: Arc::new(SwapLock::new()),
            tasks: Arc::new(PendingTaskList::default()),
        }
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
    bitcoin_target_block: usize,
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
    monero_daemon_address: String,
    env_config: EnvConfig,
) -> Result<(monero::Wallet, monero::WalletRpcProcess)> {
    let network = env_config.monero_network;

    const MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME: &str = "swap-tool-blockchain-monitoring-wallet";

    let monero_wallet_rpc = monero::WalletRpc::new(data_dir.join("monero")).await?;

    let monero_wallet_rpc_process = monero_wallet_rpc
        .run(network, Some(monero_daemon_address))
        .await?;

    let monero_wallet = monero::Wallet::open_or_create(
        monero_wallet_rpc_process.endpoint(),
        MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME.to_string(),
        env_config,
    )
    .await?;

    Ok((monero_wallet, monero_wallet_rpc_process))
}

mod data {
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
            tor_socks5_port: 9050,
            namespace: XmrBtcNamespace::from_is_testnet(false),
            server_address: None,
            env_config,
            seed: Some(seed),
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
    use crate::api::request::{Method, Request};

    use libp2p::Multiaddr;
    use std::str::FromStr;
    use uuid::Uuid;

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
                tor_socks5_port: 9050,
                namespace: XmrBtcNamespace::from_is_testnet(is_testnet),
                server_address: None,
                env_config,
                seed: Some(seed),
                debug,
                json,
                is_testnet,
                data_dir,
            }
        }
    }

    impl Request {
        pub fn buy_xmr(is_testnet: bool) -> Request {
            let seller = Multiaddr::from_str(MULTI_ADDRESS).unwrap();
            let bitcoin_change_address = {
                if is_testnet {
                    bitcoin::Address::from_str(BITCOIN_TESTNET_ADDRESS).unwrap()
                } else {
                    bitcoin::Address::from_str(BITCOIN_MAINNET_ADDRESS).unwrap()
                }
            };

            let monero_receive_address = {
                if is_testnet {
                    monero::Address::from_str(MONERO_STAGENET_ADDRESS).unwrap()
                } else {
                    monero::Address::from_str(MONERO_MAINNET_ADDRESS).unwrap()
                }
            };

            Request::new(Method::BuyXmr {
                seller,
                bitcoin_change_address,
                monero_receive_address,
                swap_id: Uuid::new_v4(),
            })
        }

        pub fn resume() -> Request {
            Request::new(Method::Resume {
                swap_id: Uuid::from_str(SWAP_ID).unwrap(),
            })
        }

        pub fn cancel() -> Request {
            Request::new(Method::CancelAndRefund {
                swap_id: Uuid::from_str(SWAP_ID).unwrap(),
            })
        }

        pub fn refund() -> Request {
            Request::new(Method::CancelAndRefund {
                swap_id: Uuid::from_str(SWAP_ID).unwrap(),
            })
        }
    }
}
