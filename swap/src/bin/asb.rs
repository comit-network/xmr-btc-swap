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

use anyhow::{Context, Result};
use bdk::descriptor::Segwitv0;
use bdk::keys::DerivableKey;
use prettytable::{row, Table};
use std::path::Path;
use std::sync::Arc;
use structopt::StructOpt;
use swap::asb::command::{Arguments, Command};
use swap::asb::config::{
    initial_setup, query_user_for_initial_testnet_config, read_config, Config, ConfigNotInitialized,
};
use swap::database::Database;
use swap::execution_params::{ExecutionParams, GetExecutionParams};
use swap::fs::default_config_path;
use swap::monero::Amount;
use swap::protocol::alice::EventLoop;
use swap::seed::Seed;
use swap::trace::init_tracing;
use swap::{bitcoin, execution_params, kraken, monero};
use tracing::{info, warn};
use tracing_subscriber::filter::LevelFilter;

#[macro_use]
extern crate prettytable;

const DEFAULT_WALLET_NAME: &str = "asb-wallet";
const BITCOIN_NETWORK: bitcoin::Network = bitcoin::Network::Testnet;
const MONERO_NETWORK: monero::Network = monero::Network::Stagenet;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing(LevelFilter::DEBUG).expect("initialize tracing");

    let opt = Arguments::from_args();

    let config_path = if let Some(config_path) = opt.config {
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

    info!(
        "Database and Seed will be stored in directory: {}",
        config.data.dir.display()
    );

    let db_path = config.data.dir.join("database");

    let db = Database::open(config.data.dir.join(db_path).as_path())
        .context("Could not open database")?;

    let wallet_data_dir = config.data.dir.join("wallet");

    match opt.cmd {
        Command::Start { max_buy } => {
            let seed = Seed::from_file_or_generate(&config.data.dir)
                .expect("Could not retrieve/initialize seed");

            let execution_params = execution_params::Testnet::get_execution_params();

            let (bitcoin_wallet, monero_wallet) = init_wallets(
                config.clone(),
                &wallet_data_dir,
                seed.derive_extended_private_key(BITCOIN_NETWORK)?,
                execution_params,
            )
            .await?;

            info!(
                "BTC deposit address: {}",
                bitcoin_wallet.new_address().await?
            );

            let kraken_rate_updates = kraken::connect()?;

            let (event_loop, _) = EventLoop::new(
                config.network.listen,
                seed,
                execution_params,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                Arc::new(db),
                kraken_rate_updates,
                max_buy,
            )
            .unwrap();

            info!("Our peer id is {}", event_loop.peer_id());

            event_loop.run().await;
        }
        Command::History => {
            let mut table = Table::new();

            table.add_row(row!["SWAP ID", "STATE"]);

            for (swap_id, state) in db.all()? {
                table.add_row(row![swap_id, state]);
            }

            // Print the table to stdout
            table.printstd();
        }
    };

    Ok(())
}

async fn init_wallets(
    config: Config,
    bitcoin_wallet_data_dir: &Path,
    key: impl DerivableKey<Segwitv0> + Clone,
    execution_params: ExecutionParams,
) -> Result<(bitcoin::Wallet, monero::Wallet)> {
    let bitcoin_wallet = bitcoin::Wallet::new(
        config.bitcoin.electrum_rpc_url,
        config.bitcoin.electrum_http_url,
        BITCOIN_NETWORK,
        bitcoin_wallet_data_dir,
        key,
    )
    .await?;

    bitcoin_wallet.sync().await?;

    let bitcoin_balance = bitcoin_wallet.balance().await?;
    info!(
        "Connection to Bitcoin wallet succeeded, balance: {}",
        bitcoin_balance
    );

    let monero_wallet = monero::Wallet::new(
        config.monero.wallet_rpc_url.clone(),
        MONERO_NETWORK,
        DEFAULT_WALLET_NAME.to_string(),
        execution_params.monero_avg_block_time,
    );

    // Setup the Monero wallet
    monero_wallet.open_or_create().await?;

    let balance = monero_wallet.get_balance().await?;
    if balance == Amount::ZERO {
        let deposit_address = monero_wallet.get_main_address().await?;
        warn!(
            "The Monero balance is 0, make sure to deposit funds at: {}",
            deposit_address
        )
    } else {
        info!("Monero balance: {}", balance);
    }

    Ok((bitcoin_wallet, monero_wallet))
}
