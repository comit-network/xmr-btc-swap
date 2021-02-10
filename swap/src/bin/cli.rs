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
use std::{path::PathBuf, sync::Arc};
use structopt::StructOpt;
use swap::{
    bitcoin,
    cli::{Cancel, Command, Options, Refund, Resume},
    config,
    config::{
        initial_setup, query_user_for_initial_testnet_config, read_config, ConfigNotInitialized,
    },
    database::Database,
    execution_params,
    execution_params::GetExecutionParams,
    fs::{default_config_path, default_data_dir},
    monero,
    monero::{CreateWallet, OpenWallet},
    protocol::{
        bob,
        bob::{cancel::CancelError, Builder},
        SwapAmounts,
    },
    trace::init_tracing,
};
use tracing::{error, info, warn};
use uuid::Uuid;

#[macro_use]
extern crate prettytable;

const MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME: &str = "swap-tool-blockchain-monitoring-wallet";

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing(LevelFilter::Debug).expect("initialize tracing");

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
    let seed = config::Seed::from_file_or_generate(&data_dir)
        .expect("Could not retrieve/initialize seed")
        .into();

    // hardcode to testnet/stagenet
    let bitcoin_network = bitcoin::Network::Testnet;
    let monero_network = monero::Network::Stagenet;
    let execution_params = execution_params::Testnet::get_execution_params();

    match opt.cmd {
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
            force,
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

            match bob::cancel(
                swap.swap_id,
                swap.state,
                swap.bitcoin_wallet,
                swap.db,
                force,
            )
            .await?
            {
                Ok((txid, _)) => {
                    info!("Cancel transaction successfully published with id {}", txid)
                }
                Err(CancelError::CancelTimelockNotExpiredYet) => error!(
                    "The Cancel Transaction cannot be published yet, \
                    because the timelock has not expired. Please try again later."
                ),
                Err(CancelError::CancelTxAlreadyPublished) => {
                    warn!("The Cancel Transaction has already been published.")
                }
            }
        }
        Command::Refund(Refund::BuyXmr {
            swap_id,
            alice_peer_id,
            alice_addr,
            config,
            force,
        }) => {
            let (bitcoin_wallet, monero_wallet) =
                init_wallets(config.path, bitcoin_network, monero_network).await?;

            // TODO: Optimize to only use the Bitcoin wallet, Monero wallet is unnecessary
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
            bob::refund(
                swap.swap_id,
                swap.state,
                swap.execution_params,
                swap.bitcoin_wallet,
                swap.db,
                force,
            )
            .await??;
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

    let monero_wallet = monero::Wallet::new(config.monero.wallet_rpc_url.clone(), monero_network);

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
