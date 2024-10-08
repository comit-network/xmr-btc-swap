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
use libp2p::core::multiaddr::Protocol;
use libp2p::core::Multiaddr;
use libp2p::swarm::AddressScore;
use libp2p::Swarm;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use std::convert::TryInto;
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use structopt::clap;
use structopt::clap::ErrorKind;
use swap::asb::command::{parse_args, Arguments, Command};
use swap::asb::config::{
    initial_setup, query_user_for_initial_config, read_config, Config, ConfigNotInitialized,
};
use swap::asb::{cancel, punish, redeem, refund, safely_abort, EventLoop, Finality, KrakenRate};
use swap::common::tracing_util::Format;
use swap::common::{self, check_latest_version, get_logs};
use swap::database::{open_db, AccessMode};
use swap::network::rendezvous::XmrBtcNamespace;
use swap::network::swarm;
use swap::protocol::alice::swap::is_complete;
use swap::protocol::alice::{run, AliceState};
use swap::protocol::State;
use swap::seed::Seed;
use swap::tor::AuthenticatedClient;
use swap::{bitcoin, kraken, monero, tor};
use tracing_subscriber::filter::LevelFilter;

const DEFAULT_WALLET_NAME: &str = "asb-wallet";

#[tokio::main]
async fn main() -> Result<()> {
    // parse cli arguments
    let Arguments {
        testnet,
        json,
        config_path,
        env_config,
        cmd,
    } = match parse_args(env::args_os()) {
        Ok(args) => args,
        Err(e) => {
            // make sure to display the clap error message it exists
            if let Some(clap_err) = e.downcast_ref::<clap::Error>() {
                if let ErrorKind::HelpDisplayed | ErrorKind::VersionDisplayed = clap_err.kind {
                    println!("{}", clap_err.message);
                    std::process::exit(0);
                }
            }
            bail!(e);
        }
    };

    // warn if we're not on the latest version
    if let Err(e) = check_latest_version(env!("CARGO_PKG_VERSION")).await {
        eprintln!("{}", e);
    }

    // read config from the specified path
    let config = match read_config(config_path.clone())? {
        Ok(config) => config,
        Err(ConfigNotInitialized {}) => {
            initial_setup(config_path.clone(), query_user_for_initial_config(testnet)?)?;
            read_config(config_path)?.expect("after initial setup config can be read")
        }
    };

    // initialize tracing
    let format = if json { Format::Json } else { Format::Raw };
    let log_dir = config.data.dir.join("logs");
    common::tracing_util::init(LevelFilter::DEBUG, format, log_dir).expect("initialize tracing");

    // check for conflicting env / config values
    if config.monero.network != env_config.monero_network {
        bail!(format!(
            "Expected monero network in config file to be {:?} but was {:?}",
            env_config.monero_network, config.monero.network
        ));
    }
    if config.bitcoin.network != env_config.bitcoin_network {
        bail!(format!(
            "Expected bitcoin network in config file to be {:?} but was {:?}",
            env_config.bitcoin_network, config.bitcoin.network
        ));
    }

    let seed =
        Seed::from_file_or_generate(&config.data.dir).expect("Could not retrieve/initialize seed");

    match cmd {
        Command::Start { resume_only } => {
            let db = open_db(config.data.dir.join("sqlite"), AccessMode::ReadWrite).await?;

            // check and warn for duplicate rendezvous points
            let mut rendezvous_addrs = config.network.rendezvous_point.clone();
            let prev_len = rendezvous_addrs.len();
            rendezvous_addrs.sort();
            rendezvous_addrs.dedup();
            let new_len = rendezvous_addrs.len();

            if new_len < prev_len {
                tracing::warn!(
                    "`rendezvous_point` config has {} duplicate entries, they are being ignored.",
                    prev_len - new_len
                );
            }

            // initialize monero wallet
            let monero_wallet = init_monero_wallet(&config, env_config).await?;
            let monero_address = monero_wallet.get_main_address();
            tracing::info!(%monero_address, "Monero wallet address");

            // check monero balance
            let monero = monero_wallet.get_balance().await?;
            match (monero.balance, monero.unlocked_balance) {
                (0, _) => {
                    tracing::warn!(
                        %monero_address,
                        "The Monero balance is 0, make sure to deposit funds at",
                    )
                }
                (total, 0) => {
                    let total = monero::Amount::from_piconero(total);
                    tracing::warn!(
                        %total,
                        "Unlocked Monero balance is 0, total balance is",
                    )
                }
                (total, unlocked) => {
                    let total = monero::Amount::from_piconero(total);
                    let unlocked = monero::Amount::from_piconero(unlocked);
                    tracing::info!(%total, %unlocked, "Monero wallet balance");
                }
            }

            // init bitcoin wallet
            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;
            let bitcoin_balance = bitcoin_wallet.balance().await?;
            tracing::info!(%bitcoin_balance, "Bitcoin wallet balance");

            let kraken_price_updates = kraken::connect(config.maker.price_ticker_ws_url.clone())?;

            // setup Tor hidden services
            let tor_client =
                tor::Client::new(config.tor.socks5_port).with_control_port(config.tor.control_port);
            let _ac = match tor_client.assert_tor_running().await {
                Ok(_) => {
                    tracing::info!("Setting up Tor hidden service");
                    let ac =
                        register_tor_services(config.network.clone().listen, tor_client, &seed)
                            .await?;
                    Some(ac)
                }
                Err(_) => {
                    tracing::warn!("Tor not found. Running on clear net");
                    None
                }
            };

            let kraken_rate = KrakenRate::new(config.maker.ask_spread, kraken_price_updates);
            let namespace = XmrBtcNamespace::from_is_testnet(testnet);

            let mut swarm = swarm::asb(
                &seed,
                config.maker.min_buy_btc,
                config.maker.max_buy_btc,
                kraken_rate.clone(),
                resume_only,
                env_config,
                namespace,
                &rendezvous_addrs,
            )?;

            for listen in config.network.listen.clone() {
                Swarm::listen_on(&mut swarm, listen.clone())
                    .with_context(|| format!("Failed to listen on network interface {}", listen))?;
            }

            tracing::info!(peer_id = %swarm.local_peer_id(), "Network layer initialized");

            for external_address in config.network.external_addresses {
                let _ = Swarm::add_external_address(
                    &mut swarm,
                    external_address,
                    AddressScore::Infinite,
                );
            }

            let (event_loop, mut swap_receiver) = EventLoop::new(
                swarm,
                env_config,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                db,
                kraken_rate.clone(),
                config.maker.min_buy_btc,
                config.maker.max_buy_btc,
                config.maker.external_bitcoin_redeem_address,
            )
            .unwrap();

            tokio::spawn(async move {
                while let Some(swap) = swap_receiver.recv().await {
                    let rate = kraken_rate.clone();
                    tokio::spawn(async move {
                        let swap_id = swap.swap_id;
                        match run(swap, rate).await {
                            Ok(state) => {
                                tracing::debug!(%swap_id, final_state=%state, "Swap completed")
                            }
                            Err(error) => {
                                tracing::error!(%swap_id, "Swap failed: {:#}", error)
                            }
                        }
                    });
                }
            });

            event_loop.run().await;
        }
        Command::History { only_unfinished } => {
            let db = open_db(config.data.dir.join("sqlite"), AccessMode::ReadOnly).await?;
            let mut table: Table = Table::new();

            table.set_header(vec![
                "Swap ID",
                "Start Date",
                "State",
                "BTC Amount",
                "XMR Amount",
                "Exchange Rate",
                "Trading Partner Peer ID",
                "Completed",
            ]);

            let all_swaps = db.all().await?;
            for (swap_id, state) in all_swaps {
                if let Err(e) = async {
                    let latest_state: AliceState = state.try_into()?;
                    let is_completed = is_complete(&latest_state);

                    if only_unfinished && is_completed {
                        return Ok::<_, anyhow::Error>(());
                    }

                    let all_states = db.get_states(swap_id).await?;
                    let state3 = all_states
                        .iter()
                        .find_map(|s| match s {
                            State::Alice(AliceState::BtcLockTransactionSeen { state3 }) => {
                                Some(state3)
                            }
                            _ => None,
                        })
                        .context("Failed to get \"BtcLockTransactionSeen\" state")?;

                    let swap_start_date = db.get_swap_start_date(swap_id).await?;
                    let peer_id = db.get_peer_id(swap_id).await?;

                    let exchange_rate = Decimal::from_f64(state3.btc.to_btc())
                        .ok_or_else(|| anyhow::anyhow!("Failed to convert BTC amount to Decimal"))?
                        .checked_div(state3.xmr.as_xmr())
                        .ok_or_else(|| anyhow::anyhow!("Division by zero or overflow"))?;
                    let exchange_rate = format!("{} XMR/BTC", exchange_rate.round_dp(8));

                    if json {
                        tracing::info!(
                            swap_id = %swap_id,
                            swap_start_date = %swap_start_date,
                            latest_state = %latest_state,
                            btc_amount = %state3.btc,
                            xmr_amount = %state3.xmr,
                            exchange_rate = %exchange_rate,
                            trading_partner_peer_id = %peer_id,
                            completed = is_completed,
                            "Found swap in database"
                        );
                    } else {
                        table.add_row(vec![
                            swap_id.to_string(),
                            swap_start_date.to_string(),
                            latest_state.to_string(),
                            state3.btc.to_string(),
                            state3.xmr.to_string(),
                            exchange_rate,
                            peer_id.to_string(),
                            is_completed.to_string(),
                        ]);
                    }

                    Ok::<_, anyhow::Error>(())
                }
                .await
                {
                    tracing::error!(swap_id = %swap_id, error = %e, "Failed to get swap details");
                }
            }

            if !json {
                println!("{}", table);
            }
        }
        Command::Config => {
            let config_json = serde_json::to_string_pretty(&config)?;
            println!("{}", config_json);
        }
        Command::Logs {
            logs_dir,
            swap_id,
            redact,
        } => {
            let dir = logs_dir.unwrap_or(config.data.dir.join("logs"));

            let log_messages = get_logs(dir, swap_id, redact).await?;

            for msg in log_messages {
                println!("{msg}");
            }
        }
        Command::WithdrawBtc { amount, address } => {
            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;

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
        Command::Balance => {
            let monero_wallet = init_monero_wallet(&config, env_config).await?;
            let monero_balance = monero_wallet.get_balance().await?;
            tracing::info!(%monero_balance);

            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;
            let bitcoin_balance = bitcoin_wallet.balance().await?;
            tracing::info!(%bitcoin_balance);
            tracing::info!(%bitcoin_balance, %monero_balance, "Current balance");
        }
        Command::Cancel { swap_id } => {
            let db = open_db(config.data.dir.join("sqlite"), AccessMode::ReadWrite).await?;

            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;

            let (txid, _) = cancel(swap_id, Arc::new(bitcoin_wallet), db).await?;

            tracing::info!("Cancel transaction successfully published with id {}", txid);
        }
        Command::Refund { swap_id } => {
            let db = open_db(config.data.dir.join("sqlite"), AccessMode::ReadWrite).await?;

            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;
            let monero_wallet = init_monero_wallet(&config, env_config).await?;

            refund(
                swap_id,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                db,
            )
            .await?;

            tracing::info!("Monero successfully refunded");
        }
        Command::Punish { swap_id } => {
            let db = open_db(config.data.dir.join("sqlite"), AccessMode::ReadWrite).await?;

            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;

            let (txid, _) = punish(swap_id, Arc::new(bitcoin_wallet), db).await?;

            tracing::info!("Punish transaction successfully published with id {}", txid);
        }
        Command::SafelyAbort { swap_id } => {
            let db = open_db(config.data.dir.join("sqlite"), AccessMode::ReadWrite).await?;

            safely_abort(swap_id, db).await?;

            tracing::info!("Swap safely aborted");
        }
        Command::Redeem {
            swap_id,
            do_not_await_finality,
        } => {
            let db = open_db(config.data.dir.join("sqlite"), AccessMode::ReadWrite).await?;

            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;

            let (txid, _) = redeem(
                swap_id,
                Arc::new(bitcoin_wallet),
                db,
                Finality::from_bool(do_not_await_finality),
            )
            .await?;

            tracing::info!("Redeem transaction successfully published with id {}", txid);
        }
        Command::ExportBitcoinWallet => {
            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;
            let wallet_export = bitcoin_wallet.wallet_export("asb").await?;
            println!("{}", wallet_export.to_string())
        }
    }

    Ok(())
}

async fn init_bitcoin_wallet(
    config: &Config,
    seed: &Seed,
    env_config: swap::env::Config,
) -> Result<bitcoin::Wallet> {
    tracing::debug!("Opening Bitcoin wallet");
    let data_dir = &config.data.dir;
    let wallet = bitcoin::Wallet::new(
        config.bitcoin.electrum_rpc_url.clone(),
        data_dir,
        seed.derive_extended_private_key(env_config.bitcoin_network)?,
        env_config,
        config.bitcoin.target_block,
    )
    .await
    .context("Failed to initialize Bitcoin wallet")?;

    wallet.sync().await?;

    Ok(wallet)
}

async fn init_monero_wallet(
    config: &Config,
    env_config: swap::env::Config,
) -> Result<monero::Wallet> {
    tracing::debug!("Opening Monero wallet");
    let wallet = monero::Wallet::open_or_create(
        config.monero.wallet_rpc_url.clone(),
        DEFAULT_WALLET_NAME.to_string(),
        env_config,
    )
    .await?;

    Ok(wallet)
}

/// Registers a hidden service for each network.
/// Note: Once ac goes out of scope, the services will be de-registered.
async fn register_tor_services(
    networks: Vec<Multiaddr>,
    tor_client: tor::Client,
    seed: &Seed,
) -> Result<AuthenticatedClient> {
    let mut ac = tor_client.into_authenticated_client().await?;

    let hidden_services_details = networks
        .iter()
        .flat_map(|network| {
            network.iter().map(|protocol| match protocol {
                Protocol::Tcp(port) => Some((
                    port,
                    SocketAddr::new(IpAddr::from(Ipv4Addr::new(127, 0, 0, 1)), port),
                )),
                _ => {
                    // We only care for Tcp for now.
                    None
                }
            })
        })
        .flatten()
        .collect::<Vec<_>>();

    let key = seed.derive_torv3_key();

    ac.add_services(&hidden_services_details, &key).await?;

    let onion_address = key
        .public()
        .get_onion_address()
        .get_address_without_dot_onion();

    hidden_services_details.iter().for_each(|(port, _)| {
        let onion_address = format!("/onion3/{}:{}", onion_address, port);
        tracing::info!(%onion_address, "Successfully created hidden service");
    });

    Ok(ac)
}
