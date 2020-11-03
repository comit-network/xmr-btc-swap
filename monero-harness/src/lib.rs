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

pub mod image;
pub mod rpc;

use anyhow::{anyhow, bail, Result};
use std::time::Duration;
use testcontainers::{clients::Cli, core::Port, Container, Docker, RunArgs};
use tokio::time;

use crate::{
    image::{
        MONEROD_DAEMON_CONTAINER_NAME, MONEROD_DEFAULT_NETWORK, MONEROD_RPC_PORT, WALLET_RPC_PORT,
    },
    rpc::{
        monerod,
        wallet::{self, GetAddress, Refreshed, Transfer},
    },
};

/// How often we mine a block.
const BLOCK_TIME_SECS: u64 = 1;

/// Poll interval when checking if the wallet has synced with monerod.
const WAIT_WALLET_SYNC_MILLIS: u64 = 1000;

#[derive(Clone, Debug)]
pub struct Monero {
    monerod: Monerod,
    wallets: Vec<MoneroWalletRpc>,
    container_prefix: String,
}
impl<'c> Monero {
    /// Starts a new regtest monero container setup consisting out of 1 monerod
    /// node and n wallets. The containers will be prefixed with
    /// `container_prefix` if provided. There will be 1 miner wallet started
    /// automatically. Default monerod container name will be: `monerod`
    /// Default miner wallet container name will be: `miner`
    /// Default network will be: `monero`
    pub async fn new(
        cli: &'c Cli,
        container_prefix: Option<String>,
        network_prefix: Option<String>,
        additional_wallets: Vec<String>,
    ) -> Result<(Self, Vec<Container<'c, Cli, image::Monero>>)> {
        let container_prefix = container_prefix.unwrap_or_else(|| "".to_string());

        let monerod_name = format!("{}{}", container_prefix, MONEROD_DAEMON_CONTAINER_NAME);
        let network = format!(
            "{}{}",
            network_prefix.unwrap_or_else(|| "".to_string()),
            MONEROD_DEFAULT_NETWORK
        );

        tracing::info!("Starting monerod...");
        let (monerod, monerod_container) = Monerod::new(cli, monerod_name, network)?;
        let mut containers = vec![monerod_container];
        let mut wallets = vec![];

        tracing::info!("Starting miner wallet...");
        let miner = format!("{}{}", container_prefix, "miner");
        let (miner_wallet, miner_container) = MoneroWalletRpc::new(cli, &miner, &monerod).await?;

        wallets.push(miner_wallet);
        containers.push(miner_container);
        for wallet in additional_wallets.iter() {
            tracing::info!("Starting wallet: {}...", wallet);
            let wallet = format!("{}{}", container_prefix, wallet);
            let (wallet, container) = MoneroWalletRpc::new(cli, &wallet, &monerod).await?;
            wallets.push(wallet);
            containers.push(container);
        }

        Ok((
            Self {
                monerod,
                wallets,
                container_prefix,
            },
            containers,
        ))
    }

    pub fn monerod(&self) -> &Monerod {
        &self.monerod
    }

    pub fn wallet(&self, name: &str) -> Result<&MoneroWalletRpc> {
        let name = format!("{}{}", self.container_prefix, name);
        let wallet = self
            .wallets
            .iter()
            .find(|wallet| wallet.name.eq(&name))
            .ok_or_else(|| anyhow!("Could not find wallet container."))?;

        Ok(wallet)
    }

    pub async fn init(&self, alice_amount: u64, bob_amount: u64) -> Result<()> {
        let miner_wallet = self.wallet("miner")?;
        let miner_address = miner_wallet.address().await?.address;

        // generate the first 70 as bulk
        let monerod = &self.monerod;
        let block = monerod.inner().generate_blocks(70, &miner_address).await?;
        tracing::info!("Generated {:?} blocks", block);
        miner_wallet.refresh().await?;

        if alice_amount > 0 {
            let alice_wallet = self.wallet("alice")?;
            let alice_address = alice_wallet.address().await?.address;
            miner_wallet.transfer(&alice_address, alice_amount).await?;
            tracing::info!("Funded alice wallet with {}", alice_amount);
            monerod.inner().generate_blocks(10, &miner_address).await?;
            alice_wallet.refresh().await?;
        }
        if bob_amount > 0 {
            let bob_wallet = self.wallet("bob")?;
            let bob_address = bob_wallet.address().await?.address;
            miner_wallet.transfer(&bob_address, bob_amount).await?;
            tracing::info!("Funded bob wallet with {}", bob_amount);
            monerod.inner().generate_blocks(10, &miner_address).await?;
            bob_wallet.refresh().await?;
        }

        monerod.start_miner(&miner_address).await?;

        tracing::info!("Waiting for miner wallet to catch up...");
        let block_height = monerod.inner().get_block_count().await?;
        miner_wallet
            .wait_for_wallet_height(block_height)
            .await
            .unwrap();

        Ok(())
    }

    pub async fn fund(&self, address: &str, amount: u64) -> Result<Transfer> {
        self.transfer("miner", address, amount).await
    }

    pub async fn transfer_from_alice(&self, address: &str, amount: u64) -> Result<Transfer> {
        self.transfer("alice", address, amount).await
    }

    pub async fn transfer_from_bob(&self, address: &str, amount: u64) -> Result<Transfer> {
        self.transfer("bob", address, amount).await
    }

    async fn transfer(&self, from_wallet: &str, address: &str, amount: u64) -> Result<Transfer> {
        let from = self.wallet(from_wallet)?;
        let transfer = from.transfer(address, amount).await?;
        let miner_address = self.wallet("miner")?.address().await?.address;
        self.monerod
            .inner()
            .generate_blocks(10, &miner_address)
            .await?;
        from.inner().refresh().await?;
        Ok(transfer)
    }
}

