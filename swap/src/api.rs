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
    bitcoin_wallet: Option<bitcoin::Wallet>,
    monero_wallet: Option<(monero::Wallet, monero::WalletRpcProcess)>,
    tor_socks5_port: Option<u16>,
    namespace: XmrBtcNamespace,
    server_handle: Option<HttpServerHandle>,
    debug: bool,
    json: bool,
    is_testnet: bool,
}

impl Request {
    pub async fn call(&self, api_init: &Init) -> Result<()> {
        match self.cmd {
            Command::BuyXmr => { }
            Command::History => {
            }
            Command::Config => { }
            Command::WithdrawBtc => { }
            Command::StartDaemon => {
            }
            Command::Balance => {
            }
            Command::Resume => { }
            Command::Cancel => { }
            Command::Refund => { }
            Command::ListSellers => { }
            Command::ExportBitcoinWallet => { }
            Command::MoneroRecovery => { }
        }
        Ok(())
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

            let server_handle = {
                if let Some(addr) = server_address {
                    let (_addr, handle) = rpc::run_server(addr).await?;
                    Some(handle)
                } else {
                    None
                }
            };

            let tor_socks5_port = {
                if let Some(tor) = tor {
                    Some(tor.tor_socks5_port)
                } else {
                    None
                }
            };

            let init = Init {
                bitcoin_wallet: Some(init_bitcoin_wallet(
                    bitcoin_electrum_rpc_url,
                    &seed,
                    data_dir.clone(),
                    env_config,
                    bitcoin_target_block,
                )
                .await?),

                monero_wallet: Some(init_monero_wallet(
                    data_dir.clone(),
                    monero_daemon_address,
                    env_config,
                )
                .await?),
                tor_socks5_port: tor_socks5_port,
                namespace: XmrBtcNamespace::from_is_testnet(is_testnet),
                db: open_db(data_dir.join("sqlite")).await?,
                debug,
                json,
                is_testnet,
                server_handle,
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

            let init = Init {
                bitcoin_wallet: None,
                monero_wallet: None,
                tor_socks5_port, 
                namespace: XmrBtcNamespace::from_is_testnet(is_testnet),
                db: open_db(data_dir.join("sqlite")).await?,
                debug,
                json,
                is_testnet,
                server_handle: None,
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

            let init = Init {
                bitcoin_wallet: Some(init_bitcoin_wallet(
                    bitcoin_electrum_rpc_url,
                    &seed,
                    data_dir.clone(),
                    env_config,
                    bitcoin_target_block,
                )
                .await?),
                monero_wallet: None,
                tor_socks5_port, 
                namespace: XmrBtcNamespace::from_is_testnet(is_testnet),
                db: open_db(data_dir.join("sqlite")).await?,
                debug,
                json,
                is_testnet,
                server_handle: None,
            };
            Ok(init)
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
