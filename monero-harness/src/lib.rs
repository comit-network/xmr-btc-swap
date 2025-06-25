#![warn(
    unused_extern_crates,
    missing_debug_implementations,
    missing_copy_implementations,
    rust_2018_idioms,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::fallible_impl_from,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::dbg_macro
)]
#![forbid(unsafe_code)]

//! # monero-harness
//!
//! A simple lib to start a monero container (incl. monerod and
//! monero-wallet-rpc). Provides initialisation methods to generate blocks,
//! create and fund accounts, and start a continuous mining task mining blocks
//! every BLOCK_TIME_SECS seconds.
//!
//! Also provides standalone JSON RPC clients for monerod and monero-wallet-rpc.
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use testcontainers::clients::Cli;
use testcontainers::{Container, RunnableImage};
use tokio::time;

use monero::{Address, Amount};
use monero_rpc::monerod::MonerodRpc as _;
use monero_rpc::monerod::{self, GenerateBlocks};
use monero_sys::{no_listener, Daemon, SyncProgress, TxReceipt, WalletHandle};

use crate::image::{MONEROD_DAEMON_CONTAINER_NAME, MONEROD_DEFAULT_NETWORK, RPC_PORT};

pub mod image;

/// How often we mine a block.
const BLOCK_TIME_SECS: u64 = 1;

#[derive(Debug)]

pub struct Monero {
    monerod: Monerod,
    wallets: Vec<MoneroWallet>,
}

