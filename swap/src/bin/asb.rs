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
use libp2p::Swarm;
use monero_sys::Daemon;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use std::convert::TryInto;
use std::env;
use std::sync::Arc;
use structopt::clap;
use structopt::clap::ErrorKind;
use swap::asb::command::{parse_args, Arguments, Command};
use swap::asb::config::{
    initial_setup, query_user_for_initial_config, read_config, Config, ConfigNotInitialized,
};
use swap::asb::{cancel, punish, redeem, refund, safely_abort, EventLoop, Finality, KrakenRate};
use swap::common::tor::init_tor_client;
use swap::common::tracing_util::Format;
use swap::common::{self, get_logs, warn_if_outdated};
use swap::database::{open_db, AccessMode};
use swap::network::rendezvous::XmrBtcNamespace;
use swap::network::swarm;
use swap::protocol::alice::swap::is_complete;
use swap::protocol::alice::{run, AliceState};
use swap::protocol::{Database, State};
use swap::seed::Seed;
use swap::{bitcoin, kraken, monero};
use tracing_subscriber::filter::LevelFilter;
use uuid::Uuid;

const DEFAULT_WALLET_NAME: &str = "asb-wallet";

trait IntoDaemon {
    fn into_daemon(self) -> Result<Daemon>;
}

impl IntoDaemon for url::Url {
    fn into_daemon(self) -> Result<Daemon> {
        let address = self.to_string();
        let ssl = self.scheme() == "https";

        Ok(Daemon { address, ssl })
    }
}

impl IntoDaemon for monero_rpc_pool::ServerInfo {
    fn into_daemon(self) -> Result<Daemon> {
        let address = format!("http://{}:{}", self.host, self.port);
        let ssl = false; // Pool server always uses HTTP locally

        Ok(Daemon { address, ssl })
    }
}

