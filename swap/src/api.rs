use anyhow::{bail, Context, Result};
use comfy_table::Table;
use jsonrpsee::http_server::{HttpServerHandle};
use qrcode::render::unicode;
use qrcode::QrCode;
use crate::env::GetConfig;
use std::cmp::min;
use crate::network::rendezvous::XmrBtcNamespace;
use std::net::SocketAddr;
use libp2p::core::Multiaddr;
use std::convert::TryInto;
use crate::bitcoin::Amount;
use std::env;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use crate::bitcoin::TxLock;
use crate::cli::command::{parse_args_and_apply_defaults, Command, ParseResult, Options, Bitcoin, Monero, Tor};
use crate::cli::{list_sellers, EventLoop, SellerStatus};
use crate::common::check_latest_version;
use crate::database::open_db;
use crate::libp2p_ext::MultiAddrExt;
use crate::network::quote::{BidQuote, ZeroQuoteReceived};
use crate::network::swarm;
use crate::protocol::bob;
use crate::protocol::bob::{BobState, Swap};
use crate::seed::Seed;
use crate::rpc;
use crate::{bitcoin, cli, monero};
use url::Url;
use uuid::Uuid;
use crate::protocol::Database;
use crate::env::{Config, Mainnet, Testnet};
use crate::fs::system_data_dir;
use serde_json::json;
use std::str::FromStr;
use tokio::task;
use serde::ser::{Serialize, Serializer, SerializeStruct};


pub struct Request {
    pub params: Params,
    pub cmd: Command,
}

#[derive(Default)]
pub struct Params {
    pub seller: Option<Multiaddr>,
    pub bitcoin_change_address: Option<bitcoin::Address>,
    pub monero_receive_address: Option<monero::Address>,
    pub rendezvous_point: Option<Multiaddr>,
    pub swap_id: Option<Uuid>,
    pub amount: Option<Amount>,
    pub address: Option<bitcoin::Address>,
}

pub struct Init {
    db: Arc<dyn Database + Send + Sync>,
    pub bitcoin_wallet: Option<Arc<bitcoin::Wallet>>,
    monero_wallet: Option<Arc<monero::Wallet>>,
    tor_socks5_port: Option<u16>,
    namespace: XmrBtcNamespace,
    //server_handle: Option<task::JoinHandle<()>>,
    server_address: Option<SocketAddr>,
    pub seed: Option<Seed>,
    pub debug: bool,
    pub json: bool,
    pub is_testnet: bool,
}