#[derive(Clone, Debug)]
pub struct Monerod {
    rpc_port: u16,
    name: String,
    network: String,
}

#[derive(Clone, Debug)]
pub struct MoneroWalletRpc {
    rpc_port: u16,
    name: String,
    network: String,
}

impl<'c> Monerod {
    /// Starts a new regtest monero container.
    fn new(
        cli: &'c Cli,
        name: String,
        network: String,
    ) -> Result<(Self, Container<'c, Cli, image::Monero>)> {
        let monerod_rpc_port: u16 =
            port_check::free_local_port().ok_or_else(|| anyhow!("Could not retrieve free port"))?;

        let image = image::Monero::default().with_mapped_port(Port {
            local: monerod_rpc_port,
            internal: MONEROD_RPC_PORT,
        });
        let run_args = RunArgs::default()
            .with_name(name.clone())
            .with_network(network.clone());
        let docker = cli.run_with_args(image, run_args);

        Ok((
            Self {
                rpc_port: monerod_rpc_port,
                name,
                network,
            },
            docker,
        ))
    }

    pub fn inner(&self) -> monerod::Client {
        monerod::Client::localhost(self.rpc_port)
    }

    /// Spawns a task to mine blocks in a regular interval to the provided
    /// address
    pub async fn start_miner(&self, miner_wallet_address: &str) -> Result<()> {
        let monerod = self.inner();
        let _ = tokio::spawn(mine(monerod, miner_wallet_address.to_string()));
        Ok(())
    }
}

impl<'c> MoneroWalletRpc {
    /// Starts a new wallet container which is attached to
    /// MONEROD_DEFAULT_NETWORK and MONEROD_DAEMON_CONTAINER_NAME
    async fn new(
        cli: &'c Cli,
        name: &str,
        monerod: &Monerod,
    ) -> Result<(Self, Container<'c, Cli, image::Monero>)> {
        let wallet_rpc_port: u16 =
            port_check::free_local_port().ok_or_else(|| anyhow!("Could not retrieve free port"))?;

        let daemon_address = format!("{}:{}", monerod.name, MONEROD_RPC_PORT);
        let image = image::Monero::wallet(&name, daemon_address).with_mapped_port(Port {
            local: wallet_rpc_port,
            internal: WALLET_RPC_PORT,
        });

        let network = monerod.network.clone();
        let run_args = RunArgs::default()
            .with_name(name)
            .with_network(network.clone());
        let docker = cli.run_with_args(image, run_args);

        // create new wallet
        wallet::Client::localhost(wallet_rpc_port)
            .create_wallet(name)
            .await
            .unwrap();

        Ok((
            Self {
                rpc_port: wallet_rpc_port,
                name: name.to_string(),
                network,
            },
            docker,
        ))
    }

    pub fn inner(&self) -> wallet::Client {
        wallet::Client::localhost(self.rpc_port)
    }

    // It takes a little while for the wallet to sync with monerod.
    pub async fn wait_for_wallet_height(&self, height: u32) -> Result<()> {
        let mut retry: u8 = 0;
        while self.inner().block_height().await?.height < height {
            if retry >= 30 {
                // ~30 seconds
                bail!("Wallet could not catch up with monerod after 30 retries.")
            }
            time::delay_for(Duration::from_millis(WAIT_WALLET_SYNC_MILLIS)).await;
            retry += 1;
        }
        Ok(())
    }

    /// Sends amount to address
    pub async fn transfer(&self, address: &str, amount: u64) -> Result<Transfer> {
        let transfer = self.inner().transfer(0, amount, address).await?;
        self.inner().refresh().await?;
        Ok(transfer)
    }

    pub async fn address(&self) -> Result<GetAddress> {
        self.inner().get_address(0).await
    }

    pub async fn balance(&self) -> Result<u64> {
        self.inner().refresh().await?;
        self.inner().get_balance(0).await
    }

    pub async fn refresh(&self) -> Result<Refreshed> {
        self.inner().refresh().await
    }
}
/// Mine a block ever BLOCK_TIME_SECS seconds.
async fn mine(monerod: monerod::Client, reward_address: String) -> Result<()> {
    loop {
        time::delay_for(Duration::from_secs(BLOCK_TIME_SECS)).await;
        monerod.generate_blocks(1, &reward_address).await?;
    }
}
