use anyhow::{bail, Context, Result};
use comfy_table::Table;
use jsonrpsee::http_server::{HttpServerHandle};
use qrcode::render::unicode;
use qrcode::QrCode;
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
use crate::cli::command::{parse_args_and_apply_defaults, Command, ParseResult, Options};
use crate::cli::{list_sellers, EventLoop, SellerStatus};
use crate::common::check_latest_version;
use crate::database::open_db;
use crate::env::Config;
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

#[derive(Debug, PartialEq)]
pub struct InternalApi {
    pub opts: Options,
    pub params: Params,
    pub cmd: Command,
}

#[derive(Debug, PartialEq, Default)]
pub struct Params {
    pub bitcoin_electrum_rpc_url: Option<Url>,
    pub bitcoin_target_block: Option<usize>,
    pub seller: Option<Multiaddr>,
    pub bitcoin_change_address: Option<bitcoin::Address>,
    pub monero_receive_address: Option<monero::Address>,
    pub monero_daemon_address: Option<String>,
    pub tor_socks5_port: Option<u16>,
    pub namespace: Option<XmrBtcNamespace>,
    pub rendezvous_point: Option<Multiaddr>,
    pub swap_id: Option<Uuid>,
    pub server_address: Option<SocketAddr>,
    pub amount: Option<Amount>,
    pub address: Option<bitcoin::Address>,
}

impl InternalApi {
    pub async fn call(self) -> Result<()> {
        let opts = &self.opts;
        let params = self.params;
        match self.cmd {
            Command::BuyXmr => { }
            Command::History => {
                cli::tracing::init(opts.debug, opts.json, opts.data_dir.join("logs"), None)?;

                let db = open_db(opts.data_dir.join("sqlite")).await?;
                let swaps = db.all().await?;

                if opts.json {
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
            Command::Config => { }
            Command::WithdrawBtc => { }
            Command::StartDaemon => {
                let handle = rpc::run_server(params.server_address.unwrap()).await?;
                loop {}
            }
            Command::Balance => {
                cli::tracing::init(opts.debug, opts.json, opts.data_dir.join("logs"), None)?;

                let seed = Seed::from_file_or_generate(opts.data_dir.as_path())
                    .context("Failed to read in seed file")?;
                let bitcoin_wallet = init_bitcoin_wallet(
                    params.bitcoin_electrum_rpc_url.unwrap(),
                    &seed,
                    opts.data_dir.clone(),
                    opts.env_config,
                    params.bitcoin_target_block.unwrap(),
                )
                .await?;

                let bitcoin_balance = bitcoin_wallet.balance().await?;
                tracing::info!(
                    balance = %bitcoin_balance,
                    "Checked Bitcoin balance",
                );
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
