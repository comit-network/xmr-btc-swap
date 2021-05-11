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

use anyhow::{bail, Context, Result};
use prettytable::{row, Table};
use std::cmp::min;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use structopt::StructOpt;
use swap::bitcoin::TxLock;
use swap::cli::command::{Arguments, Command, MoneroParams};
use swap::database::Database;
use swap::env::{Config, GetConfig};
use swap::network::quote::BidQuote;
use swap::network::swarm;
use swap::protocol::bob;
use swap::protocol::bob::{EventLoop, Swap};
use swap::seed::Seed;
use swap::{bitcoin, cli, env, monero};
use tracing::{debug, error, info, warn};
use url::Url;
use uuid::Uuid;

#[macro_use]
extern crate prettytable;

#[tokio::main]
async fn main() -> Result<()> {
    let Arguments { data, debug, cmd } = Arguments::from_args();

    match cmd {
        Command::BuyXmr {
            alice_peer_id,
            alice_multiaddr,
            monero_params:
                MoneroParams {
                    receive_monero_address,
                    monero_daemon_host,
                },
            electrum_rpc_url,
            tor_socks5_port,
            bitcoin_target_block,
        } => {
            let swap_id = Uuid::new_v4();

            let data_dir = data.0;
            cli::tracing::init(debug, data_dir.join("logs"), swap_id)?;
            let db = Database::open(data_dir.join("database").as_path())
                .context("Failed to open database")?;
            let seed = Seed::from_file_or_generate(data_dir.as_path())
                .context("Failed to read in seed file")?;
            let env_config = env::Testnet::get_config();

            if receive_monero_address.network != env_config.monero_network {
                bail!(
                    "Given monero address is on network {:?}, expected address on network {:?}",
                    receive_monero_address.network,
                    env_config.monero_network
                )
            }

            let bitcoin_wallet = init_bitcoin_wallet(
                electrum_rpc_url,
                &seed,
                data_dir.clone(),
                env_config,
                bitcoin_target_block,
            )
            .await?;
            let (monero_wallet, _process) =
                init_monero_wallet(data_dir, monero_daemon_host, env_config).await?;
            let bitcoin_wallet = Arc::new(bitcoin_wallet);

            let mut swarm = swarm::bob(&seed, alice_peer_id, tor_socks5_port).await?;
            swarm
                .behaviour_mut()
                .add_address(alice_peer_id, alice_multiaddr);

            let swap_id = Uuid::new_v4();
            let (event_loop, mut event_loop_handle) =
                EventLoop::new(swap_id, swarm, alice_peer_id, bitcoin_wallet.clone())?;
            let event_loop = tokio::spawn(event_loop.run());

            let max_givable = || bitcoin_wallet.max_giveable(TxLock::script_size());
            let (send_bitcoin, fees) = determine_btc_to_swap(
                event_loop_handle.request_quote(),
                max_givable().await?,
                bitcoin_wallet.new_address(),
                || bitcoin_wallet.balance(),
                max_givable,
                || bitcoin_wallet.sync(),
            )
            .await?;

            info!("Swapping {} with {} fees", send_bitcoin, fees);

            db.insert_peer_id(swap_id, alice_peer_id).await?;

            let swap = Swap::new(
                db,
                swap_id,
                bitcoin_wallet,
                Arc::new(monero_wallet),
                env_config,
                event_loop_handle,
                receive_monero_address,
                send_bitcoin,
            );

            tokio::select! {
                result = event_loop => {
                    result
                        .context("EventLoop panicked")?;
                },
                result = bob::run(swap) => {
                    result.context("Failed to complete swap")?;
                }
            }
        }
        Command::History => {
            let data_dir = data.0;

            let db = Database::open(data_dir.join("database").as_path())
                .context("Failed to open database")?;

            let mut table = Table::new();

            table.add_row(row!["SWAP ID", "STATE"]);

            for (swap_id, state) in db.all_bob()? {
                table.add_row(row![swap_id, state]);
            }

            // Print the table to stdout
            table.printstd();
        }
        Command::Resume {
            swap_id,
            alice_multiaddr,
            monero_params:
                MoneroParams {
                    receive_monero_address,
                    monero_daemon_host,
                },
            electrum_rpc_url,
            tor_socks5_port,
            bitcoin_target_block,
        } => {
            let data_dir = data.0;
            cli::tracing::init(debug, data_dir.join("logs"), swap_id)?;
            let db = Database::open(data_dir.join("database").as_path())
                .context("Failed to open database")?;
            let seed = Seed::from_file_or_generate(data_dir.as_path())
                .context("Failed to read in seed file")?;
            let env_config = env::Testnet::get_config();

            if receive_monero_address.network != env_config.monero_network {
                bail!("The given monero address is on network {:?}, expected address of network {:?}.", receive_monero_address.network, env_config.monero_network)
            }

            let bitcoin_wallet = init_bitcoin_wallet(
                electrum_rpc_url,
                &seed,
                data_dir.clone(),
                env_config,
                bitcoin_target_block,
            )
            .await?;
            let (monero_wallet, _process) =
                init_monero_wallet(data_dir, monero_daemon_host, env_config).await?;
            let bitcoin_wallet = Arc::new(bitcoin_wallet);

            let alice_peer_id = db.get_peer_id(swap_id)?;

            let mut swarm = swarm::bob(&seed, alice_peer_id, tor_socks5_port).await?;
            let bob_peer_id = swarm.local_peer_id();
            tracing::debug!("Our peer-id: {}", bob_peer_id);
            swarm
                .behaviour_mut()
                .add_address(alice_peer_id, alice_multiaddr);

            let (event_loop, event_loop_handle) =
                EventLoop::new(swap_id, swarm, alice_peer_id, bitcoin_wallet.clone())?;
            let handle = tokio::spawn(event_loop.run());

            let swap = Swap::from_db(
                db,
                swap_id,
                bitcoin_wallet,
                Arc::new(monero_wallet),
                env_config,
                event_loop_handle,
                receive_monero_address,
            )?;

            tokio::select! {
                event_loop_result = handle => {
                    event_loop_result?;
                },
                swap_result = bob::run(swap) => {
                    swap_result?;
                }
            }
        }
        Command::Cancel {
            swap_id,
            force,
            electrum_rpc_url,
            bitcoin_target_block,
        } => {
            let data_dir = data.0;
            cli::tracing::init(debug, data_dir.join("logs"), swap_id)?;
            let db = Database::open(data_dir.join("database").as_path())
                .context("Failed to open database")?;
            let seed = Seed::from_file_or_generate(data_dir.as_path())
                .context("Failed to read in seed file")?;
            let env_config = env::Testnet::get_config();

            let bitcoin_wallet = init_bitcoin_wallet(
                electrum_rpc_url,
                &seed,
                data_dir,
                env_config,
                bitcoin_target_block,
            )
            .await?;

            let cancel = bob::cancel(swap_id, Arc::new(bitcoin_wallet), db, force).await?;

            match cancel {
                Ok((txid, _)) => {
                    debug!("Cancel transaction successfully published with id {}", txid)
                }
                Err(bob::cancel::Error::CancelTimelockNotExpiredYet) => error!(
                    "The Cancel Transaction cannot be published yet, because the timelock has not expired. Please try again later"
                ),
            }
        }
        Command::Refund {
            swap_id,
            force,
            electrum_rpc_url,
            bitcoin_target_block,
        } => {
            let data_dir = data.0;
            cli::tracing::init(debug, data_dir.join("logs"), swap_id)?;
            let db = Database::open(data_dir.join("database").as_path())
                .context("Failed to open database")?;
            let seed = Seed::from_file_or_generate(data_dir.as_path())
                .context("Failed to read in seed file")?;
            let env_config = env::Testnet::get_config();

            let bitcoin_wallet = init_bitcoin_wallet(
                electrum_rpc_url,
                &seed,
                data_dir,
                env_config,
                bitcoin_target_block,
            )
            .await?;

            bob::refund(swap_id, Arc::new(bitcoin_wallet), db, force).await??;
        }
    };
    Ok(())
}