impl<'c> Monero {
    /// Starts a new regtest monero container setup consisting out of 1 monerod
    /// node and n wallets. The docker container and network will be prefixed
    /// with a randomly generated `prefix`. One miner wallet is started
    /// automatically.
    /// monerod container name is: `prefix`_`monerod`
    /// network is: `prefix`_`monero`
    /// miner wallet container name is: `miner`
    pub async fn new(
        cli: &'c Cli,
        additional_wallets: Vec<&'static str>,
    ) -> Result<(
        Self,
        Container<'c, image::Monerod>,
        Vec<Container<'c, image::MoneroWalletRpc>>,
    )> {
        let prefix = format!("{}_", random_prefix());
        let monerod_name = format!("{}{}", prefix, MONEROD_DAEMON_CONTAINER_NAME);
        let network = format!("{}{}", prefix, MONEROD_DEFAULT_NETWORK);

        tracing::info!("Starting monerod: {}", monerod_name);
        let (monerod, monerod_container) = Monerod::new(cli, monerod_name, network)?;
        let containers: Vec<Container<'c, image::MoneroWalletRpc>> = vec![];
        let mut wallets = vec![];

        let daemon = {
            let monerod_port = monerod_container.get_host_port_ipv4(RPC_PORT);
            let monerod_url = format!("http://127.0.0.1:{}", monerod_port);
            Daemon {
                address: monerod_url,
                ssl: false,
            }
        };

        {
            let client = reqwest::Client::new();
            let response = client
                .get(format!("{}/get_info", &daemon.address))
                .send()
                .await?;
            tracing::debug!("Monerod response at /get_info: {:?}", response.status());

            let response = client
                .get(format!("{}/json_rpc", &daemon.address))
                .send()
                .await?;
            tracing::debug!(
                "Monerod response at /json_rpc (expected error: -32600): {:?}",
                response.text().await?
            );
        }

        let miner = "miner";
        tracing::info!("Creating miner wallet: {}", miner);
        let miner_wallet = MoneroWallet::new(miner, daemon.clone(), prefix.clone())
            .await
            .context("Failed to create miner wallet")?;

        tracing::info!("Created miner wallet: {}", miner_wallet.name());

        wallets.push(miner_wallet);
        for wallet in additional_wallets.iter() {
            tracing::info!("Starting wallet: {}", wallet);

            let wallet_instance = tokio::time::timeout(Duration::from_secs(300), async {
                loop {
                    match MoneroWallet::new(wallet, daemon.clone(), prefix.clone()).await {
                        Ok(w) => break w,
                        Err(e) => {
                            tracing::warn!(
                                "Wallet creation error: {} – retrying in 2 seconds...",
                                e
                            );
                            time::sleep(Duration::from_secs(2)).await;
                        }
                    }
                }
            })
            .await
            .context("All retry attempts for creating a wallet exhausted")?;

            wallets.push(wallet_instance);
        }

        Ok((Self { monerod, wallets }, monerod_container, containers))
    }

    pub fn monerod(&self) -> &Monerod {
        &self.monerod
    }

    pub fn wallet(&self, name: &str) -> Result<&MoneroWallet> {
        let wallet = self
            .wallets
            .iter()
            .find(|wallet| wallet.name.eq(&name))
            .ok_or_else(|| anyhow!("Could not find wallet container."))?;

        Ok(wallet)
    }

    pub async fn init_miner(&self) -> Result<()> {
        let miner_wallet = self.wallet("miner")?;
        let miner_address = miner_wallet.address().await?.to_string();

        tracing::info!("Miner address: {}", miner_address);

        // Generate the first 120 blocks in bulk
        let amount_of_blocks = 120;
        let monerod = &self.monerod;
        let res = monerod
            .generate_blocks(amount_of_blocks, miner_address.clone())
            .await
            .context("Failed to generate blocks")?;
        tracing::info!(
            "Generated {:?} blocks to {}",
            res.blocks.len(),
            miner_address
        );
        if res.blocks.len() < amount_of_blocks.try_into().unwrap() {
            tracing::error!(
                "Expected to generate {} blocks, but only generated {}",
                amount_of_blocks,
                res.blocks.len()
            );
            bail!("Failed to generate enough blocks");
        }

        // Make sure to refresh the wallet to see the new balance
        let block_height = monerod.client().get_block_count().await?.count as u64;

        tracing::info!(
            "Waiting for miner wallet to catch up to blockchain height {}",
            block_height
        );
        miner_wallet.refresh().await?;

        // Debug: Check wallet balance after initial block generation
        let balance = miner_wallet.balance().await?;
        tracing::info!(
            "Miner balance after initial block generation: {}",
            Amount::from_pico(balance)
        );

        if balance == 0 {
            tracing::error!("Miner balance is still 0 after initial block generation");
            bail!("Miner balance is still 0 after initial block generation");
        }

        Ok(())
    }

    pub async fn init_wallet(&self, name: &str, amount_in_outputs: Vec<u64>) -> Result<()> {
        let wallet = self.wallet(name)?;

        self.init_external_wallet(name, &wallet.wallet, amount_in_outputs)
            .await
    }

    pub async fn init_external_wallet(
        &self,
        name: &str,
        wallet: &WalletHandle,
        amount_in_outputs: Vec<u64>,
    ) -> Result<()> {
        let miner_wallet = self.wallet("miner")?;
        let miner_address = miner_wallet.address().await?.to_string();
        let monerod = &self.monerod;

        if amount_in_outputs.is_empty() || amount_in_outputs.iter().sum::<u64>() == 0 {
            tracing::info!(address=%wallet.main_address().await, "Initializing wallet `{}` with {}", name, Amount::ZERO);
            return Ok(());
        }

        let mut expected_total = 0;

        tracing::info!("Syncing miner wallet");
        miner_wallet.refresh().await?;

        for amount in amount_in_outputs {
            if amount > 0 {
                miner_wallet
                    .transfer(&wallet.main_address().await, amount)
                    .await
                    .context("Miner could not transfer funds to wallet")?;
                expected_total += amount;
                tracing::debug!(
                    "Funded wallet `{}` with {}",
                    name,
                    Amount::from_pico(amount)
                );
            }
        }

        tracing::info!(
            address=%wallet.main_address().await,
            "Funding wallet `{}` with {}. Generating 10 blocks to unlock.",
            name,
            Amount::from_pico(expected_total)
        );
        monerod.generate_blocks(10, miner_address.clone()).await?;
        tracing::info!("Generated 10 blocks to unlock. Waiting for wallet to catch up.");
        tokio::time::sleep(Duration::from_secs(2)).await;

        let cloned_name = name.to_owned();
        wallet
            .wait_until_synced(Some(move |sync_progress: SyncProgress| {
                tracing::debug!(
                    current = sync_progress.current_block,
                    target = sync_progress.target_block,
                    "Synching wallet {}",
                    &cloned_name
                );
            }))
            .await
            .context("Failed to sync Monero wallet up to new 10 blocks")?;

        tokio::time::sleep(Duration::from_secs(10)).await;

        wallet.wait_until_synced(no_listener()).await?;

        let total = wallet.total_balance().await.as_pico();

        assert_eq!(total, expected_total);

        tracing::info!(
            "Wallet `{}` has received {} (unlocked)",
            &name,
            Amount::from_pico(total)
        );

        Ok(())
    }

    pub async fn generate_block(&self) -> Result<()> {
        let miner_wallet = self.wallet("miner")?;
        let miner_address = miner_wallet.address().await?.to_string();
        self.monerod()
            .generate_blocks(15, miner_address.clone())
            .await?;
        Ok(())
    }

    pub async fn start_miner(&self) -> Result<()> {
        let miner_wallet = self.wallet("miner")?;
        let miner_address = miner_wallet.address().await?.to_string();
        let monerod = &self.monerod;

        monerod.start_miner(&miner_address).await?;

        tracing::info!("Waiting for miner wallet to catch up...");
        miner_wallet.refresh().await?;

        Ok(())
    }

    pub async fn init_and_start_miner(&self) -> Result<()> {
        self.init_miner().await?;
        self.start_miner().await?;

        Ok(())
    }
}

