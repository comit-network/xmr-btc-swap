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
use serde::Deserialize;
use std::time::Duration;
use testcontainers::{clients::Cli, core::Port, Container, Docker, RunArgs};
use tokio::time;

use crate::{
    image::{
        MONEROD_DAEMON_CONTAINER_NAME, MONEROD_DEFAULT_NETWORK, MONEROD_RPC_PORT, WALLET_RPC_PORT,
    },
    rpc::{
        monerod,
        wallet::{self, GetAddress, Transfer},
    },
};

/// How often we mine a block.
const BLOCK_TIME_SECS: u64 = 1;

/// Poll interval when checking if the wallet has synced with monerod.
const WAIT_WALLET_SYNC_MILLIS: u64 = 1000;

#[derive(Clone, Debug)]
pub struct Monero {
    rpc_port: u16,
    name: String,
}

impl<'c> Monero {
    /// Starts a new regtest monero container.
    pub fn new_monerod(cli: &'c Cli) -> Result<(Self, Container<'c, Cli, image::Monero>)> {
        let monerod_rpc_port: u16 =
            port_check::free_local_port().ok_or_else(|| anyhow!("Could not retrieve free port"))?;

        let image = image::Monero::default().with_mapped_port(Port {
            local: monerod_rpc_port,
            internal: MONEROD_RPC_PORT,
        });

        let run_args = RunArgs::default()
            .with_name(MONEROD_DAEMON_CONTAINER_NAME)
            .with_network(MONEROD_DEFAULT_NETWORK);
        let docker = cli.run_with_args(image, run_args);

        Ok((
            Self {
                rpc_port: monerod_rpc_port,
                name: "monerod".to_string(),
            },
            docker,
        ))
    }

    pub async fn new_wallet(
        cli: &'c Cli,
        name: &str,
    ) -> Result<(Self, Container<'c, Cli, image::Monero>)> {
        let wallet_rpc_port: u16 =
            port_check::free_local_port().ok_or_else(|| anyhow!("Could not retrieve free port"))?;

        let image = image::Monero::wallet(&name).with_mapped_port(Port {
            local: wallet_rpc_port,
            internal: WALLET_RPC_PORT,
        });

        let run_args = RunArgs::default()
            .with_name(name)
            .with_network(MONEROD_DEFAULT_NETWORK);
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
            },
            docker,
        ))
    }

    pub fn monerod_rpc_client(&self) -> monerod::Client {
        monerod::Client::localhost(self.rpc_port)
    }

    pub fn wallet_rpc_client(&self) -> wallet::Client {
        wallet::Client::localhost(self.rpc_port)
    }

    /// Spawns a task to mine blocks in a regular interval to the provided
    /// address
    pub async fn start_miner(&self, miner_wallet_address: &str) -> Result<()> {
        let monerod = self.monerod_rpc_client();
        // generate the first 70 as bulk
        let block = monerod.generate_blocks(70, &miner_wallet_address).await?;
        println!("Generated {:?} blocks", block);
        let _ = tokio::spawn(mine(monerod.clone(), miner_wallet_address.to_string()));
        Ok(())
    }

    // It takes a little while for the wallet to sync with monerod.
    pub async fn wait_for_wallet_height(&self, height: u32) -> Result<()> {
        let mut retry: u8 = 0;
        while self.wallet_rpc_client().block_height().await?.height < height {
            if retry >= 30 {
                // ~30 seconds
                bail!("Wallet could not catch up with monerod after 30 retries.")
            }
            time::delay_for(Duration::from_millis(WAIT_WALLET_SYNC_MILLIS)).await;
            retry += 1;
        }
        Ok(())
    }
}

/// Mine a block ever BLOCK_TIME_SECS seconds.
async fn mine(monerod: monerod::Client, reward_address: String) -> Result<()> {
    loop {
        time::delay_for(Duration::from_secs(BLOCK_TIME_SECS)).await;
        monerod.generate_blocks(1, &reward_address).await?;
    }
}

// We should be able to use monero-rs for this but it does not include all
// the fields.
#[derive(Clone, Debug, Deserialize)]
pub struct BlockHeader {
    pub block_size: u32,
    pub depth: u32,
    pub difficulty: u32,
    pub hash: String,
    pub height: u32,
    pub major_version: u32,
    pub minor_version: u32,
    pub nonce: u32,
    pub num_txes: u32,
    pub orphan_status: bool,
    pub prev_hash: String,
    pub reward: u64,
    pub timestamp: u32,
}