async fn init_bitcoin_wallet(
    electrum_rpc_url: Url,
    seed: &Seed,
    data_dir: PathBuf,
    env_config: Config,
    bitcoin_target_block: usize,
) -> Result<bitcoin::Wallet> {
    let wallet_dir = data_dir.join("wallet");

    let wallet = bitcoin::Wallet::new(
        electrum_rpc_url.clone(),
        &wallet_dir,
        seed.derive_extended_private_key(env_config.bitcoin_network)?,
        env_config,
        bitcoin_target_block,
    )
    .await
    .context("Failed to initialize Bitcoin wallet")?;

    wallet.sync().await?;

    Ok(wallet)
}

async fn init_monero_wallet(
    data_dir: PathBuf,
    monero_daemon_host: String,
    env_config: Config,
) -> Result<(monero::Wallet, monero::WalletRpcProcess)> {
    let network = env_config.monero_network;

    const MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME: &str = "swap-tool-blockchain-monitoring-wallet";

    let monero_wallet_rpc = monero::WalletRpc::new(data_dir.join("monero")).await?;

    let monero_wallet_rpc_process = monero_wallet_rpc
        .run(network, monero_daemon_host.as_str())
        .await?;

    let monero_wallet = monero::Wallet::open_or_create(
        monero_wallet_rpc_process.endpoint(),
        MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME.to_string(),
        env_config,
    )
    .await?;

    Ok((monero_wallet, monero_wallet_rpc_process))
}