fn random_prefix() -> String {
    use rand::Rng;

    rand::thread_rng()
        .sample_iter(rand::distributions::Alphanumeric)
        .take(4)
        .collect()
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Monerod {
    name: String,
    network: String,
    client: monerod::Client,
    rpc_port: u16,
}

pub struct MoneroWallet {
    name: String,
    wallet: WalletHandle,
}

impl std::fmt::Debug for MoneroWallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MoneroWallet {{ name: {} }}", self.name)
    }
}

// Old symbol kept as alias so dependant crates/tests can be migrated gradually.
pub type MoneroWalletRpc = MoneroWallet;

impl<'c> Monerod {
    /// Starts a new regtest monero container.
    fn new(
        cli: &'c Cli,
        name: String,
        network: String,
    ) -> Result<(Self, Container<'c, image::Monerod>)> {
        let image = image::Monerod;
        let image: RunnableImage<image::Monerod> = RunnableImage::from(image)
            .with_container_name(name.clone())
            .with_network(network.clone());

        let container = cli.run(image);
        let monerod_rpc_port = container.get_host_port_ipv4(RPC_PORT);

        Ok((
            Self {
                name,
                network,
                client: monerod::Client::localhost(monerod_rpc_port)?,
                rpc_port: monerod_rpc_port,
            },
            container,
        ))
    }

    pub fn client(&self) -> &monerod::Client {
        &self.client
    }

    /// Spawns a task to mine blocks in a regular interval to the provided
    /// address
    pub async fn start_miner(&self, miner_wallet_address: &str) -> Result<()> {
        let monerod = self.client().clone();
        tokio::spawn(mine(monerod, miner_wallet_address.to_string()));
        Ok(())
    }

    /// Maybe this helps with wallet syncing?
    pub async fn generate_blocks(
        &self,
        amount: u64,
        address: impl Into<String>,
    ) -> Result<GenerateBlocks> {
        let address = address.into();

        for _ in 0..amount {
            self.client().generateblocks(1, address.clone()).await?;
            tracing::trace!("Generated block, sleeping for 250ms");
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        Ok(GenerateBlocks {
            blocks: vec!["".to_string(); amount.try_into().unwrap()],
            height: u32::try_from(amount).unwrap(),
        })
    }
}

