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
use log::LevelFilter;
use prettytable::{row, Table};
use std::{path::Path, sync::Arc};
use structopt::StructOpt;
use swap::{
    bitcoin,
    database::Database,
    execution_params,
    execution_params::GetExecutionParams,
    fs::default_config_path,
    monero,
    monero::{CreateWallet, OpenWallet},
    nectar::{
        command::{Arguments, Command},
        config::{
            initial_setup, query_user_for_initial_testnet_config, read_config, Config,
            ConfigNotInitialized,
        },
    },
    protocol::alice::EventLoop,
    seed::Seed,
    trace::init_tracing,
};
use tracing::{info, warn};

#[macro_use]
extern crate prettytable;

const MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME: &str = "swap-tool-blockchain-monitoring-wallet";
const BITCOIN_NETWORK: bitcoin::Network = bitcoin::Network::Testnet;
const MONERO_NETWORK: monero::Network = monero::Network::Stagenet;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing(LevelFilter::Debug).expect("initialize tracing");

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
        Command::Start => {
            let seed = Seed::from_file_or_generate(&config.data.dir)
                .expect("Could not retrieve/initialize seed");

            let execution_params = execution_params::Testnet::get_execution_params();

            let (bitcoin_wallet, monero_wallet) = init_wallets(
                config.clone(),
                &wallet_data_dir,
                seed.extended_private_key(BITCOIN_NETWORK)?.private_key,
            )
            .await?;

            info!(
                "BTC deposit address: {}",
                bitcoin_wallet.new_address().await?
            );

            let (mut event_loop, _) = EventLoop::new(
                config.network.listen,
                seed,
                execution_params,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                Arc::new(db),
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
    private_key: ::bitcoin::PrivateKey,
) -> Result<(bitcoin::Wallet, monero::Wallet)> {
    let bitcoin_wallet = bitcoin::Wallet::new(
        config.bitcoin.electrum_rpc_url,
        config.bitcoin.electrum_http_url,
        BITCOIN_NETWORK,
        bitcoin_wallet_data_dir,
        private_key,
    )
    .await?;

    bitcoin_wallet
        .sync_wallet()
        .await
        .expect("Could not sync btc wallet");

    let bitcoin_balance = bitcoin_wallet.balance().await?;
    info!(
        "Connection to Bitcoin wallet succeeded, balance: {}",
        bitcoin_balance
    );

    let monero_wallet = monero::Wallet::new(config.monero.wallet_rpc_url.clone(), MONERO_NETWORK);

    // Setup the temporary Monero wallet necessary for monitoring the blockchain
    let open_monitoring_wallet_response = monero_wallet
        .open_wallet(MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME)
        .await;
    if open_monitoring_wallet_response.is_err() {
        monero_wallet
            .create_wallet(MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME)
            .await
            .context(format!(
                "Unable to create Monero wallet for blockchain monitoring.\
             Please ensure that the monero-wallet-rpc is available at {}",
                config.monero.wallet_rpc_url
            ))?;

        info!(
            "Created Monero wallet for blockchain monitoring with name {}",
            MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME
        );
    } else {
        info!(
            "Opened Monero wallet for blockchain monitoring with name {}",
            MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME
        );
    }

    let _test_wallet_connection = monero_wallet.inner.block_height().await?;
    info!("The Monero wallet RPC is set up correctly!");

    Ok((bitcoin_wallet, monero_wallet))
}
