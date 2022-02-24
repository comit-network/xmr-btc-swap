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
use comfy_table::Table;
use qrcode::render::unicode;
use qrcode::QrCode;
use std::cmp::min;
use std::convert::TryInto;
use std::env;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use swap::bitcoin::TxLock;
use swap::cli::command::{parse_args_and_apply_defaults, Arguments, Command, ParseResult};
use swap::cli::{list_sellers, EventLoop, SellerStatus};
use swap::database::open_db;
use swap::env::Config;
use swap::libp2p_ext::MultiAddrExt;
use swap::network::quote::{BidQuote, ZeroQuoteReceived};
use swap::network::swarm;
use swap::protocol::bob;
use swap::protocol::bob::{BobState, Swap};
use swap::seed::Seed;
use swap::{bitcoin, cli, monero};
use url::Url;
use uuid::Uuid;

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
            seller,
            bitcoin_electrum_rpc_url,
            bitcoin_target_block,
            bitcoin_change_address,
            monero_receive_address,
            monero_daemon_address,
            tor_socks5_port,
        } => {
            let swap_id = Uuid::new_v4();

            cli::tracing::init(debug, json, data_dir.join("logs"), Some(swap_id))?;
            let db = open_db(data_dir.join("sqlite")).await?;
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
            let seller_peer_id = seller
                .extract_peer_id()
                .context("Seller address must contain peer ID")?;
            db.insert_address(seller_peer_id, seller.clone()).await?;

            let behaviour = cli::Behaviour::new(seller_peer_id, env_config, bitcoin_wallet.clone());
            let mut swarm =
                swarm::cli(seed.derive_libp2p_identity(), tor_socks5_port, behaviour).await?;
            swarm.behaviour_mut().add_address(seller_peer_id, seller);

            tracing::debug!(peer_id = %swarm.local_peer_id(), "Network layer initialized");

            let (event_loop, mut event_loop_handle) =
                EventLoop::new(swap_id, swarm, seller_peer_id, env_config)?;
            let event_loop = tokio::spawn(event_loop.run());

            let max_givable = || bitcoin_wallet.max_giveable(TxLock::script_size());
            let (amount, fees) = match determine_btc_to_swap(
                json,
                event_loop_handle.request_quote(),
                bitcoin_wallet.new_address(),
                || bitcoin_wallet.balance(),
                max_givable,
                || bitcoin_wallet.sync(),
            )
            .await
            {
                Ok(val) => val,
                Err(error) => match error.downcast::<ZeroQuoteReceived>() {
                    Ok(_) => {
                        bail!("Seller's XMR balance is currently too low to initiate a swap, please try again later")
                    }
                    Err(other) => bail!(other),
                },
            };

            tracing::info!(%amount, %fees,  "Determined swap amount");

            db.insert_peer_id(swap_id, seller_peer_id).await?;
            db.insert_monero_address(swap_id, monero_receive_address)
                .await?;

            let swap = Swap::new(
                db,
                swap_id,
                bitcoin_wallet,
                Arc::new(monero_wallet),
                env_config,
                event_loop_handle,
                monero_receive_address,
                bitcoin_change_address,
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
            cli::tracing::init(debug, json, data_dir.join("logs"), None)?;

            let db = open_db(data_dir.join("sqlite")).await?;
            let swaps = db.all().await?;

            if json {
                for (swap_id, state) in swaps {
                    let state: BobState = state.try_into()?;
                    tracing::info!(swap_id=%swap_id.to_string(), state=%state.to_string(), "Read swap state from database");
                }
            } else {
                let mut table = Table::new();

                table.set_header(vec!["SWAP ID", "STATE"]);

                for (swap_id, state) in swaps {
                    let state: BobState = state.try_into()?;
                    table.add_row(vec![swap_id.to_string(), state.to_string()]);
                }

                println!("{}", table);
            }
        }
        Command::Config => {
            cli::tracing::init(debug, json, data_dir.join("logs"), None)?;

            tracing::info!(path=%data_dir.display(), "Data directory");
            tracing::info!(path=%format!("{}/logs", data_dir.display()), "Log files directory");
            tracing::info!(path=%format!("{}/sqlite", data_dir.display()), "Sqlite file location");
            tracing::info!(path=%format!("{}/seed.pem", data_dir.display()), "Seed file location");
            tracing::info!(path=%format!("{}/monero", data_dir.display()), "Monero-wallet-rpc directory");
            tracing::info!(path=%format!("{}/wallet", data_dir.display()), "Internal bitcoin wallet directory");
        }
        Command::WithdrawBtc {
            bitcoin_electrum_rpc_url,
            bitcoin_target_block,
            amount,
            address,
        } => {
            cli::tracing::init(debug, json, data_dir.join("logs"), None)?;
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

            let amount = match amount {
                Some(amount) => amount,
                None => {
                    bitcoin_wallet
                        .max_giveable(address.script_pubkey().len())
                        .await?
                }
            };

            let psbt = bitcoin_wallet
                .send_to_address(address, amount, None)
                .await?;
            let signed_tx = bitcoin_wallet.sign_and_finalize(psbt).await?;

            bitcoin_wallet.broadcast(signed_tx, "withdraw").await?;
        }

        Command::Balance {
            bitcoin_electrum_rpc_url,
            bitcoin_target_block,
        } => {
            cli::tracing::init(debug, json, data_dir.join("logs"), None)?;
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

            let bitcoin_balance = bitcoin_wallet.balance().await?;
            tracing::info!(
                balance = %bitcoin_balance,
                "Checked Bitcoin balance",
            );
        }
        Command::Resume {
            swap_id,
            bitcoin_electrum_rpc_url,
            bitcoin_target_block,
            monero_daemon_address,
            tor_socks5_port,
        } => {
            cli::tracing::init(debug, json, data_dir.join("logs"), Some(swap_id))?;
            let db = open_db(data_dir.join("sqlite")).await?;
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

            let seller_peer_id = db.get_peer_id(swap_id).await?;
            let seller_addresses = db.get_addresses(seller_peer_id).await?;

            let behaviour = cli::Behaviour::new(seller_peer_id, env_config, bitcoin_wallet.clone());
            let mut swarm =
                swarm::cli(seed.derive_libp2p_identity(), tor_socks5_port, behaviour).await?;
            let our_peer_id = swarm.local_peer_id();
            tracing::debug!(peer_id = %our_peer_id, "Network layer initialized");

            for seller_address in seller_addresses {
                swarm
                    .behaviour_mut()
                    .add_address(seller_peer_id, seller_address);
            }

            let (event_loop, event_loop_handle) =
                EventLoop::new(swap_id, swarm, seller_peer_id, env_config)?;
            let handle = tokio::spawn(event_loop.run());

            let monero_receive_address = db.get_monero_address(swap_id).await?;
            let swap = Swap::from_db(
                db,
                swap_id,
                bitcoin_wallet,
                Arc::new(monero_wallet),
                env_config,
                event_loop_handle,
                monero_receive_address,
            )
            .await?;

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
            bitcoin_electrum_rpc_url,
            bitcoin_target_block,
        } => {
            cli::tracing::init(debug, json, data_dir.join("logs"), Some(swap_id))?;
            let db = open_db(data_dir.join("sqlite")).await?;
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

            let (txid, _) = cli::cancel(swap_id, Arc::new(bitcoin_wallet), db).await?;
            tracing::debug!("Cancel transaction successfully published with id {}", txid);
        }
        Command::Refund {
            swap_id,
            bitcoin_electrum_rpc_url,
            bitcoin_target_block,
        } => {
            cli::tracing::init(debug, json, data_dir.join("logs"), Some(swap_id))?;
            let db = open_db(data_dir.join("sqlite")).await?;
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

            cli::refund(swap_id, Arc::new(bitcoin_wallet), db).await?;
        }
        Command::ListSellers {
            rendezvous_point,
            namespace,
            tor_socks5_port,
        } => {
            let rendezvous_node_peer_id = rendezvous_point
                .extract_peer_id()
                .context("Rendezvous node address must contain peer ID")?;

            cli::tracing::init(debug, json, data_dir.join("logs"), None)?;
            let seed = Seed::from_file_or_generate(data_dir.as_path())
                .context("Failed to read in seed file")?;
            let identity = seed.derive_libp2p_identity();

            let sellers = list_sellers(
                rendezvous_node_peer_id,
                rendezvous_point,
                namespace,
                tor_socks5_port,
                identity,
            )
            .await?;

            if json {
                for seller in sellers {
                    match seller.status {
                        SellerStatus::Online(quote) => {
                            tracing::info!(
                                price = %quote.price.to_string(),
                                min_quantity = %quote.min_quantity.to_string(),
                                max_quantity = %quote.max_quantity.to_string(),
                                status = "Online",
                                address = %seller.multiaddr.to_string(),
                                "Fetched peer status"
                            );
                        }
                        SellerStatus::Unreachable => {
                            tracing::info!(
                                status = "Unreachable",
                                address = %seller.multiaddr.to_string(),
                                "Fetched peer status"
                            );
                        }
                    }
                }
            } else {
                let mut table = Table::new();

                table.set_header(vec![
                    "PRICE",
                    "MIN_QUANTITY",
                    "MAX_QUANTITY",
                    "STATUS",
                    "ADDRESS",
                ]);

                for seller in sellers {
                    let row = match seller.status {
                        SellerStatus::Online(quote) => {
                            vec![
                                quote.price.to_string(),
                                quote.min_quantity.to_string(),
                                quote.max_quantity.to_string(),
                                "Online".to_owned(),
                                seller.multiaddr.to_string(),
                            ]
                        }
                        SellerStatus::Unreachable => {
                            vec![
                                "???".to_owned(),
                                "???".to_owned(),
                                "???".to_owned(),
                                "Unreachable".to_owned(),
                                seller.multiaddr.to_string(),
                            ]
                        }
                    };

                    table.add_row(row);
                }

                println!("{}", table);
            }
        }
        Command::ExportBitcoinWallet {
            bitcoin_electrum_rpc_url,
            bitcoin_target_block,
        } => {
            cli::tracing::init(debug, json, data_dir.join("logs"), None)?;

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
            let wallet_export = bitcoin_wallet.wallet_export("cli").await?;
            tracing::info!(descriptor=%wallet_export.to_string(), "Exported bitcoin wallet");
        }
        Command::MoneroRecovery { swap_id } => {
            let db = open_db(data_dir.join("sqlite")).await?;

            let swap_state: BobState = db.get_state(swap_id).await?.try_into()?;

            match swap_state {
                BobState::Started { .. }
                | BobState::SwapSetupCompleted(_)
                | BobState::BtcLocked { .. }
                | BobState::XmrLockProofReceived { .. }
                | BobState::XmrLocked(_)
                | BobState::EncSigSent(_)
                | BobState::CancelTimelockExpired(_)
                | BobState::BtcCancelled(_)
                | BobState::BtcRefunded(_)
                | BobState::BtcPunished { .. }
                | BobState::SafelyAborted
                | BobState::XmrRedeemed { .. } => {
                    bail!("Cannot print monero recovery information in state {}, only possible for BtcRedeemed", swap_state)
                }
                BobState::BtcRedeemed(state5) => {
                    let (spend_key, view_key) = state5.xmr_keys();

                    let address = monero::Address::standard(
                        env_config.monero_network,
                        monero::PublicKey::from_private_key(&spend_key),
                        monero::PublicKey::from(view_key.public()),
                    );
                    tracing::info!("Wallet address: {}", address.to_string());

                    let view_key = serde_json::to_string(&view_key)?;
                    println!("View key: {}", view_key);

                    println!("Spend key: {}", spend_key);
                }
            }
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
    tracing::debug!("Requesting quote");
    let bid_quote = bid_quote.await?;

    if bid_quote.max_quantity == bitcoin::Amount::ZERO {
        bail!(ZeroQuoteReceived)
    }

    tracing::info!(
        price = %bid_quote.price,
        minimum_amount = %bid_quote.min_quantity,
        maximum_amount = %bid_quote.max_quantity,
        "Received quote",
    );

    let mut max_giveable = max_giveable_fn().await?;

    if max_giveable == bitcoin::Amount::ZERO || max_giveable < bid_quote.min_quantity {
        let deposit_address = get_new_address.await?;
        let minimum_amount = bid_quote.min_quantity;
        let maximum_amount = bid_quote.max_quantity;

        if !json {
            eprintln!("{}", qr_code(&deposit_address)?);
        }

        loop {
            tracing::info!(
                %deposit_address,
                %max_giveable,
                %minimum_amount,
                %maximum_amount,
                "Waiting for Bitcoin deposit",
            );

            max_giveable = loop {
                sync().await?;
                let new_max_givable = max_giveable_fn().await?;

                if new_max_givable > max_giveable {
                    break new_max_givable;
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            };

            let new_balance = balance().await?;
            tracing::info!(%new_balance, %max_giveable, "Received Bitcoin");

            if max_giveable < bid_quote.min_quantity {
                tracing::info!("Deposited amount is less than `min_quantity`");
                continue;
            }

            break;
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
    use swap::tracing_ext::capture_logs;
    use tracing::level_filters::LevelFilter;

    #[tokio::test]
    async fn given_no_balance_and_transfers_less_than_max_swaps_max_giveable() {
        let writer = capture_logs(LevelFilter::INFO);
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

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.00000000 BTC maximum_amount=0.01000000 BTC
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 max_giveable=0.00000000 BTC minimum_amount=0.00000000 BTC maximum_amount=0.01000000 BTC
 INFO swap: Received Bitcoin new_balance=0.00100000 BTC max_giveable=0.00090000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_no_balance_and_transfers_more_then_swaps_max_quantity_from_quote() {
        let writer = capture_logs(LevelFilter::INFO);
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

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.00000000 BTC maximum_amount=0.01000000 BTC
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 max_giveable=0.00000000 BTC minimum_amount=0.00000000 BTC maximum_amount=0.01000000 BTC
 INFO swap: Received Bitcoin new_balance=0.10010000 BTC max_giveable=0.10000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_initial_balance_below_max_quantity_swaps_max_givable() {
        let writer = capture_logs(LevelFilter::INFO);
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

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.00000000 BTC maximum_amount=0.01000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_initial_balance_above_max_quantity_swaps_max_quantity() {
        let writer = capture_logs(LevelFilter::INFO);
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

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.00000000 BTC maximum_amount=0.01000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_no_initial_balance_then_min_wait_for_sufficient_deposit() {
        let writer = capture_logs(LevelFilter::INFO);
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

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.01000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 max_giveable=0.00000000 BTC minimum_amount=0.01000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Received Bitcoin new_balance=0.01010000 BTC max_giveable=0.01000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_balance_less_then_min_wait_for_sufficient_deposit() {
        let writer = capture_logs(LevelFilter::INFO);
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

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.01000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 max_giveable=0.00010000 BTC minimum_amount=0.01000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Received Bitcoin new_balance=0.01010000 BTC max_giveable=0.01000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_no_initial_balance_and_transfers_less_than_min_keep_waiting() {
        let writer = capture_logs(LevelFilter::INFO);
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

        assert!(matches!(error, tokio::time::error::Elapsed { .. }));
        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.10000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 max_giveable=0.00000000 BTC minimum_amount=0.10000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Received Bitcoin new_balance=0.01010000 BTC max_giveable=0.01000000 BTC
 INFO swap: Deposited amount is less than `min_quantity`
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 max_giveable=0.01000000 BTC minimum_amount=0.10000000 BTC maximum_amount=184467440737.09551615 BTC
"
        );
    }

    #[tokio::test]
    async fn given_longer_delay_until_deposit_should_not_spam_user() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::from_btc(0.2).unwrap(),
        ])));

        tokio::time::timeout(
            Duration::from_secs(10),
            determine_btc_to_swap(
                true,
                async { Ok(quote_with_min(0.1)) },
                get_dummy_address(),
                || async { Ok(Amount::from_btc(0.21)?) },
                || async {
                    let mut result = givable.lock().unwrap();

                    result.give()
                },
                || async { Ok(()) },
            ),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.10000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 max_giveable=0.00000000 BTC minimum_amount=0.10000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Received Bitcoin new_balance=0.21000000 BTC max_giveable=0.20000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_bid_quote_max_amount_0_return_errorq() {
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::from_btc(0.0001).unwrap(),
            Amount::from_btc(0.01).unwrap(),
        ])));

        let determination_error = determine_btc_to_swap(
            true,
            async { Ok(quote_with_max(0.00)) },
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.0101)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
            || async { Ok(()) },
        )
        .await
        .err()
        .unwrap()
        .to_string();

        assert_eq!("Received quote of 0", determination_error);
    }

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