async fn determine_btc_to_swap<FB, TB, FMG, TMG, FS, TS>(
    bid_quote: impl Future<Output = Result<BidQuote>>,
    mut current_maximum_giveable: bitcoin::Amount,
    get_new_address: impl Future<Output = Result<bitcoin::Address>>,
    balance: FB,
    max_giveable: FMG,
    sync: FS,
) -> Result<(bitcoin::Amount, bitcoin::Amount)>
where
    TB: Future<Output = Result<bitcoin::Amount>>,
    FB: Fn() -> TB,
    TMG: Future<Output = Result<bitcoin::Amount>>,
    FMG: Fn() -> TMG,
    TS: Future<Output = Result<()>>,
    FS: Fn() -> TS,
{
    debug!("Requesting quote");
    let bid_quote = bid_quote.await?;
    info!("Received quote: 1 XMR ~ {}", bid_quote.price);

    let max_giveable = if current_maximum_giveable == bitcoin::Amount::ZERO
        || current_maximum_giveable < bid_quote.min_quantity
    {
        let deposit_address = get_new_address.await?;
        let minimum_amount = bid_quote.min_quantity;
        let maximum_amount = bid_quote.max_quantity;

        info!(
            %deposit_address,
            %current_maximum_giveable,
            %minimum_amount,
            %maximum_amount,
            "Please deposit BTC you want to swap to",
        );

        loop {
            sync().await?;

            let new_max_givable = max_giveable().await?;

            if new_max_givable != current_maximum_giveable {
                current_maximum_giveable = new_max_givable;

                let new_balance = balance().await?;
                tracing::info!(
                    %new_balance,
                    %current_maximum_giveable,
                    "Received BTC",
                );

                if current_maximum_giveable >= bid_quote.min_quantity {
                    break;
                } else {
                    tracing::info!(
                        %minimum_amount,
                        %deposit_address,
                        "Please deposit more, not enough BTC to trigger swap with",
                    );
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        current_maximum_giveable
    } else {
        current_maximum_giveable
    };

    let balance = balance().await?;
    let fees = balance - max_giveable;

    let max_accepted = bid_quote.max_quantity;

    let btc_swap_amount = min(max_giveable, max_accepted);

    Ok((btc_swap_amount, fees))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::determine_btc_to_swap;
    use ::bitcoin::Amount;
    use tracing::subscriber;

    #[tokio::test]
    async fn given_no_balance_and_transfers_less_than_max_swaps_max_giveable() {
        let _guard = subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());

        let (amount, fees) = determine_btc_to_swap(
            async { Ok(quote_with_max(0.01)) },
            Amount::ZERO,
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.001)?) },
            || async { Ok(Amount::from_btc(0.0009)?) },
            || async { Ok(()) },
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.0009).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees))
    }

    #[tokio::test]
    async fn given_no_balance_and_transfers_more_then_swaps_max_quantity_from_quote() {
        let _guard = subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());

        let (amount, fees) = determine_btc_to_swap(
            async { Ok(quote_with_max(0.01)) },
            Amount::ZERO,
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.1001)?) },
            || async { Ok(Amount::from_btc(0.1)?) },
            || async { Ok(()) },
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.01).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees))
    }

    #[tokio::test]
    async fn given_initial_balance_below_max_quantity_swaps_max_givable() {
        let _guard = subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());

        let (amount, fees) = determine_btc_to_swap(
            async { Ok(quote_with_max(0.01)) },
            Amount::from_btc(0.0049).unwrap(),
            async { panic!("should not request new address when initial balance is > 0") },
            || async { Ok(Amount::from_btc(0.005)?) },
            || async { panic!("should not wait for deposit when initial balance > 0") },
            || async { Ok(()) },
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.0049).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees))
    }

    #[tokio::test]
    async fn given_initial_balance_above_max_quantity_swaps_max_quantity() {
        let _guard = subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());

        let (amount, fees) = determine_btc_to_swap(
            async { Ok(quote_with_max(0.01)) },
            Amount::from_btc(0.1).unwrap(),
            async { panic!("should not request new address when initial balance is > 0") },
            || async { Ok(Amount::from_btc(0.1001)?) },
            || async { panic!("should not wait for deposit when initial balance > 0") },
            || async { Ok(()) },
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.01).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees))
    }

    #[tokio::test]
    async fn given_no_initial_balance_then_min_wait_for_sufficient_deposit() {
        let _guard = subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());

        let (amount, fees) = determine_btc_to_swap(
            async { Ok(quote_with_min(0.01)) },
            Amount::ZERO,
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.0101)?) },
            || async { Ok(Amount::from_btc(0.01)?) },
            || async { Ok(()) },
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.01).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees))
    }

    #[tokio::test]
    async fn given_balance_less_then_min_wait_for_sufficient_deposit() {
        let _guard = subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());

        let (amount, fees) = determine_btc_to_swap(
            async { Ok(quote_with_min(0.01)) },
            Amount::from_btc(0.0001).unwrap(),
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.0101)?) },
            || async { Ok(Amount::from_btc(0.01)?) },
            || async { Ok(()) },
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.01).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees))
    }

    #[tokio::test]
    async fn given_no_initial_balance_and_transfers_less_than_min_keep_waiting() {
        let _guard = subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());

        let error = tokio::time::timeout(
            Duration::from_secs(1),
            determine_btc_to_swap(
                async { Ok(quote_with_min(0.1)) },
                Amount::ZERO,
                get_dummy_address(),
                || async { Ok(Amount::from_btc(0.0101)?) },
                || async { Ok(Amount::from_btc(0.01)?) },
                || async { Ok(()) },
            ),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, tokio::time::error::Elapsed { .. }))
    }

    fn quote_with_max(btc: f64) -> BidQuote {
        BidQuote {
            price: Amount::from_btc(0.001).unwrap(),
            max_quantity: Amount::from_btc(btc).unwrap(),
            min_quantity: Amount::ZERO,
        }
    }

    fn quote_with_min(btc: f64) -> BidQuote {
        BidQuote {
            price: Amount::from_btc(0.001).unwrap(),
            max_quantity: Amount::max_value(),
            min_quantity: Amount::from_btc(btc).unwrap(),
        }
    }

    async fn get_dummy_address() -> Result<bitcoin::Address> {
        Ok("1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6".parse()?)
    }
}
