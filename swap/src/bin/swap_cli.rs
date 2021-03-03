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
    bitcoin::{Amount, TxLock},
    cli::{
        command::{Arguments, Command},
        config::{read_config, Config},
    },
    database::Database,
    execution_params,
    execution_params::GetExecutionParams,
    monero,
    monero::OpenOrCreate,
    protocol::{
        bob,
        bob::{cancel::CancelError, Builder},
    },
    seed::Seed,
};
use tracing::{debug, error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;
use uuid::Uuid;

#[macro_use]
extern crate prettytable;

const MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME: &str = "swap-tool-blockchain-monitoring-wallet";

#[tokio::main]
async fn main() -> Result<()> {
    let args = Arguments::from_args();

    let is_terminal = atty::is(atty::Stream::Stderr);
    let base_subscriber = |level| {
        FmtSubscriber::builder()
            .with_writer(std::io::stderr)
            .with_ansi(is_terminal)
            .with_target(false)
            .with_env_filter(format!("swap={}", level))
    };

    if args.debug {
        let subscriber = base_subscriber(Level::DEBUG)
            .with_timer(tracing_subscriber::fmt::time::ChronoLocal::with_format(
                "%F %T".to_owned(),
            ))
            .finish();

        tracing::subscriber::set_global_default(subscriber)?;
    } else {
        let subscriber = base_subscriber(Level::INFO)
            .without_time()
            .with_level(false)
            .finish();

        tracing::subscriber::set_global_default(subscriber)?;
    }

    let config = match args.config {
        Some(config_path) => read_config(config_path)??,
        None => Config::testnet(),
    };

    debug!(
        "Database and seed will be stored in {}",
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

    match args.cmd {
        Command::BuyXmr {
            receive_monero_address,
            alice_peer_id,
            alice_addr,
        } => {
            let bitcoin_wallet =
                init_bitcoin_wallet(config, bitcoin_network, &wallet_data_dir, seed).await?;
            let monero_wallet =
                init_monero_wallet(monero_network, monero_wallet_rpc_process.endpoint()).await?;

            let swap_id = Uuid::new_v4();

            // TODO: Also wait for more funds if balance < dust
            if bitcoin_wallet.balance().await? == Amount::ZERO {
                info!(
                    "Please deposit BTC to {}",
                    bitcoin_wallet.new_address().await?
                );

                while bitcoin_wallet.balance().await? == Amount::ZERO {
                    bitcoin_wallet.sync_wallet().await?;

                    tokio::time::sleep(Duration::from_secs(1)).await;
                }

                debug!("Received {}", bitcoin_wallet.balance().await?);
            } else {
                info!(
                    "Still got {} left in wallet, swapping ...",
                    bitcoin_wallet.balance().await?
                );
            }

            let send_bitcoin = bitcoin_wallet.max_giveable(TxLock::script_size()).await?;

            let bob_factory = Builder::new(
                seed,
                db,
                swap_id,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                alice_addr,
                alice_peer_id,
                execution_params,
                receive_monero_address,
            );
            let (swap, event_loop) = bob_factory.with_init_params(send_bitcoin).build().await?;

            let handle = tokio::spawn(async move { event_loop.run().await });
            let swap = bob::run(swap);
            tokio::select! {
                event_loop_result = handle => {
                    event_loop_result??;
                },
                swap_result = swap => {
                    swap_result?;
                }
            }
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
        Command::Resume {
            receive_monero_address,
            swap_id,
            alice_peer_id,
            alice_addr,
        } => {
            let bitcoin_wallet =
                init_bitcoin_wallet(config, bitcoin_network, &wallet_data_dir, seed).await?;
            let monero_wallet =
                init_monero_wallet(monero_network, monero_wallet_rpc_process.endpoint()).await?;

            let bob_factory = Builder::new(
                seed,
                db,
                swap_id,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                alice_addr,
                alice_peer_id,
                execution_params,
                receive_monero_address,
            );
            let (swap, event_loop) = bob_factory.build().await?;
            let handle = tokio::spawn(async move { event_loop.run().await });
            let swap = bob::run(swap);
            tokio::select! {
                event_loop_result = handle => {
                    event_loop_result??;
                },
                swap_result = swap => {
                    swap_result?;
                }
            }
        }
        Command::Cancel { swap_id, force } => {
            let bitcoin_wallet =
                init_bitcoin_wallet(config, bitcoin_network, &wallet_data_dir, seed).await?;

            let resume_state = db.get_state(swap_id)?.try_into_bob()?.into();
            let cancel =
                bob::cancel(swap_id, resume_state, Arc::new(bitcoin_wallet), db, force).await?;

            match cancel {
                Ok((txid, _)) => {
                    debug!("Cancel transaction successfully published with id {}", txid)
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
        Command::Refund { swap_id, force } => {
            let bitcoin_wallet =
                init_bitcoin_wallet(config, bitcoin_network, &wallet_data_dir, seed).await?;

            let resume_state = db.get_state(swap_id)?.try_into_bob()?.into();

            bob::refund(
                swap_id,
                resume_state,
                execution_params,
                Arc::new(bitcoin_wallet),
                db,
                force,
            )
            .await??;
        }
    };
    Ok(())
}

async fn init_bitcoin_wallet(
    config: Config,
    bitcoin_network: bitcoin::Network,
    bitcoin_wallet_data_dir: &Path,
    seed: Seed,
) -> Result<bitcoin::Wallet> {
    let bitcoin_wallet = bitcoin::Wallet::new(
        config.bitcoin.electrum_rpc_url,
        config.bitcoin.electrum_http_url,
        bitcoin_network,
        bitcoin_wallet_data_dir,
        seed.extended_private_key(bitcoin_network)?,
    )
    .await?;

    bitcoin_wallet
        .sync_wallet()
        .await
        .context("failed to sync balance of bitcoin wallet")?;

    Ok(bitcoin_wallet)
}

async fn init_monero_wallet(
    monero_network: monero::Network,
    monero_wallet_rpc_url: Url,
) -> Result<monero::Wallet> {
    let monero_wallet = monero::Wallet::new(
        monero_wallet_rpc_url.clone(),
        monero_network,
        MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME.to_string(),
    );

    monero_wallet.open_or_create().await?;

    let _test_wallet_connection = monero_wallet
        .block_height()
        .await
        .context("failed to validate connection to monero-wallet-rpc")?;

    Ok(monero_wallet)
}