#[tokio::main]
pub async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install default rustls provider");

    let Arguments {
        testnet,
        json,
        trace,
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

    // Check in the background if there's a new version available
    tokio::spawn(async move { warn_if_outdated(env!("CARGO_PKG_VERSION")).await });

    // Read config from the specified path
    let config = match read_config(config_path.clone())? {
        Ok(config) => config,
        Err(ConfigNotInitialized {}) => {
            initial_setup(config_path.clone(), query_user_for_initial_config(testnet)?)?;
            read_config(config_path.clone())?.expect("after initial setup config can be read")
        }
    };

    // Initialize tracing
    let format = if json { Format::Json } else { Format::Raw };
    let log_dir = config.data.dir.join("logs");
    common::tracing_util::init(LevelFilter::DEBUG, format, log_dir, None, trace)
        .expect("initialize tracing");
    tracing::info!(
        binary = "asb",
        version = env!("VERGEN_GIT_DESCRIBE"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        "Setting up context"
    );

    // Check for conflicting env / config values
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

    let db_file = config.data.dir.join("sqlite");

    match cmd {
        Command::Start { resume_only } => {
            let db = open_db(db_file, AccessMode::ReadWrite, None).await?;

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

            // Initialize Monero wallet
            let monero_wallet = init_monero_wallet(&config, env_config).await?;
            let monero_address = monero_wallet.main_wallet().await.main_address().await;
            tracing::info!(%monero_address, "Monero wallet address");

            // Check Monero balance
            let wallet = monero_wallet.main_wallet().await;

            let total = wallet.total_balance().await.as_pico();
            let unlocked = wallet.unlocked_balance().await.as_pico();

            match (total, unlocked) {
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

            // Initialize Bitcoin wallet
            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;
            let bitcoin_balance = bitcoin_wallet.balance().await?;
            tracing::info!(%bitcoin_balance, "Bitcoin wallet balance");

            // Connect to Kraken
            let kraken_price_updates = kraken::connect(config.maker.price_ticker_ws_url.clone())?;

            let kraken_rate = KrakenRate::new(config.maker.ask_spread, kraken_price_updates);
            let namespace = XmrBtcNamespace::from_is_testnet(testnet);

            // Initialize Tor client
            let tor_client = init_tor_client(&config.data.dir, None).await?.into();

            let (mut swarm, onion_addresses) = swarm::asb(
                &seed,
                config.maker.min_buy_btc,
                config.maker.max_buy_btc,
                kraken_rate.clone(),
                resume_only,
                env_config,
                namespace,
                &rendezvous_addrs,
                tor_client,
                config.tor.register_hidden_service,
                config.tor.hidden_service_num_intro_points,
            )?;

            for listen in config.network.listen.clone() {
                if let Err(e) = Swarm::listen_on(&mut swarm, listen.clone()) {
                    tracing::warn!("Failed to listen on network interface {}: {}. Consider removing it from the config.", listen, e);
                }
            }

            for onion_address in onion_addresses {
                match swarm.listen_on(onion_address.clone()) {
                    Err(e) => {
                        tracing::warn!(
                            "Failed to listen on onion address {}: {}",
                            onion_address,
                            e
                        );
                    }
                    _ => {
                        swarm.add_external_address(onion_address);
                    }
                }
            }

            tracing::info!(peer_id = %swarm.local_peer_id(), "Network layer initialized");

            for external_address in config.network.external_addresses {
                swarm.add_external_address(external_address);
            }

            let (event_loop, mut swap_receiver) = EventLoop::new(
                swarm,
                env_config,
                Arc::new(bitcoin_wallet),
                monero_wallet.clone(),
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
            let db = open_db(db_file, AccessMode::ReadOnly, None).await?;
            let mut table = Table::new();

            table.set_header(vec![
                "Swap ID",
                "Start Date",
                "State",
                "Bitcoin Lock TxId",
                "BTC Amount",
                "XMR Amount",
                "Exchange Rate",
                "Taker Peer ID",
                "Completed",
            ]);

            let all_swaps = db.all().await?;
            for (swap_id, state) in all_swaps {
                let state: AliceState = state
                    .try_into()
                    .expect("Alice database only has Alice states");

                if only_unfinished && is_complete(&state) {
                    continue;
                }

                match SwapDetails::from_db_state(swap_id, state, &db).await {
                    Ok(details) => {
                        if json {
                            details.log_info();
                        } else {
                            table.add_row(details.to_table_row());
                        }
                    }
                    Err(e) => {
                        tracing::error!(swap_id = %swap_id, error = %e, "Failed to get swap details");
                    }
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

            let withdraw_tx_unsigned = match amount {
                Some(amount) => {
                    bitcoin_wallet
                        .send_to_address_dynamic_fee(address, amount, None)
                        .await?
                }
                None => {
                    bitcoin_wallet
                        .sweep_balance_to_address_dynamic_fee(address)
                        .await?
                }
            };

            let signed_tx = bitcoin_wallet
                .sign_and_finalize(withdraw_tx_unsigned)
                .await?;

            bitcoin_wallet.broadcast(signed_tx, "withdraw").await?;
        }
        Command::Balance => {
            let monero_wallet = init_monero_wallet(&config, env_config).await?;
            let monero_balance = monero_wallet.main_wallet().await.total_balance().await;
            tracing::info!(%monero_balance);

            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;
            let bitcoin_balance = bitcoin_wallet.balance().await?;
            tracing::info!(%bitcoin_balance);
            tracing::info!(%bitcoin_balance, %monero_balance, "Current balance");
        }
        Command::Cancel { swap_id } => {
            let db = open_db(db_file, AccessMode::ReadWrite, None).await?;

            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;

            let (txid, _) = cancel(swap_id, Arc::new(bitcoin_wallet), db).await?;

            tracing::info!("Cancel transaction successfully published with id {}", txid);
        }
        Command::Refund { swap_id } => {
            let db = open_db(db_file, AccessMode::ReadWrite, None).await?;

            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;
            let monero_wallet = init_monero_wallet(&config, env_config).await?;

            refund(swap_id, Arc::new(bitcoin_wallet), monero_wallet.clone(), db).await?;

            tracing::info!("Monero successfully refunded");
        }
        Command::Punish { swap_id } => {
            let db = open_db(db_file, AccessMode::ReadWrite, None).await?;

            let bitcoin_wallet = init_bitcoin_wallet(&config, &seed, env_config).await?;

            let (txid, _) = punish(swap_id, Arc::new(bitcoin_wallet), db).await?;

            tracing::info!("Punish transaction successfully published with id {}", txid);
        }
        Command::SafelyAbort { swap_id } => {
            let db = open_db(db_file, AccessMode::ReadWrite, None).await?;

            safely_abort(swap_id, db).await?;

            tracing::info!("Swap safely aborted");
        }
        Command::Redeem {
            swap_id,
            do_not_await_finality,
        } => {
            let db = open_db(db_file, AccessMode::ReadWrite, None).await?;

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
            println!("{}", wallet_export)
        }
        Command::ExportMoneroWallet => {
            let monero_wallet = init_monero_wallet(&config, env_config).await?;
            let main_wallet = monero_wallet.main_wallet().await;

            let seed = main_wallet.seed().await;
            let creation_height = main_wallet.creation_height().await;

            println!("Seed          : {seed}");
            println!("Restore height: {creation_height}");
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
    let wallet = bitcoin::wallet::WalletBuilder::default()
        .seed(seed.clone())
        .network(env_config.bitcoin_network)
        .electrum_rpc_urls(
            config
                .bitcoin
                .electrum_rpc_urls
                .iter()
                .map(|url| url.as_str().to_string())
                .collect::<Vec<String>>(),
        )
        .persister(bitcoin::wallet::PersisterConfig::SqliteFile {
            data_dir: config.data.dir.clone(),
        })
        .finality_confirmations(env_config.bitcoin_finality_confirmations)
        .target_block(config.bitcoin.target_block)
        .use_mempool_space_fee_estimation(config.bitcoin.use_mempool_space_fee_estimation)
        .sync_interval(env_config.bitcoin_sync_interval())
        .build()
        .await
        .context("Failed to initialize Bitcoin wallet")?;

    wallet.sync().await?;

    Ok(wallet)
}

async fn init_monero_wallet(
    config: &Config,
    env_config: swap::env::Config,
) -> Result<Arc<monero::Wallets>> {
    tracing::debug!("Initializing Monero wallets");

    let daemon = if config.monero.monero_node_pool {
        // Start the monero-rpc-pool and use it
        tracing::info!("Starting Monero RPC Pool for ASB");

        let (server_info, _status_receiver, _task_manager) =
            monero_rpc_pool::start_server_with_random_port(
                monero_rpc_pool::config::Config::new_random_port(
                    "127.0.0.1".to_string(),
                    config.data.dir.join("monero-rpc-pool"),
                ),
                env_config.monero_network,
            )
            .await
            .context("Failed to start Monero RPC Pool for ASB")?;

        let pool_url = format!("http://{}:{}", server_info.host, server_info.port);
        tracing::info!("Monero RPC Pool started for ASB on {}", pool_url);

        server_info
            .into_daemon()
            .context("Failed to convert ServerInfo to Daemon")?
    } else {
        tracing::info!(
            "Using direct Monero daemon connection: {}",
            config.monero.daemon_url
        );

        config
            .monero
            .daemon_url
            .clone()
            .into_daemon()
            .context("Failed to convert daemon URL to Daemon")?
    };

    let manager = monero::Wallets::new(
        config.data.dir.join("monero/wallets"),
        DEFAULT_WALLET_NAME.to_string(),
        daemon,
        env_config.monero_network,
        false,
        None,
    )
    .await
    .context("Failed to initialize Monero wallets")?;

    Ok(Arc::new(manager))
}

/// This struct is used to extract swap details from the database and print them in a table format
#[derive(Debug)]
struct SwapDetails {
    swap_id: String,
    start_date: String,
    state: String,
    btc_lock_txid: String,
    btc_amount: String,
    xmr_amount: String,
    exchange_rate: String,
    peer_id: String,
    completed: bool,
}

impl SwapDetails {
    async fn from_db_state(
        swap_id: Uuid,
        latest_state: AliceState,
        db: &Arc<dyn Database + Send + Sync>,
    ) -> Result<Self> {
        let completed = is_complete(&latest_state);

        let all_states = db.get_states(swap_id).await?;
        let state3 = all_states
            .iter()
            .find_map(|s| match s {
                State::Alice(AliceState::BtcLockTransactionSeen { state3 }) => Some(state3),
                _ => None,
            })
            .context("Failed to get \"BtcLockTransactionSeen\" state")?;

        let exchange_rate = Self::calculate_exchange_rate(state3.btc, state3.xmr)?;
        let start_date = db.get_swap_start_date(swap_id).await?;
        let btc_lock_txid = state3.tx_lock.txid();
        let peer_id = db.get_peer_id(swap_id).await?;

        Ok(Self {
            swap_id: swap_id.to_string(),
            start_date: start_date.to_string(),
            state: latest_state.to_string(),
            btc_lock_txid: btc_lock_txid.to_string(),
            btc_amount: state3.btc.to_string(),
            xmr_amount: state3.xmr.to_string(),
            exchange_rate,
            peer_id: peer_id.to_string(),
            completed,
        })
    }

    fn calculate_exchange_rate(btc: bitcoin::Amount, xmr: monero::Amount) -> Result<String> {
        let btc_decimal = Decimal::from_f64(btc.to_btc())
            .ok_or_else(|| anyhow::anyhow!("Failed to convert BTC amount to Decimal"))?;
        let xmr_decimal = Decimal::from_f64(xmr.as_xmr())
            .ok_or_else(|| anyhow::anyhow!("Failed to convert XMR amount to Decimal"))?;

        let rate = btc_decimal
            .checked_div(xmr_decimal)
            .ok_or_else(|| anyhow::anyhow!("Division by zero or overflow"))?;

        Ok(format!("{} XMR/BTC", rate.round_dp(8)))
    }

    fn to_table_row(&self) -> Vec<String> {
        vec![
            self.swap_id.clone(),
            self.start_date.clone(),
            self.state.clone(),
            self.btc_lock_txid.clone(),
            self.btc_amount.clone(),
            self.xmr_amount.clone(),
            self.exchange_rate.clone(),
            self.peer_id.clone(),
            self.completed.to_string(),
        ]
    }

    fn log_info(&self) {
        tracing::info!(
            swap_id = %self.swap_id,
            swap_start_date = %self.start_date,
            latest_state = %self.state,
            btc_lock_txid = %self.btc_lock_txid,
            btc_amount = %self.btc_amount,
            xmr_amount = %self.xmr_amount,
            exchange_rate = %self.exchange_rate,
            taker_peer_id = %self.peer_id,
            completed = self.completed,
            "Found swap in database"
        );
    }
}
