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
use libp2p::core::multiaddr::Protocol;
use libp2p::core::Multiaddr;
use libp2p::Swarm;
use prettytable::{row, Table};
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use structopt::clap;
use structopt::clap::ErrorKind;
use swap::asb::command::{parse_args, Arguments, Command};
use swap::asb::config::{
    initial_setup, query_user_for_initial_config, read_config, Config, ConfigNotInitialized,
};
use swap::database::Database;
use swap::monero::Amount;
use swap::network::swarm;
use swap::protocol::alice;
use swap::protocol::alice::event_loop::KrakenRate;
use swap::protocol::alice::{redeem, run, EventLoop};
use swap::seed::Seed;
use swap::tor::AuthenticatedClient;
use swap::{asb, bitcoin, kraken, monero, tor};
use tracing::{debug, info, warn};
use tracing_subscriber::filter::LevelFilter;

#[macro_use]
extern crate prettytable;

const DEFAULT_WALLET_NAME: &str = "asb-wallet";

#[tokio::main]
async fn main() -> Result<()> {
    let Arguments {
        testnet,
        json,
        config_path,
        env_config,
        cmd,
    } = match parse_args(env::args_os()) {
        Ok(args) => args,
        Err(e) => {
            if let Some(clap_err) = e.downcast_ref::<clap::Error>() {
                match clap_err.kind {
                    ErrorKind::HelpDisplayed | ErrorKind::VersionDisplayed => {
                        println!("{}", clap_err.message);
                        std::process::exit(0);
                    }
                    _ => {
                        bail!(e);
                    }
                }
            }
            bail!(e);
        }
    };

    asb::tracing::init(LevelFilter::DEBUG, json).expect("initialize tracing");

    let config = match read_config(config_path.clone())? {
        Ok(config) => config,
        Err(ConfigNotInitialized {}) => {
            initial_setup(config_path.clone(), query_user_for_initial_config(testnet)?)?;
            read_config(config_path)?.expect("after initial setup config can be read")
        }
    };

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

    info!(
        db_folder = %config.data.dir.display(),
        "Database and Seed will be stored in",
    );

    let db_path = config.data.dir.join("database");

    let db = Database::open(config.data.dir.join(db_path).as_path())
        .context("Could not open database")?;

    let seed =
        Seed::from_file_or_generate(&config.data.dir).expect("Could not retrieve/initialize seed");

    match cmd {
        Command::Start { resume_only } => {
            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;

            let monero_wallet = init_monero_wallet(&config, env_config).await?;

            let bitcoin_balance = bitcoin_wallet.balance().await?;
            info!(%bitcoin_balance, "Initialized Bitcoin wallet");

            let monero_balance = monero_wallet.get_balance().await?;
            if monero_balance == Amount::ZERO {
                let monero_address = monero_wallet.get_main_address();
                warn!(
                    %monero_address,
                    "The Monero balance is 0, make sure to deposit funds at",
                )
            } else {
                info!(%monero_balance, "Initialized Monero wallet");
            }

            let kraken_price_updates = kraken::connect()?;

            // setup Tor hidden services
            let tor_client =
                tor::Client::new(config.tor.socks5_port).with_control_port(config.tor.control_port);
            let _ac = match tor_client.assert_tor_running().await {
                Ok(_) => {
                    tracing::info!("Tor found. Setting up hidden service");
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

            let current_balance = monero_wallet.get_balance().await?;
            let lock_fee = monero_wallet.static_tx_fee_estimate();
            let kraken_rate = KrakenRate::new(config.maker.ask_spread, kraken_price_updates);
            let mut swarm = swarm::alice(
                &seed,
                current_balance,
                lock_fee,
                config.maker.min_buy_btc,
                config.maker.max_buy_btc,
                kraken_rate.clone(),
                resume_only,
                env_config,
            )?;

            for listen in config.network.listen {
                Swarm::listen_on(&mut swarm, listen.clone())
                    .with_context(|| format!("Failed to listen on network interface {}", listen))?;
            }

            let (event_loop, mut swap_receiver) = EventLoop::new(
                swarm,
                env_config,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                Arc::new(db),
                kraken_rate.clone(),
                config.maker.min_buy_btc,
                config.maker.max_buy_btc,
            )
            .unwrap();

            tokio::spawn(async move {
                while let Some(swap) = swap_receiver.recv().await {
                    let rate = kraken_rate.clone();
                    tokio::spawn(async move {
                        let swap_id = swap.swap_id;
                        match run(swap, rate).await {
                            Ok(state) => {
                                tracing::debug!(%swap_id, %state, "Swap finished with state")
                            }
                            Err(error) => {
                                tracing::error!(%swap_id, "Swap failed. Error {:#}", error)
                            }
                        }
                    });
                }
            });

            info!(peer_id = %event_loop.peer_id(), "Our peer-id");

            event_loop.run().await;
        }
        Command::History => {
            let mut table = Table::new();

            table.add_row(row!["SWAP ID", "STATE"]);

            for (swap_id, state) in db.all_alice()? {
                table.add_row(row![swap_id, state]);
            }

            // Print the table to stdout
            table.printstd();
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

            let psbt = bitcoin_wallet.send_to_address(address, amount).await?;
            let signed_tx = bitcoin_wallet.sign_and_finalize(psbt).await?;

            bitcoin_wallet.broadcast(signed_tx, "withdraw").await?;
        }
        Command::Balance => {
            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;
            let monero_wallet = init_monero_wallet(&config, env_config).await?;

            let bitcoin_balance = bitcoin_wallet.balance().await?;
            let monero_balance = monero_wallet.get_balance().await?;

            tracing::info!(
                %bitcoin_balance,
                %monero_balance,
                "Current balance");
        }
        Command::Cancel { swap_id, force } => {
            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;

            let (txid, _) =
                alice::cancel(swap_id, Arc::new(bitcoin_wallet), Arc::new(db), force).await??;

            tracing::info!("Cancel transaction successfully published with id {}", txid);
        }
        Command::Refund { swap_id, force } => {
            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;
            let monero_wallet = init_monero_wallet(&config, env_config).await?;

            alice::refund(
                swap_id,
                Arc::new(bitcoin_wallet),
                Arc::new(monero_wallet),
                Arc::new(db),
                force,
            )
            .await??;

            tracing::info!("Monero successfully refunded");
        }
        Command::Punish { swap_id, force } => {
            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;

            let (txid, _) =
                alice::punish(swap_id, Arc::new(bitcoin_wallet), Arc::new(db), force).await??;

            tracing::info!("Punish transaction successfully published with id {}", txid);
        }
        Command::SafelyAbort { swap_id } => {
            alice::safely_abort(swap_id, Arc::new(db)).await?;

            tracing::info!("Swap safely aborted");
        }
        Command::Redeem {
            swap_id,
            force,
            do_not_await_finality,
        } => {
            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;

            let (txid, _) = alice::redeem(
                swap_id,
                Arc::new(bitcoin_wallet),
                Arc::new(db),
                force,
                redeem::Finality::from_bool(do_not_await_finality),
            )
            .await?;

            tracing::info!("Redeem transaction successfully published with id {}", txid);
        }
    }

    Ok(())
}

async fn init_bitcoin_wallet(
    config: &Config,
    seed: &Seed,
    env_config: swap::env::Config,
) -> Result<bitcoin::Wallet> {
    debug!("Opening Bitcoin wallet");
    let wallet_dir = config.data.dir.join("wallet");

    let wallet = bitcoin::Wallet::new(
        config.bitcoin.electrum_rpc_url.clone(),
        &wallet_dir,
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
    debug!("Opening Monero wallet");
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
        tracing::info!(%onion_address);
    });

    Ok(ac)
}