impl MoneroWallet {
    /// Create a new wallet using monero-sys bindings connected to the provided monerod instance.
    async fn new(name: &str, daemon: Daemon, prefix: String) -> Result<Self> {
        // Wallet files will be stored in the system temporary directory with the prefix to avoid clashes
        let mut wallet_path = std::env::temp_dir();
        wallet_path.push(format!("{}{}", prefix, name));

        // Use Mainnet network type – regtest daemon accepts mainnet prefixes
        // and this avoids address-parsing errors when calling daemon RPCs.
        let wallet = WalletHandle::open_or_create(
            wallet_path.display().to_string(),
            daemon,
            monero::Network::Mainnet,
            true,
        )
        .await
        .context("Failed to create or open wallet")?;

        // Allow mismatched daemon version when running in regtest
        // Also trusts the daemon.
        // Also set's the
        wallet.unsafe_prepare_for_regtest().await;

        Ok(Self {
            name: name.to_string(),
            wallet,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn address(&self) -> Result<Address> {
        Ok(self.wallet.main_address().await)
    }

    pub async fn balance(&self) -> Result<u64> {
        // First make sure we're connected to the daemon
        let connected = self.wallet.connected().await;
        tracing::debug!("Wallet connected to daemon: {}", connected);

        // Force a refresh first
        self.refresh().await?;

        let total = self.wallet.total_balance().await.as_pico();
        tracing::debug!(
            "Wallet `{}` balance (total): {}",
            self.name,
            Amount::from_pico(total)
        );
        Ok(total)
    }

    pub async fn unlocked_balance(&self) -> Result<u64> {
        Ok(self.wallet.unlocked_balance().await.as_pico())
    }

    pub async fn refresh(&self) -> Result<()> {
        let name = self.name.clone();

        self.wallet
            .wait_until_synced(Some(move |sync_progress: SyncProgress| {
                tracing::debug!(
                    current = sync_progress.current_block,
                    target = sync_progress.target_block,
                    "Synching wallet {}",
                    &name
                );
            }))
            .await?;
        Ok(())
    }

    pub async fn transfer(&self, address: &Address, amount_pico: u64) -> Result<TxReceipt> {
        tracing::info!(
            "`{}` transferring {}",
            self.name,
            Amount::from_pico(amount_pico),
        );
        let amount = Amount::from_pico(amount_pico);
        self.wallet
            .transfer(address, amount)
            .await
            .context("Failed to perform transfer")
    }

    pub async fn sweep(&self, address: &Address) -> Result<TxReceipt> {
        tracing::info!("`{}` sweeping", self.name);

        self.wallet
            .sweep(address)
            .await
            .context("Failed to perform sweep")?
            .into_iter()
            .next()
            .context("No transaction receipts returned from sweep")
    }

    pub async fn sweep_multi(&self, addresses: &[Address], ratios: &[f64]) -> Result<TxReceipt> {
        tracing::info!("`{}` sweeping multi ({:?})", self.name, ratios);
        self.balance().await?;

        self.wallet
            .sweep_multi(addresses, ratios)
            .await
            .context("Failed to perform sweep")?
            .into_iter()
            .next()
            .context("No transaction receipts returned from sweep")
    }

    pub async fn blockchain_height(&self) -> Result<u64> {
        self.wallet.blockchain_height().await
    }
}

/// Mine a block ever BLOCK_TIME_SECS seconds.
async fn mine(monerod: monerod::Client, reward_address: String) -> Result<()> {
    loop {
        time::sleep(Duration::from_secs(BLOCK_TIME_SECS)).await;
        monerod.generateblocks(1, reward_address.clone()).await?;
    }
}
