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
use prettytable::{row, Table};
use reqwest::Url;
use std::{path::Path, sync::Arc, time::Duration};
use structopt::StructOpt;
use swap::{
    bitcoin,
    bitcoin::Amount,
    cli::{
        command::{Arguments, Cancel, Command, Refund, Resume},
        config::{read_config, Config},
    },
    database::Database,
    execution_params,
    execution_params::GetExecutionParams,
    monero,
    monero::{CreateWallet, OpenWallet},
    protocol::{
        bob,
        bob::{cancel::CancelError, Builder},
    },
    seed::Seed,
    trace::init_tracing,
};
use tracing::{debug, error, info, warn};
use tracing_subscriber::filter::LevelFilter;
use uuid::Uuid;

#[macro_use]
extern crate prettytable;

const MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME: &str = "swap-tool-blockchain-monitoring-wallet";

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing(LevelFilter::DEBUG).expect("initialize tracing");

    let opt = Arguments::from_args();

    let config = match opt.config {
        Some(config_path) => read_config(config_path)??,
        None => Config::testnet(),
    };

    info!(
        "Database and Seed will be stored in directory: {}",
        config.data.dir.display()
    );

    let db = Database::open(config.data.dir.join("database").as_path())
        .context("Could not open database")?;

    let wallet_data_dir = config.data.dir.join("wallet");
    let seed =
        Seed::from_file_or_generate(&config.data.dir).expect("Could not retrieve/initialize seed");

    // hardcode to testnet/stagenet
    let bitcoin_network = bitcoin::Network::Testnet;
    let monero_network = monero::Network::Stagenet;
    let execution_params = execution_params::Testnet::get_execution_params();

    let monero_wallet_rpc = monero::WalletRpc::new(config.data.dir.join("monero")).await?;

    let monero_wallet_rpc_process = monero_wallet_rpc
        .run(monero_network, "stagenet.community.xmr.to")
        .await?;

    match opt.cmd {
        Command::BuyXmr {
            alice_peer_id,
            alice_addr,
        } => {
            let (bitcoin_wallet, monero_wallet) = init_wallets(
                config,
                bitcoin_network,
                &wallet_data_dir,
                monero_network,
                seed,
                monero_wallet_rpc_process.endpoint(),
            )
            .await?;

            // TODO: Also wait for more funds if balance < dust
            if bitcoin_wallet.balance().await? == Amount::ZERO {
                debug!(
                    "Waiting for BTC at address {}",
                    bitcoin_wallet.new_address().await?
                );

                while bitcoin_wallet.balance().await? == Amount::ZERO {
                    bitcoin_wallet.sync_wallet().await?;

                    tokio::time::sleep(Duration::from_secs(1)).await;
                }

                debug!("Received {}", bitcoin_wallet.balance().await?);
            }

            let send_bitcoin = bitcoin_wallet.max_giveable().await?;

            info!("Swapping {} ...", send_bitcoin);

            let bob_factory = Builder::new(
                seed,
                db,
                Uuid::new_v4(),
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                alice_addr,
                alice_peer_id,
                execution_params,
            );
            let (swap, event_loop) = bob_factory.with_init_params(send_bitcoin).build().await?;

            tokio::spawn(async move { event_loop.run().await });
            bob::run(swap).await?;
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
        Command::Resume(Resume::BuyXmr {
            swap_id,
            alice_peer_id,
            alice_addr,
        }) => {
            let (bitcoin_wallet, monero_wallet) = init_wallets(
                config,
                bitcoin_network,
                &wallet_data_dir,
                monero_network,
                seed,
                monero_wallet_rpc_process.endpoint(),
            )
            .await?;

            let bob_factory = Builder::new(
                seed,
                db,
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
            force,
        }) => {
            // TODO: Optimization: Only init the Bitcoin wallet, Monero wallet unnecessary
            let (bitcoin_wallet, monero_wallet) = init_wallets(
                config,
                bitcoin_network,
                &wallet_data_dir,
                monero_network,
                seed,
                monero_wallet_rpc_process.endpoint(),
            )
            .await?;

            let bob_factory = Builder::new(
                seed,
                db,
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
            force,
        }) => {
            let (bitcoin_wallet, monero_wallet) = init_wallets(
                config,
                bitcoin_network,
                &wallet_data_dir,
                monero_network,
                seed,
                monero_wallet_rpc_process.endpoint(),
            )
            .await?;

            // TODO: Optimize to only use the Bitcoin wallet, Monero wallet is unnecessary
            let bob_factory = Builder::new(
                seed,
                db,
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
    config: Config,
    bitcoin_network: bitcoin::Network,
    bitcoin_wallet_data_dir: &Path,
    monero_network: monero::Network,
    seed: Seed,
    monero_wallet_rpc_url: Url,
) -> Result<(bitcoin::Wallet, monero::Wallet)> {
    let bitcoin_wallet = bitcoin::Wallet::new(
        config.bitcoin.electrum_rpc_url,
        config.bitcoin.electrum_http_url,
        bitcoin_network,
        bitcoin_wallet_data_dir,
        seed.extended_private_key(bitcoin_network)?.private_key,
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

    let monero_wallet = monero::Wallet::new(
        monero_wallet_rpc_url.clone(),
        monero_network,
        MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME.to_string(),
    );

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
                monero_wallet_rpc_url
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

    let _test_wallet_connection = monero_wallet.block_height().await?;
    info!("The Monero wallet RPC is set up correctly!");

    Ok((bitcoin_wallet, monero_wallet))
}