impl Request {
    pub async fn call(&self, api_init: Arc<Init>) -> Result<serde_json::Value> {
        let result = match self.cmd {
            Command::BuyXmr => {
                let swap_id = Uuid::new_v4();

                let seed = api_init.seed.as_ref().unwrap();
                let env_config = env_config_from(api_init.is_testnet);
                let btc = api_init.bitcoin_wallet.as_ref().unwrap();
                let seller = self.params.seller.clone().unwrap();
                let monero_receive_address = self.params.monero_receive_address.unwrap();
                let bitcoin_change_address = self.params.bitcoin_change_address.clone().unwrap();

                let bitcoin_wallet = btc;
                let seller_peer_id = self.params.seller.as_ref().unwrap()
                    .extract_peer_id()
                    .context("Seller address must contain peer ID")?;
                api_init.db.insert_address(seller_peer_id, seller.clone()).await?;

                let behaviour = cli::Behaviour::new(
                    seller_peer_id,
                    env_config_from(api_init.is_testnet),
                    bitcoin_wallet.clone(),
                    (seed.derive_libp2p_identity(), api_init.namespace),
                );
                let mut swarm =
                    swarm::cli(seed.derive_libp2p_identity(), api_init.tor_socks5_port.unwrap(), behaviour).await?;
                swarm.behaviour_mut().add_address(seller_peer_id, seller);

                tracing::debug!(peer_id = %swarm.local_peer_id(), "Network layer initialized");

                let (event_loop, mut event_loop_handle) =
                    EventLoop::new(swap_id, swarm, seller_peer_id)?;
                let event_loop = tokio::spawn(event_loop.run());

                let max_givable = || bitcoin_wallet.max_giveable(TxLock::script_size());
                let estimate_fee = |amount| bitcoin_wallet.estimate_fee(TxLock::weight(), amount);

                let (amount, fees) = match determine_btc_to_swap(
                    api_init.json,
                    event_loop_handle.request_quote(),
                    bitcoin_wallet.new_address(),
                    || bitcoin_wallet.balance(),
                    max_givable,
                    || bitcoin_wallet.sync(),
                    estimate_fee,
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

                api_init.db.insert_peer_id(swap_id, seller_peer_id).await?;
                api_init.db.insert_monero_address(swap_id, monero_receive_address)
                    .await?;
                let monero_wallet = api_init.monero_wallet.as_ref().unwrap();

                let swap = Swap::new(
                    Arc::clone(&api_init.db),
                    swap_id,
                    Arc::clone(&bitcoin_wallet),
                    Arc::clone(&monero_wallet),
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
                json!({
                    "empty": "true"
                })
            }
            Command::History => {
                let swaps = api_init.db.all().await?;
                let mut vec: Vec<(Uuid, String)> = Vec::new();
                for (swap_id, state) in swaps {
                    let state: BobState = state.try_into()?;
                    vec.push((swap_id, state.to_string()));
                }
                json!({
                    "swaps": vec
                })

            }
            Command::Config => {
                json!({
                    "empty": "true"
                })
            }
            Command::WithdrawBtc => {
                json!({
                    "empty": "true"
                })
            }
            Command::StartDaemon => {
                let addr2 = "127.0.0.1:1234".parse()?;

                let server_handle = {
                    if let Some(addr) = api_init.server_address {
                        let (_addr, handle) = rpc::run_server(addr, api_init).await?;
                        Some(handle)
                    } else {
                        let (_addr, handle) = rpc::run_server(addr2, api_init).await?;
                        Some(handle)
                    }
                };
                loop {

                }
                json!({
                    "empty": "true"
                })
            }
            Command::Balance => {
                let debug = api_init.debug;
                let json = api_init.json;
                let is_testnet = api_init.is_testnet;

                let bitcoin_balance = api_init.bitcoin_wallet
                    .as_ref().unwrap().balance().await?;
                tracing::info!(
                    balance = %bitcoin_balance,
                    "Checked Bitcoin balance",
                );
                json!({
                    "balance": bitcoin_balance.as_sat()
                })
            }
            Command::Resume => {
                json!({
                    "empty": "true"
                })
            }
            Command::Cancel => {
                json!({
                    "empty": "true"
                })
            }
            Command::Refund => {
                json!({
                    "empty": "true"
                })
            }
            Command::ListSellers => {
                let rendezvous_point = self.params.rendezvous_point.clone().unwrap();
                let rendezvous_node_peer_id = rendezvous_point
                    .extract_peer_id()
                    .context("Rendezvous node address must contain peer ID")?;

                let identity = api_init.seed.as_ref().unwrap().derive_libp2p_identity();

                let sellers = list_sellers(
                    rendezvous_node_peer_id,
                    rendezvous_point,
                    api_init.namespace,
                    api_init.tor_socks5_port.unwrap(),
                    identity,
                )
                .await?;


                json!({
                    "empty": "true"
                })
            }
            Command::ExportBitcoinWallet => {
                json!({
                    "empty": "true"
                })
            }
            Command::MoneroRecovery => {
                json!({
                    "empty": "true"
                })
            }
        };
        Ok(result)
    }
}
impl Init {
    //pub async fn build_server(bitcoin_electrum_rpc_url: Url, bitcoin_target_block: usize, monero_daemon_address: String, tor_socks5_port: u16, namespace: XmrBtcNamespace, server_address: SocketAddr, data_dir: PathBuf, env_config: Config) -> Result<Init> {
    pub async fn build(
        bitcoin: Bitcoin,
        monero: Monero, 
        tor: Option<Tor>, 
        data: Option<PathBuf>, 
        is_testnet: bool,
        debug: bool,
        json: bool,
        server_address: Option<SocketAddr>,
        ) -> Result<Init> {
            let (bitcoin_electrum_rpc_url, bitcoin_target_block) =
                bitcoin.apply_defaults(is_testnet)?;

            let monero_daemon_address = monero.apply_defaults(is_testnet);


            let data_dir = data::data_dir_from(data, is_testnet)?;
            let env_config = env_config_from(is_testnet);

            let seed = Seed::from_file_or_generate(data_dir.as_path())
                .context("Failed to read seed in file")?;



            let tor_socks5_port = {
                if let Some(tor) = tor {
                    Some(tor.tor_socks5_port)
                } else {
                    None
                }
            };

            cli::tracing::init(debug, json, data_dir.join("logs"), None)?;

            let init = Init {
                bitcoin_wallet: Some(Arc::new(init_bitcoin_wallet(
                    bitcoin_electrum_rpc_url,
                    &seed,
                    data_dir.clone(),
                    env_config,
                    bitcoin_target_block,
                )
                .await?)),

                monero_wallet: Some(Arc::new(init_monero_wallet(
                    data_dir.clone(),
                    monero_daemon_address,
                    env_config,
                )
                .await?.0)),
                tor_socks5_port: tor_socks5_port,
                namespace: XmrBtcNamespace::from_is_testnet(is_testnet),
                db: open_db(data_dir.join("sqlite")).await?,
                seed: Some(seed),
                debug,
                json,
                is_testnet,
                server_address,
            };
            

            Ok(init)
        }

    pub async fn build_walletless(
        tor: Option<Tor>,
        data: Option<PathBuf>,
        is_testnet: bool,
        debug: bool,
        json: bool,
        ) -> Result<Init> {
            let data_dir = data::data_dir_from(data, is_testnet)?;
            let env_config = env_config_from(is_testnet);

            let tor_socks5_port = {
                if let Some(tor) = tor {
                    Some(tor.tor_socks5_port)
                } else {
                    None
                }
            };
            cli::tracing::init(debug, json, data_dir.join("logs"), None)?;

            let init = Init {
                bitcoin_wallet: None,
                monero_wallet: None,
                tor_socks5_port, 
                namespace: XmrBtcNamespace::from_is_testnet(is_testnet),
                db: open_db(data_dir.join("sqlite")).await?,
                seed: None,
                debug,
                json,
                is_testnet,
                server_address: None,
            };
            Ok(init)
    }

    pub async fn build_with_btc(
        bitcoin: Bitcoin,
        tor: Option<Tor>,
        data: Option<PathBuf>,
        is_testnet: bool,
        debug: bool,
        json: bool,
        ) -> Result<Init> {
            let (bitcoin_electrum_rpc_url, bitcoin_target_block) =
                bitcoin.apply_defaults(is_testnet)?;

            let data_dir = data::data_dir_from(data, is_testnet)?;
            let env_config = env_config_from(is_testnet);

            let seed = Seed::from_file_or_generate(data_dir.as_path())
                .context("Failed to read seed in file")?;

            let tor_socks5_port = {
                if let Some(tor) = tor {
                    Some(tor.tor_socks5_port)
                } else {
                    None
                }
            };

            cli::tracing::init(debug, json, data_dir.join("logs"), None)?;

            let init = Init {
                bitcoin_wallet: Some(Arc::new(init_bitcoin_wallet(
                    bitcoin_electrum_rpc_url,
                    &seed,
                    data_dir.clone(),
                    env_config,
                    bitcoin_target_block,
                )
                .await?)),
                monero_wallet: None,
                tor_socks5_port, 
                namespace: XmrBtcNamespace::from_is_testnet(is_testnet),
                db: open_db(data_dir.join("sqlite")).await?,
                seed: Some(seed),
                debug,
                json,
                is_testnet,
                server_address: None,
            };
            Ok(init)
    }
}

impl Serialize for Init {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // 3 is the number of fields in the struct.
        let mut state = serializer.serialize_struct("Init", 3)?;
        state.serialize_field("debug", &self.debug)?;
        state.serialize_field("json", &self.json)?;
        state.end()
    }
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

fn qr_code(value: &impl ToString) -> Result<String> {
    let code = QrCode::new(value.to_string())?;
    let qr_code = code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();
    Ok(qr_code)
}
async fn determine_btc_to_swap<FB, TB, FMG, TMG, FS, TS, FFE, TFE>(
    json: bool,
    bid_quote: impl Future<Output = Result<BidQuote>>,
    get_new_address: impl Future<Output = Result<bitcoin::Address>>,
    balance: FB,
    max_giveable_fn: FMG,
    sync: FS,
    estimate_fee: FFE,
) -> Result<(bitcoin::Amount, bitcoin::Amount)>
where
    TB: Future<Output = Result<bitcoin::Amount>>,
    FB: Fn() -> TB,
    TMG: Future<Output = Result<bitcoin::Amount>>,
    FMG: Fn() -> TMG,
    TS: Future<Output = Result<()>>,
    FS: Fn() -> TS,
    FFE: Fn(bitcoin::Amount) -> TFE,
    TFE: Future<Output = Result<bitcoin::Amount>>,
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
            let min_outstanding = bid_quote.min_quantity - max_giveable;
            let min_fee = estimate_fee(min_outstanding).await?;
            let min_deposit = min_outstanding + min_fee;

            tracing::info!(
                "Deposit at least {} to cover the min quantity with fee!",
                min_deposit
            );
            tracing::info!(
                %deposit_address,
                %min_deposit,
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

mod data {
    use super::*;

    pub fn data_dir_from(arg_dir: Option<PathBuf>, testnet: bool) -> Result<PathBuf> {
        let base_dir = match arg_dir {
            Some(custom_base_dir) => custom_base_dir,
            None => os_default()?,
        };

        let sub_directory = if testnet { "testnet" } else { "mainnet" };

        Ok(base_dir.join(sub_directory))
    }

    fn os_default() -> Result<PathBuf> {
        Ok(system_data_dir()?.join("cli"))
    }
}

fn env_config_from(testnet: bool) -> Config {
    if testnet {
        Testnet::get_config()
    } else {
        Mainnet::get_config()
    }
}
