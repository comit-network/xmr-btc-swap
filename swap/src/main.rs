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
    cli::{Command, Options, Resume},
    config::{
        initial_setup, query_user_for_initial_testnet_config, read_config, reset_config,
        ConfigNotInitialized,
    },
};
use anyhow::{Context, Result};
use database::Database;
use fs::default_config_path;
use prettytable::{row, Table};
use protocol::{alice, bob, bob::Builder, SwapAmounts};
use settings::Settings;
use std::{path::PathBuf, sync::Arc};
use structopt::StructOpt;
use trace::init_tracing;
use tracing::{info, log::LevelFilter};
use uuid::Uuid;

pub mod bitcoin;
pub mod config;
pub mod database;
pub mod monero;
pub mod network;
pub mod protocol;
pub mod seed;
pub mod settings;
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

    info!(
        "Database and Seed will be stored in directory: {}",
        opt.data_dir
    );
    let data_dir = std::path::Path::new(opt.data_dir.as_str()).to_path_buf();
    let db_path = data_dir.join("database");

    let seed = config::seed::Seed::from_file_or_generate(&data_dir)
        .expect("Could not retrieve/initialize seed")
        .into();

    match opt.cmd {
        Command::SellXmr {
            listen_addr,
            send_monero,
            receive_bitcoin,
            config,
        } => {
            let settings = init_settings(config.config_path)?;

            let swap_amounts = SwapAmounts {
                xmr: send_monero,
                btc: receive_bitcoin,
            };

            let (bitcoin_wallet, monero_wallet) = setup_wallets(settings.wallets).await?;

            let swap_id = Uuid::new_v4();

            info!(
                "Swap sending {} and receiving {} started with ID {}",
                send_monero, receive_bitcoin, swap_id
            );

            let alice_factory = alice::Builder::new(
                seed,
                settings.protocol,
                swap_id,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                db_path,
                listen_addr,
            )
            .await;
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
            let settings = init_settings(config.config_path)?;

            let swap_amounts = SwapAmounts {
                btc: send_bitcoin,
                xmr: receive_monero,
            };

            let (bitcoin_wallet, monero_wallet) = setup_wallets(settings.wallets).await?;

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
                settings.protocol,
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
            let settings = init_settings(config.config_path)?;

            let (bitcoin_wallet, monero_wallet) = setup_wallets(settings.wallets).await?;

            let alice_factory = alice::Builder::new(
                seed,
                settings.protocol,
                swap_id,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                db_path,
                listen_addr,
            )
            .await;
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
            let settings = init_settings(config.config_path)?;

            let (bitcoin_wallet, monero_wallet) = setup_wallets(settings.wallets).await?;

            let bob_factory = Builder::new(
                seed,
                db_path,
                swap_id,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                alice_addr,
                alice_peer_id,
                settings.protocol,
            );
            let (swap, event_loop) = bob_factory.build().await?;

            tokio::spawn(async move { event_loop.run().await });
            bob::run(swap).await?;
        }
        Command::ResetConfig { config } => reset_config(config.config_path)?,
    };

    Ok(())
}

fn init_settings(config_path: Option<PathBuf>) -> Result<Settings> {
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

    let settings = Settings::from_config_file_and_defaults(config);

    Ok(settings)
}

async fn setup_wallets(settings: settings::Wallets) -> Result<(bitcoin::Wallet, monero::Wallet)> {
    let bitcoin_wallet = bitcoin::Wallet::new(
        settings.bitcoin.wallet_name.as_str(),
        settings.bitcoin.bitcoind_url,
        settings.bitcoin.network,
    )
    .await?;
    let bitcoin_balance = bitcoin_wallet.balance().await?;
    info!(
        "Connection to Bitcoin wallet succeeded, balance: {}",
        bitcoin_balance
    );

    let monero_wallet =
        monero::Wallet::new(settings.monero.wallet_rpc_url, settings.monero.network);
    let monero_balance = monero_wallet.get_balance().await?;
    info!(
        "Connection to Monero wallet succeeded, balance: {}",
        monero_balance
    );

    Ok((bitcoin_wallet, monero_wallet))
}
