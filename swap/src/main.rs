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
#![allow(non_snake_case)]

use crate::cli::{Command, Options, Resume};
use anyhow::{Context, Result};
use prettytable::{row, Table};
use std::sync::Arc;
use structopt::StructOpt;
use swap::{
    bitcoin,
    config::Config,
    database::Database,
    monero,
    protocol::{alice, bob, bob::SwapFactory, StartingBalances},
    trace::init_tracing,
    SwapAmounts,
};
use tracing::{info, log::LevelFilter};
use uuid::Uuid;

mod cli;

#[macro_use]
extern crate prettytable;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing(LevelFilter::Info).expect("initialize tracing");

    let opt = Options::from_args();
    let config = Config::testnet();

    info!(
        "Database and Seed will be stored in directory: {}",
        opt.data_dir
    );
    let data_dir = std::path::Path::new(opt.data_dir.as_str()).to_path_buf();
    let db_path = data_dir.join("database");

    let seed = swap::config::seed::Seed::from_file_or_generate(&data_dir)
        .expect("Could not retrieve/initialize seed")
        .into();

    match opt.cmd {
        Command::SellXmr {
            bitcoind_url,
            bitcoin_wallet_name,
            monero_wallet_rpc_url,
            listen_addr,
            send_monero,
            receive_bitcoin,
        } => {
            let swap_amounts = SwapAmounts {
                xmr: send_monero,
                btc: receive_bitcoin,
            };

            let (bitcoin_wallet, monero_wallet, starting_balances) = setup_wallets(
                bitcoind_url,
                bitcoin_wallet_name.as_str(),
                monero_wallet_rpc_url,
                config,
            )
            .await?;

            let swap_id = Uuid::new_v4();

            info!(
                "Swap sending {} and receiving {} started with ID {}",
                send_monero, receive_bitcoin, swap_id
            );

            let alice_factory = alice::SwapFactory::new(
                seed,
                config,
                swap_id,
                bitcoin_wallet,
                monero_wallet,
                starting_balances,
                db_path,
                listen_addr,
            )
            .await;
            let (swap, mut event_loop) = alice_factory.new_swap_as_alice(swap_amounts).await?;

            tokio::spawn(async move { event_loop.run().await });
            alice::run(swap).await?;
        }
        Command::BuyXmr {
            alice_peer_id,
            alice_addr,
            bitcoind_url,
            bitcoin_wallet_name,
            monero_wallet_rpc_url,
            send_bitcoin,
            receive_monero,
        } => {
            let swap_amounts = SwapAmounts {
                btc: send_bitcoin,
                xmr: receive_monero,
            };

            let (bitcoin_wallet, monero_wallet, starting_balances) = setup_wallets(
                bitcoind_url,
                bitcoin_wallet_name.as_str(),
                monero_wallet_rpc_url,
                config,
            )
            .await?;

            let swap_id = Uuid::new_v4();

            info!(
                "Swap sending {} and receiving {} started with ID {}",
                send_bitcoin, receive_monero, swap_id
            );

            let bob_factory = SwapFactory::new(
                seed,
                db_path,
                swap_id,
                bitcoin_wallet,
                monero_wallet,
                config,
                starting_balances,
                alice_addr,
                alice_peer_id,
            );
            let (swap, event_loop) = bob_factory.new_swap_as_bob(swap_amounts).await?;

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
            bitcoind_url,
            bitcoin_wallet_name,
            monero_wallet_rpc_url,
            listen_addr,
        }) => {
            let (bitcoin_wallet, monero_wallet, starting_balances) = setup_wallets(
                bitcoind_url,
                bitcoin_wallet_name.as_str(),
                monero_wallet_rpc_url,
                config,
            )
            .await?;

            let alice_factory = alice::SwapFactory::new(
                seed,
                config,
                swap_id,
                bitcoin_wallet,
                monero_wallet,
                starting_balances,
                db_path,
                listen_addr,
            )
            .await;
            let (swap, mut event_loop) = alice_factory.recover_alice_from_db().await?;

            tokio::spawn(async move { event_loop.run().await });
            alice::run(swap).await?;
        }
        Command::Resume(Resume::BuyXmr {
            swap_id,
            bitcoind_url,
            bitcoin_wallet_name,
            monero_wallet_rpc_url,
            alice_peer_id,
            alice_addr,
        }) => {
            let (bitcoin_wallet, monero_wallet, starting_balances) = setup_wallets(
                bitcoind_url,
                bitcoin_wallet_name.as_str(),
                monero_wallet_rpc_url,
                config,
            )
            .await?;

            let bob_factory = SwapFactory::new(
                seed,
                db_path,
                swap_id,
                bitcoin_wallet,
                monero_wallet,
                config,
                starting_balances,
                alice_addr,
                alice_peer_id,
            );
            let (swap, event_loop) = bob_factory.resume().await?;

            tokio::spawn(async move { event_loop.run().await });
            bob::run(swap).await?;
        }
    };

    Ok(())
}

async fn setup_wallets(
    bitcoind_url: url::Url,
    bitcoin_wallet_name: &str,
    monero_wallet_rpc_url: url::Url,
    config: Config,
) -> Result<(
    Arc<swap::bitcoin::Wallet>,
    Arc<swap::monero::Wallet>,
    StartingBalances,
)> {
    let bitcoin_wallet =
        swap::bitcoin::Wallet::new(bitcoin_wallet_name, bitcoind_url, config.bitcoin_network)
            .await?;
    let bitcoin_balance = bitcoin_wallet.balance().await?;
    info!(
        "Connection to Bitcoin wallet succeeded, balance: {}",
        bitcoin_balance
    );
    let bitcoin_wallet = Arc::new(bitcoin_wallet);

    let monero_wallet = monero::Wallet::new(monero_wallet_rpc_url, config.monero_network);
    let monero_balance = monero_wallet.get_balance().await?;
    info!(
        "Connection to Monero wallet succeeded, balance: {}",
        monero_balance
    );
    let monero_wallet = Arc::new(monero_wallet);

    let starting_balances = StartingBalances {
        btc: bitcoin_balance,
        xmr: monero_balance,
    };

    Ok((bitcoin_wallet, monero_wallet, starting_balances))
}
