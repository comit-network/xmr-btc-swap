#![warn(
    unused_extern_crates,
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
#![allow(non_snake_case)]

use crate::{
    cli::{Cancel, Command, Options, Resume},
    config::{
        initial_setup, query_user_for_initial_testnet_config, read_config, ConfigNotInitialized,
    },
    execution_params::GetExecutionParams,
    protocol::bob::cancel::CancelError,
};
use anyhow::{Context, Result};
use database::Database;
use fs::{default_config_path, default_data_dir};
use log::LevelFilter;
use prettytable::{row, Table};
use protocol::{alice, bob, bob::Builder, SwapAmounts};
use std::{path::PathBuf, sync::Arc};
use structopt::StructOpt;
use trace::init_tracing;
use tracing::info;
use uuid::Uuid;

pub mod bitcoin;
pub mod config;
pub mod database;
pub mod execution_params;
pub mod monero;
pub mod network;
pub mod protocol;
pub mod seed;
pub mod trace;

mod cli;
mod fs;
mod serde_peer_id;

#[macro_use]
extern crate prettytable;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing(LevelFilter::Info).expect("initialize tracing");

    let opt = Options::from_args();

    let data_dir = if let Some(data_dir) = opt.data_dir {
        data_dir
    } else {
        default_data_dir().context("unable to determine default data path")?
    };

    info!(
        "Database and Seed will be stored in directory: {}",
        data_dir.display()
    );

    let db_path = data_dir.join("database");
    let seed = config::seed::Seed::from_file_or_generate(&data_dir)
        .expect("Could not retrieve/initialize seed")
        .into();

    // hardcode to testnet/stagenet
    let bitcoin_network = bitcoin::Network::Testnet;
    let monero_network = monero::Network::Stagenet;
    let execution_params = execution_params::Testnet::get_execution_params();

    match opt.cmd {
        Command::SellXmr {
            listen_addr,
            send_monero,
            receive_bitcoin,
            config,
        } => {
            let swap_amounts = SwapAmounts {
                xmr: send_monero,
                btc: receive_bitcoin,
            };

            let (bitcoin_wallet, monero_wallet) =
                init_wallets(config.path, bitcoin_network, monero_network).await?;

            let swap_id = Uuid::new_v4();

            info!(
                "Swap sending {} and receiving {} started with ID {}",
                send_monero, receive_bitcoin, swap_id
            );

            let alice_factory = alice::Builder::new(
                seed,
                execution_params,
                swap_id,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                db_path,
                listen_addr,
            );
            let (swap, mut event_loop) =
                alice_factory.with_init_params(swap_amounts).build().await?;

            tokio::spawn(async move { event_loop.run().await });
            alice::run(swap).await?;
        }
        Command::BuyXmr {
            alice_peer_id,
            alice_addr,
            send_bitcoin,
            receive_monero,
            config,
        } => {
            let swap_amounts = SwapAmounts {
                btc: send_bitcoin,
                xmr: receive_monero,
            };

            let (bitcoin_wallet, monero_wallet) =
                init_wallets(config.path, bitcoin_network, monero_network).await?;

            let swap_id = Uuid::new_v4();

            info!(
                "Swap sending {} and receiving {} started with ID {}",
                send_bitcoin, receive_monero, swap_id
            );

            let bob_factory = Builder::new(
                seed,
                db_path,
                swap_id,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                alice_addr,
                alice_peer_id,
                execution_params,
            );
            let (swap, event_loop) = bob_factory.with_init_params(swap_amounts).build().await?;

            tokio::spawn(async move { event_loop.run().await });
            bob::run(swap).await?;
        }
        Command::History => {
            let mut table = Table::new();

            table.add_row(row!["SWAP ID", "STATE"]);

            let db = Database::open(db_path.as_path()).context("Could not open database")?;

            for (swap_id, state) in db.all()? {
                table.add_row(row![swap_id, state]);
            }

            // Print the table to stdout
            table.printstd();
        }
        Command::Resume(Resume::SellXmr {
            swap_id,
            listen_addr,
            config,
        }) => {
            let (bitcoin_wallet, monero_wallet) =
                init_wallets(config.path, bitcoin_network, monero_network).await?;

            let alice_factory = alice::Builder::new(
                seed,
                execution_params,
                swap_id,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                db_path,
                listen_addr,
            );
            let (swap, mut event_loop) = alice_factory.build().await?;

            tokio::spawn(async move { event_loop.run().await });
            alice::run(swap).await?;
        }
        Command::Resume(Resume::BuyXmr {
            swap_id,
            alice_peer_id,
            alice_addr,
            config,
        }) => {
            let (bitcoin_wallet, monero_wallet) =
                init_wallets(config.path, bitcoin_network, monero_network).await?;

            let bob_factory = Builder::new(
                seed,
                db_path,
                swap_id,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                alice_addr,
                alice_peer_id,
                execution_params,
            );
            let (swap, event_loop) = bob_factory.build().await?;

            tokio::spawn(async move { event_loop.run().await });
            bob::run(swap).await?;
        }
        Command::Cancel(Cancel::BuyXmr {
            swap_id,
            alice_peer_id,
            alice_addr,
            config,
        }) => {
            // TODO: Optimization: Only init the Bitcoin wallet, Monero wallet unnecessary
            let (bitcoin_wallet, monero_wallet) =
                init_wallets(config.path, bitcoin_network, monero_network).await?;

            let bob_factory = Builder::new(
                seed,
                db_path,
                swap_id,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                alice_addr,
                alice_peer_id,
                execution_params,
            );
            let (swap, event_loop) = bob_factory.build().await?;

            tokio::spawn(async move { event_loop.run().await });

            match bob::cancel(swap.swap_id, swap.state, swap.bitcoin_wallet, swap.db).await? {
                Ok((txid, _)) => { info!("Cancel transaction successfully published with id {}", txid)},
                Err(CancelError::CancelTimelockNotExpiredYet) => {info!("The Cancel Transaction cannot be published yet, because the timelock has not expired. Please try again later.")},
                Err(CancelError::CancelTxAlreadyPublished) => {info!("The Cancel Transaction has already been published.")}
            }
        }
    };

    Ok(())
}

async fn init_wallets(
    config_path: Option<PathBuf>,
    bitcoin_network: bitcoin::Network,
    monero_network: monero::Network,
) -> Result<(bitcoin::Wallet, monero::Wallet)> {
    let config_path = if let Some(config_path) = config_path {
        config_path
    } else {
        default_config_path()?
    };

    let config = match read_config(config_path.clone())? {
        Ok(config) => config,
        Err(ConfigNotInitialized {}) => {
            initial_setup(config_path.clone(), query_user_for_initial_testnet_config)?;
            read_config(config_path)?.expect("after initial setup config can be read")
        }
    };

    let bitcoin_wallet = bitcoin::Wallet::new(
        config.bitcoin.wallet_name.as_str(),
        config.bitcoin.bitcoind_url,
        bitcoin_network,
    )
    .await?;
    let bitcoin_balance = bitcoin_wallet.balance().await?;
    info!(
        "Connection to Bitcoin wallet succeeded, balance: {}",
        bitcoin_balance
    );

    let monero_wallet = monero::Wallet::new(config.monero.wallet_rpc_url, monero_network);
    let monero_balance = monero_wallet.get_balance().await?;
    info!(
        "Connection to Monero wallet succeeded, balance: {}",
        monero_balance
    );

    Ok((bitcoin_wallet, monero_wallet))
}
