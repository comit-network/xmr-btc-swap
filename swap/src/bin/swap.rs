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
use qrcode::render::unicode;
use qrcode::QrCode;
use std::cmp::min;
use std::env;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use swap::bitcoin::TxLock;
use swap::cli::command::{parse_args_and_apply_defaults, Arguments, Command, ParseResult};
use swap::database::Database;
use swap::env::Config;
use swap::network::quote::BidQuote;
use swap::network::swarm;
use swap::protocol::bob;
use swap::protocol::bob::{EventLoop, Swap};
use swap::seed::Seed;
use swap::{bitcoin, cli, monero};
use tracing::{debug, error, info, warn};
use url::Url;
use uuid::Uuid;

#[macro_use]
extern crate prettytable;

#[tokio::main]
async fn main() -> Result<()> {
    let Arguments {
        env_config,
        data_dir,
        debug,
        json,
        cmd,
    } = match parse_args_and_apply_defaults(env::args_os())? {
        ParseResult::Arguments(args) => args,
        ParseResult::PrintAndExitZero { message } => {
            println!("{}", message);
            std::process::exit(0);
        }
    };

    match cmd {
        Command::BuyXmr {
            seller_peer_id,
            seller_addr,
            bitcoin_electrum_rpc_url,
            bitcoin_target_block,
            monero_receive_address,
            monero_daemon_address,
            tor_socks5_port,
        } => {
            let swap_id = Uuid::new_v4();

            cli::tracing::init(debug, json, data_dir.join("logs"), swap_id)?;
            let db = Database::open(data_dir.join("database").as_path())
                .context("Failed to open database")?;
            let seed = Seed::from_file_or_generate(data_dir.as_path())
                .context("Failed to read in seed file")?;

            let bitcoin_wallet = init_bitcoin_wallet(
                bitcoin_electrum_rpc_url,
                &seed,
                data_dir.clone(),
                env_config,
                bitcoin_target_block,
            )
            .await?;
            let (monero_wallet, _process) =
                init_monero_wallet(data_dir, monero_daemon_address, env_config).await?;
            let bitcoin_wallet = Arc::new(bitcoin_wallet);

            let mut swarm = swarm::cli(&seed, seller_peer_id, tor_socks5_port).await?;
            swarm
                .behaviour_mut()
                .add_address(seller_peer_id, seller_addr);

            tracing::debug!(peer_id = %swarm.local_peer_id(), "Network layer initialized");

            let (event_loop, mut event_loop_handle) = EventLoop::new(
                swap_id,
                swarm,
                seller_peer_id,
                bitcoin_wallet.clone(),
                env_config,
            )?;
            let event_loop = tokio::spawn(event_loop.run());

            let max_givable = || bitcoin_wallet.max_giveable(TxLock::script_size());
            let (amount, fees) = determine_btc_to_swap(
                json,
                event_loop_handle.request_quote(),
                bitcoin_wallet.new_address(),
                || bitcoin_wallet.balance(),
                max_givable,
                || bitcoin_wallet.sync(),
            )
            .await?;

            info!(%amount, %fees, %swap_id,  "Swapping");

            db.insert_peer_id(swap_id, seller_peer_id).await?;

            let swap = Swap::new(
                db,
                swap_id,
                bitcoin_wallet,
                Arc::new(monero_wallet),
                env_config,
                event_loop_handle,
                monero_receive_address,
                amount,
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
            seller_addr,
            bitcoin_electrum_rpc_url,
            bitcoin_target_block,
            monero_receive_address,
            monero_daemon_address,
            tor_socks5_port,
        } => {
            cli::tracing::init(debug, json, data_dir.join("logs"), swap_id)?;
            let db = Database::open(data_dir.join("database").as_path())
                .context("Failed to open database")?;
            let seed = Seed::from_file_or_generate(data_dir.as_path())
                .context("Failed to read in seed file")?;

            if monero_receive_address.network != env_config.monero_network {
                bail!("The given monero address is on network {:?}, expected address of network {:?}.", monero_receive_address.network, env_config.monero_network)
            }

            let bitcoin_wallet = init_bitcoin_wallet(
                bitcoin_electrum_rpc_url,
                &seed,
                data_dir.clone(),
                env_config,
                bitcoin_target_block,
            )
            .await?;
            let (monero_wallet, _process) =
                init_monero_wallet(data_dir, monero_daemon_address, env_config).await?;
            let bitcoin_wallet = Arc::new(bitcoin_wallet);

            let seller_peer_id = db.get_peer_id(swap_id)?;

            let mut swarm = swarm::cli(&seed, seller_peer_id, tor_socks5_port).await?;
            let our_peer_id = swarm.local_peer_id();
            tracing::debug!(peer_id = %our_peer_id, "Initializing network module");
            swarm
                .behaviour_mut()
                .add_address(seller_peer_id, seller_addr);

            let (event_loop, event_loop_handle) = EventLoop::new(
                swap_id,
                swarm,
                seller_peer_id,
                bitcoin_wallet.clone(),
                env_config,
            )?;
            let handle = tokio::spawn(event_loop.run());

            let swap = Swap::from_db(
                db,
                swap_id,
                bitcoin_wallet,
                Arc::new(monero_wallet),
                env_config,
                event_loop_handle,
                monero_receive_address,
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
            bitcoin_electrum_rpc_url,
            bitcoin_target_block,
        } => {
            cli::tracing::init(debug, json, data_dir.join("logs"), swap_id)?;
            let db = Database::open(data_dir.join("database").as_path())
                .context("Failed to open database")?;
            let seed = Seed::from_file_or_generate(data_dir.as_path())
                .context("Failed to read in seed file")?;

            let bitcoin_wallet = init_bitcoin_wallet(
                bitcoin_electrum_rpc_url,
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
            bitcoin_electrum_rpc_url,
            bitcoin_target_block,
        } => {
            cli::tracing::init(debug, json, data_dir.join("logs"), swap_id)?;
            let db = Database::open(data_dir.join("database").as_path())
                .context("Failed to open database")?;
            let seed = Seed::from_file_or_generate(data_dir.as_path())
                .context("Failed to read in seed file")?;

            let bitcoin_wallet = init_bitcoin_wallet(
                bitcoin_electrum_rpc_url,
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
    monero_daemon_address: String,
    env_config: Config,
) -> Result<(monero::Wallet, monero::WalletRpcProcess)> {
    let network = env_config.monero_network;

    const MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME: &str = "swap-tool-blockchain-monitoring-wallet";

    let monero_wallet_rpc = monero::WalletRpc::new(data_dir.join("monero")).await?;

    let monero_wallet_rpc_process = monero_wallet_rpc
        .run(network, monero_daemon_address.as_str())
        .await?;

    let monero_wallet = monero::Wallet::open_or_create(
        monero_wallet_rpc_process.endpoint(),
        MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME.to_string(),
        env_config,
    )
    .await?;

    Ok((monero_wallet, monero_wallet_rpc_process))
}

fn qr_code(value: &impl ToString) -> Result<String> {
    let code = QrCode::new(value.to_string())?;
    let qr_code = code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();
    Ok(qr_code)
}

async fn determine_btc_to_swap<FB, TB, FMG, TMG, FS, TS>(
    json: bool,
    bid_quote: impl Future<Output = Result<BidQuote>>,
    get_new_address: impl Future<Output = Result<bitcoin::Address>>,
    balance: FB,
    max_giveable_fn: FMG,
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
    info!(
        price = %bid_quote.price,
        minimum_amount = %bid_quote.min_quantity,
        maximum_amount = %bid_quote.max_quantity,
        "Received quote: 1 XMR ~ ",
    );

    let mut max_giveable = max_giveable_fn().await?;

    if max_giveable == bitcoin::Amount::ZERO || max_giveable < bid_quote.min_quantity {
        let deposit_address = get_new_address.await?;
        let minimum_amount = bid_quote.min_quantity;
        let maximum_amount = bid_quote.max_quantity;

        if !json {
            eprintln!("{}", qr_code(&deposit_address)?);
        }

        info!(
            %deposit_address,
            %max_giveable,
            %minimum_amount,
            %maximum_amount,
            "Please deposit BTC you want to swap to",
        );

        loop {
            sync().await?;

            let new_max_givable = max_giveable_fn().await?;

            if new_max_givable != max_giveable {
                max_giveable = new_max_givable;

                let new_balance = balance().await?;
                tracing::info!(
                    %new_balance,
                    %max_giveable,
                    "Received BTC",
                );

                if max_giveable >= bid_quote.min_quantity {
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
    use std::sync::Mutex;
    use tracing::subscriber;

    struct MaxGiveable {
        amounts: Vec<Amount>,
        call_counter: usize,
    }

    impl MaxGiveable {
        fn new(amounts: Vec<Amount>) -> Self {
            Self {
                amounts,
                call_counter: 0,
            }
        }
        fn give(&mut self) -> Result<Amount> {
            let amount = self
                .amounts
                .get(self.call_counter)
                .ok_or_else(|| anyhow::anyhow!("No more balances available"))?;
            self.call_counter += 1;
            Ok(*amount)
        }
    }

    #[tokio::test]
    async fn given_no_balance_and_transfers_less_than_max_swaps_max_giveable() {
        let _guard = subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::ZERO,
            Amount::from_btc(0.0009).unwrap(),
        ])));

        let (amount, fees) = determine_btc_to_swap(
            true,
            async { Ok(quote_with_max(0.01)) },
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.001)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
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
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::ZERO,
            Amount::from_btc(0.1).unwrap(),
        ])));

        let (amount, fees) = determine_btc_to_swap(
            true,
            async { Ok(quote_with_max(0.01)) },
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.1001)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
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
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::from_btc(0.0049).unwrap(),
            Amount::from_btc(99.9).unwrap(),
        ])));

        let (amount, fees) = determine_btc_to_swap(
            true,
            async { Ok(quote_with_max(0.01)) },
            async { panic!("should not request new address when initial balance  is > 0") },
            || async { Ok(Amount::from_btc(0.005)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
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
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::from_btc(0.1).unwrap(),
            Amount::from_btc(99.9).unwrap(),
        ])));

        let (amount, fees) = determine_btc_to_swap(
            true,
            async { Ok(quote_with_max(0.01)) },
            async { panic!("should not request new address when initial balance is > 0") },
            || async { Ok(Amount::from_btc(0.1001)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
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
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::ZERO,
            Amount::from_btc(0.01).unwrap(),
        ])));

        let (amount, fees) = determine_btc_to_swap(
            true,
            async { Ok(quote_with_min(0.01)) },
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.0101)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
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
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::from_btc(0.0001).unwrap(),
            Amount::from_btc(0.01).unwrap(),
        ])));

        let (amount, fees) = determine_btc_to_swap(
            true,
            async { Ok(quote_with_min(0.01)) },
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.0101)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
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
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::ZERO,
            Amount::from_btc(0.01).unwrap(),
            Amount::from_btc(0.01).unwrap(),
            Amount::from_btc(0.01).unwrap(),
            Amount::from_btc(0.01).unwrap(),
        ])));

        let error = tokio::time::timeout(
            Duration::from_secs(1),
            determine_btc_to_swap(
                true,
                async { Ok(quote_with_min(0.1)) },
                get_dummy_address(),
                || async { Ok(Amount::from_btc(0.0101)?) },
                || async {
                    let mut result = givable.lock().unwrap();
                    result.give()
                },
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
