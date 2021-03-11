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
use std::sync::Arc;
use std::time::Duration;
use structopt::StructOpt;
use swap::bitcoin::{Amount, TxLock};
use swap::cli::command::{AliceConnectParams, Arguments, Command, MoneroParams};
use swap::cli::config::{read_config, Config};
use swap::database::Database;
use swap::execution_params::{ExecutionParams, GetExecutionParams};
use swap::network::quote::BidQuote;
use swap::protocol::bob;
use swap::protocol::bob::{Builder, EventLoop};
use swap::seed::Seed;
use swap::{bitcoin, execution_params, monero};
use tracing::{debug, error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;
use uuid::Uuid;

#[macro_use]
extern crate prettytable;

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

    let config = match args.file_path {
        Some(config_path) => read_config(config_path)??,
        None => Config::testnet(),
    };

    let db = Database::open(config.data.dir.join("database").as_path())
        .context("Failed to open database")?;

    let seed =
        Seed::from_file_or_generate(&config.data.dir).context("Failed to read in seed file")?;

    // hardcode to testnet/stagenet
    let bitcoin_network = bitcoin::Network::Testnet;
    let monero_network = monero::Network::Stagenet;
    let execution_params = execution_params::Testnet::get_execution_params();

    match args.cmd {
        Command::BuyXmr {
            connect_params:
                AliceConnectParams {
                    peer_id: alice_peer_id,
                    multiaddr: alice_addr,
                },
            monero_params:
                MoneroParams {
                    receive_monero_address,
                    monero_daemon_host,
                },
        } => {
            if receive_monero_address.network != monero_network {
                bail!(
                    "Given monero address is on network {:?}, expected address on network {:?}",
                    receive_monero_address.network,
                    monero_network
                )
            }

            let bitcoin_wallet = init_bitcoin_wallet(bitcoin_network, &config, seed).await?;
            let (monero_wallet, _process) = init_monero_wallet(
                monero_network,
                &config,
                monero_daemon_host,
                execution_params,
            )
            .await?;
            let bitcoin_wallet = Arc::new(bitcoin_wallet);
            let (event_loop, mut event_loop_handle) = EventLoop::new(
                &seed.derive_libp2p_identity(),
                alice_peer_id,
                alice_addr,
                bitcoin_wallet.clone(),
            )?;
            let handle = tokio::spawn(event_loop.run());

            let send_bitcoin = determine_btc_to_swap(
                event_loop_handle.request_quote(),
                bitcoin_wallet.balance(),
                bitcoin_wallet.new_address(),
                async {
                    while bitcoin_wallet.balance().await? == Amount::ZERO {
                        bitcoin_wallet.sync().await?;

                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }

                    bitcoin_wallet.balance().await
                },
                bitcoin_wallet.max_giveable(TxLock::script_size()),
            )
            .await?;

            let swap = Builder::new(
                db,
                Uuid::new_v4(),
                bitcoin_wallet.clone(),
                Arc::new(monero_wallet),
                execution_params,
                event_loop_handle,
                receive_monero_address,
            )
            .with_init_params(send_bitcoin)
            .build()?;

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
            swap_id,
            connect_params:
                AliceConnectParams {
                    peer_id: alice_peer_id,
                    multiaddr: alice_addr,
                },
            monero_params:
                MoneroParams {
                    receive_monero_address,
                    monero_daemon_host,
                },
        } => {
            if receive_monero_address.network != monero_network {
                bail!("The given monero address is on network {:?}, expected address of network {:?}.", receive_monero_address.network, monero_network)
            }

            let bitcoin_wallet = init_bitcoin_wallet(bitcoin_network, &config, seed).await?;
            let (monero_wallet, _process) = init_monero_wallet(
                monero_network,
                &config,
                monero_daemon_host,
                execution_params,
            )
            .await?;
            let bitcoin_wallet = Arc::new(bitcoin_wallet);

            let (event_loop, event_loop_handle) = EventLoop::new(
                &seed.derive_libp2p_identity(),
                alice_peer_id,
                alice_addr,
                bitcoin_wallet.clone(),
            )?;
            let handle = tokio::spawn(event_loop.run());

            let swap = Builder::new(
                db,
                swap_id,
                bitcoin_wallet.clone(),
                Arc::new(monero_wallet),
                execution_params,
                event_loop_handle,
                receive_monero_address,
            )
            .build()?;

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
            let bitcoin_wallet = init_bitcoin_wallet(bitcoin_network, &config, seed).await?;

            let resume_state = db.get_state(swap_id)?.try_into_bob()?.into();
            let cancel =
                bob::cancel(swap_id, resume_state, Arc::new(bitcoin_wallet), db, force).await?;

            match cancel {
                Ok((txid, _)) => {
                    debug!("Cancel transaction successfully published with id {}", txid)
                }
                Err(bob::cancel::Error::CancelTimelockNotExpiredYet) => error!(
                    "The Cancel Transaction cannot be published yet, \
                        because the timelock has not expired. Please try again later."
                ),
                Err(bob::cancel::Error::CancelTxAlreadyPublished) => {
                    warn!("The Cancel Transaction has already been published.")
                }
            }
        }
        Command::Refund { swap_id, force } => {
            let bitcoin_wallet = init_bitcoin_wallet(bitcoin_network, &config, seed).await?;

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
    network: bitcoin::Network,
    config: &Config,
    seed: Seed,
) -> Result<bitcoin::Wallet> {
    let wallet_dir = config.data.dir.join("wallet");

    let wallet = bitcoin::Wallet::new(
        config.bitcoin.electrum_rpc_url.clone(),
        config.bitcoin.electrum_http_url.clone(),
        network,
        &wallet_dir,
        seed.derive_extended_private_key(network)?,
    )
    .await
    .context("Failed to initialize Bitcoin wallet")?;

    wallet.sync().await?;

    Ok(wallet)
}

async fn init_monero_wallet(
    monero_network: monero::Network,
    config: &Config,
    monero_daemon_host: String,
    execution_params: ExecutionParams,
) -> Result<(monero::Wallet, monero::WalletRpcProcess)> {
    const MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME: &str = "swap-tool-blockchain-monitoring-wallet";

    let monero_wallet_rpc = monero::WalletRpc::new(config.data.dir.join("monero")).await?;

    let monero_wallet_rpc_process = monero_wallet_rpc
        .run(monero_network, monero_daemon_host.as_str())
        .await?;

    let monero_wallet = monero::Wallet::new(
        monero_wallet_rpc_process.endpoint(),
        monero_network,
        MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME.to_string(),
        execution_params.monero_avg_block_time,
    );

    monero_wallet.open_or_create().await?;

    let _test_wallet_connection = monero_wallet
        .block_height()
        .await
        .context("Failed to validate connection to monero-wallet-rpc")?;

    Ok((monero_wallet, monero_wallet_rpc_process))
}

async fn determine_btc_to_swap(
    request_quote: impl Future<Output = Result<BidQuote>>,
    initial_balance: impl Future<Output = Result<bitcoin::Amount>>,
    get_new_address: impl Future<Output = Result<bitcoin::Address>>,
    wait_for_deposit: impl Future<Output = Result<bitcoin::Amount>>,
    max_giveable: impl Future<Output = Result<bitcoin::Amount>>,
) -> Result<bitcoin::Amount> {
    debug!("Requesting quote");

    let bid_quote = request_quote.await.context("Failed to request quote")?;

    info!("Received quote: 1 XMR ~ {}", bid_quote.price);

    // TODO: Also wait for more funds if balance < dust
    let initial_balance = initial_balance.await?;

    if initial_balance == Amount::ZERO {
        info!(
            "Please deposit the BTC you want to swap to {} (max {})",
            get_new_address.await?,
            bid_quote.max_quantity
        );

        let new_balance = wait_for_deposit
            .await
            .context("Failed to wait for Bitcoin deposit")?;

        info!("Received {}", new_balance);
    } else {
        info!("Found {} in wallet", initial_balance);
    }

    let max_giveable = max_giveable
        .await
        .context("Failed to compute max 'giveable' Bitcoin amount")?;
    let max_accepted = bid_quote.max_quantity;

    if max_giveable > max_accepted {
        info!(
            "Max giveable amount {} exceeds max accepted amount {}!",
            max_giveable, max_accepted
        );
    }

    Ok(min(max_giveable, max_accepted))
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

        let amount = determine_btc_to_swap(
            async { Ok(quote_with_max(0.01)) },
            async { Ok(Amount::ZERO) },
            get_dummy_address(),
            async { Ok(Amount::from_btc(0.0001)?) },
            async { Ok(Amount::from_btc(0.00009)?) },
        )
        .await
        .unwrap();

        assert_eq!(amount, Amount::from_btc(0.00009).unwrap())
    }

    #[tokio::test]
    async fn given_no_balance_and_transfers_more_then_swaps_max_quantity_from_quote() {
        let _guard = subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());

        let amount = determine_btc_to_swap(
            async { Ok(quote_with_max(0.01)) },
            async { Ok(Amount::ZERO) },
            get_dummy_address(),
            async { Ok(Amount::from_btc(0.1)?) },
            async { Ok(Amount::from_btc(0.09)?) },
        )
        .await
        .unwrap();

        assert_eq!(amount, Amount::from_btc(0.01).unwrap())
    }

    #[tokio::test]
    async fn given_initial_balance_below_max_quantity_swaps_max_givable() {
        let _guard = subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());

        let amount = determine_btc_to_swap(
            async { Ok(quote_with_max(0.01)) },
            async { Ok(Amount::from_btc(0.005)?) },
            async { panic!("should not request new address when initial balance is > 0") },
            async { panic!("should not wait for deposit when initial balance > 0") },
            async { Ok(Amount::from_btc(0.0049)?) },
        )
        .await
        .unwrap();

        assert_eq!(amount, Amount::from_btc(0.0049).unwrap())
    }

    #[tokio::test]
    async fn given_initial_balance_above_max_quantity_swaps_max_quantity() {
        let _guard = subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());

        let amount = determine_btc_to_swap(
            async { Ok(quote_with_max(0.01)) },
            async { Ok(Amount::from_btc(0.1)?) },
            async { panic!("should not request new address when initial balance is > 0") },
            async { panic!("should not wait for deposit when initial balance > 0") },
            async { Ok(Amount::from_btc(0.09)?) },
        )
        .await
        .unwrap();

        assert_eq!(amount, Amount::from_btc(0.01).unwrap())
    }

    fn quote_with_max(btc: f64) -> BidQuote {
        BidQuote {
            price: Amount::from_btc(0.001).unwrap(),
            max_quantity: Amount::from_btc(btc).unwrap(),
        }
    }

    async fn get_dummy_address() -> Result<bitcoin::Address> {
        Ok("1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6".parse()?)
    }
}
