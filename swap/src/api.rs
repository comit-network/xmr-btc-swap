use anyhow::{bail, Context as AnyContext, Result};
use comfy_table::Table;
use qrcode::render::unicode;
use qrcode::QrCode;
use crate::env::GetConfig;
use std::cmp::min;
use crate::network::rendezvous::XmrBtcNamespace;
use std::net::SocketAddr;
use libp2p::core::Multiaddr;
use std::convert::TryInto;
use crate::bitcoin::Amount;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use crate::bitcoin::TxLock;
use crate::cli::command::{Command, Bitcoin, Monero, Tor};
use crate::cli::{list_sellers, EventLoop, SellerStatus};
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
use serde::ser::{Serialize, Serializer, SerializeStruct};
use std::fmt;


#[derive(PartialEq, Debug)]
pub struct Request {
    pub params: Params,
    pub cmd: Command,
}

#[derive(Default, PartialEq, Debug)]
pub struct Params {
    pub seller: Option<Multiaddr>,
    pub bitcoin_change_address: Option<bitcoin::Address>,
    pub monero_receive_address: Option<monero::Address>,
    pub rendezvous_point: Option<Multiaddr>,
    pub swap_id: Option<Uuid>,
    pub amount: Option<Amount>,
    pub address: Option<bitcoin::Address>,
}

pub struct Context {
    db: Arc<dyn Database + Send + Sync>,
    bitcoin_wallet: Option<Arc<bitcoin::Wallet>>,
    monero_wallet: Option<Arc<monero::Wallet>>,
    monero_rpc_process: Option<monero::WalletRpcProcess>,
    tor_socks5_port: Option<u16>,
    namespace: XmrBtcNamespace,
    server_address: Option<SocketAddr>,
    env_config: Config,
    seed: Option<Seed>,
    debug: bool,
    json: bool,
    is_testnet: bool,
}

impl Request {
    pub async fn call(&self, context: Arc<Context>) -> Result<serde_json::Value> {
        let result = match self.cmd {
            Command::BuyXmr => {
                let swap_id = Uuid::new_v4();

                let seed = context.seed.as_ref().unwrap();
                let env_config = context.env_config;
                let btc = context.bitcoin_wallet.as_ref().unwrap();
                let seller = self.params.seller.clone().unwrap();
                let monero_receive_address = self.params.monero_receive_address.unwrap();
                let bitcoin_change_address = self.params.bitcoin_change_address.clone().unwrap();

                let bitcoin_wallet = btc;
                let seller_peer_id = self.params.seller.as_ref().unwrap()
                    .extract_peer_id()
                    .context("Seller address must contain peer ID")?;
                context.db.insert_address(seller_peer_id, seller.clone()).await?;

                let behaviour = cli::Behaviour::new(
                    seller_peer_id,
                    env_config,
                    bitcoin_wallet.clone(),
                    (seed.derive_libp2p_identity(), context.namespace),
                );
                let mut swarm =
                    swarm::cli(seed.derive_libp2p_identity(), context.tor_socks5_port.unwrap(), behaviour).await?;
                swarm.behaviour_mut().add_address(seller_peer_id, seller);

                tracing::debug!(peer_id = %swarm.local_peer_id(), "Network layer initialized");

                let (event_loop, mut event_loop_handle) =
                    EventLoop::new(swap_id, swarm, seller_peer_id)?;
                let event_loop = tokio::spawn(event_loop.run());

                let max_givable = || bitcoin_wallet.max_giveable(TxLock::script_size());
                let estimate_fee = |amount| bitcoin_wallet.estimate_fee(TxLock::weight(), amount);

                let (amount, fees) = match determine_btc_to_swap(
                    context.json,
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

                context.db.insert_peer_id(swap_id, seller_peer_id).await?;
                context.db.insert_monero_address(swap_id, monero_receive_address)
                    .await?;
                let monero_wallet = context.monero_wallet.as_ref().unwrap();

                let swap = Swap::new(
                    Arc::clone(&context.db),
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
                        result
                            .context("Failed to complete swap")?;
                    }
                }
                json!({
                    "empty": "true"
                })
            }
            Command::History => {
                let swaps = context.db.all().await?;
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
//                tracing::info!(path=%data_dir.display(), "Data directory");
//                tracing::info!(path=%format!("{}/logs", data_dir.display()), "Log files directory");
//                tracing::info!(path=%format!("{}/sqlite", data_dir.display()), "Sqlite file location");
//                tracing::info!(path=%format!("{}/seed.pem", data_dir.display()), "Seed file location");
//                tracing::info!(path=%format!("{}/monero", data_dir.display()), "Monero-wallet-rpc directory");
//                tracing::info!(path=%format!("{}/wallet", data_dir.display()), "Internal bitcoin wallet directory");

                json!({
                    "result": []
                })
            }
            Command::WithdrawBtc => {
                let bitcoin_wallet = context.bitcoin_wallet.as_ref().unwrap();

                let address = self.params.address.clone().unwrap();

                let amount = match self.params.amount {
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

                bitcoin_wallet.broadcast(signed_tx.clone(), "withdraw").await?;

                json!({
                    "signed_tx": signed_tx,
                    "amount": amount.as_sat(),
                    "txid": signed_tx.txid(),
                })
            }
            Command::StartDaemon => {
                let addr2 = "127.0.0.1:1234".parse()?;

                let server_handle = {
                    if let Some(addr) = context.server_address {
                        let (_addr, handle) = rpc::run_server(addr, context).await?;
                        Some(handle)
                    } else {
                        let (_addr, handle) = rpc::run_server(addr2, context).await?;
                        Some(handle)
                    }
                };
                loop {

                }
                json!({
                    "result": []
                })
            }
            Command::Balance => {
                let bitcoin_wallet = context.bitcoin_wallet.as_ref().unwrap();

                bitcoin_wallet.sync().await?;
                let bitcoin_balance = bitcoin_wallet.balance().await?;
                tracing::info!(
                    balance = %bitcoin_balance,
                    "Checked Bitcoin balance",
                );

                json!({
                    "balance": bitcoin_balance.as_sat()
                })
            }
            Command::Resume => {
                let swap_id = self.params.swap_id.unwrap();

                let seller_peer_id = context.db.get_peer_id(swap_id).await?;
                let seller_addresses = context.db.get_addresses(seller_peer_id).await?;

                let seed = context.seed.as_ref().unwrap().derive_libp2p_identity();

                let behaviour = cli::Behaviour::new(
                    seller_peer_id,
                    context.env_config,
                    Arc::clone(context.bitcoin_wallet.as_ref().unwrap()),
                    (seed.clone(), context.namespace),
                );
                let mut swarm =
                    swarm::cli(seed.clone(), context.tor_socks5_port.clone().unwrap(), behaviour).await?;
                let our_peer_id = swarm.local_peer_id();

                tracing::debug!(peer_id = %our_peer_id, "Network layer initialized");

                for seller_address in seller_addresses {
                    swarm
                        .behaviour_mut()
                        .add_address(seller_peer_id, seller_address);
                }

                let (event_loop, event_loop_handle) = EventLoop::new(swap_id, swarm, seller_peer_id)?;
                let handle = tokio::spawn(event_loop.run());

                let monero_receive_address = context.db.get_monero_address(swap_id).await?;
                let swap = Swap::from_db(
                    Arc::clone(&context.db),
                    swap_id,
                    Arc::clone(context.bitcoin_wallet.as_ref().unwrap()),
                    Arc::clone(context.monero_wallet.as_ref().unwrap()),
                    context.env_config,
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
                json!({
                    "result": []
                })
            }
            Command::Cancel => {
                let bitcoin_wallet = context.bitcoin_wallet.as_ref().unwrap();

                let (txid, _) = cli::cancel(self.params.swap_id.unwrap(), Arc::clone(bitcoin_wallet), Arc::clone(&context.db)).await?;
                
                tracing::debug!("Cancel transaction successfully published with id {}", txid);

                json!({
                    "txid": txid,
                })
            }
            Command::Refund => {
                let bitcoin_wallet = context.bitcoin_wallet.as_ref().unwrap();

                let state = cli::refund(self.params.swap_id.unwrap(), Arc::clone(bitcoin_wallet), Arc::clone(&context.db)).await?;

                json!({
                    "result": state
                })
            }
            Command::ListSellers => {
                let rendezvous_point = self.params.rendezvous_point.clone().unwrap();
                let rendezvous_node_peer_id = rendezvous_point
                    .extract_peer_id()
                    .context("Rendezvous node address must contain peer ID")?;

                let identity = context.seed.as_ref().unwrap().derive_libp2p_identity();

                let sellers = list_sellers(
                    rendezvous_node_peer_id,
                    rendezvous_point,
                    context.namespace,
                    context.tor_socks5_port.unwrap(),
                    identity,
                ).await?;

                for seller in &sellers {
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

                json!({
                    "sellers": sellers
                })
            }
            Command::ExportBitcoinWallet => {
                let bitcoin_wallet = context.bitcoin_wallet.as_ref().unwrap();

                let wallet_export = bitcoin_wallet.wallet_export("cli").await?;
                tracing::info!(descriptor=%wallet_export.to_string(), "Exported bitcoin wallet");
                json!({
                    "result": []
                })
            }
            Command::MoneroRecovery => {
                let swap_state: BobState = context.db.get_state(self.params.swap_id.clone().unwrap()).await?.try_into()?;

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
                            context.env_config.monero_network,
                            monero::PublicKey::from_private_key(&spend_key),
                            monero::PublicKey::from(view_key.public()),
                        );
                        tracing::info!("Wallet address: {}", address.to_string());

                        let view_key = serde_json::to_string(&view_key)?;
                        println!("View key: {}", view_key);

                        println!("Spend key: {}", spend_key);
                    }
                }
                json!({
                    "result": []
                })
            }
        };
        Ok(result)
    }
}

impl Context {
    pub async fn build(
        bitcoin: Option<Bitcoin>,
        monero: Option<Monero>, 
        tor: Option<Tor>, 
        data: Option<PathBuf>, 
        is_testnet: bool,
        debug: bool,
        json: bool,
        server_address: Option<SocketAddr>,
        ) -> Result<Context> {
            let data_dir = data::data_dir_from(data, is_testnet)?;
            let env_config = env_config_from(is_testnet);

            let seed = Seed::from_file_or_generate(data_dir.as_path())
                .context("Failed to read seed in file")?;

            let bitcoin_wallet = {
                if let Some(bitcoin) = bitcoin {
                    let (bitcoin_electrum_rpc_url, bitcoin_target_block) =
                        bitcoin.apply_defaults(is_testnet)?;
                    Some(Arc::new(init_bitcoin_wallet(
                        bitcoin_electrum_rpc_url,
                        &seed,
                        data_dir.clone(),
                        env_config,
                        bitcoin_target_block,
                        )
                        .await?))
                } else {
                    None
                }
            };

            let (monero_wallet, monero_rpc_process) = {
                if let Some(monero) = monero {
                    let monero_daemon_address = monero.apply_defaults(is_testnet);
                    let (wlt, prc) = init_monero_wallet(
                        data_dir.clone(),
                        monero_daemon_address,
                        env_config,
                        ).await?;
                    (Some(Arc::new(wlt)), Some(prc))
                } else {
                    (None, None)
                }
            };


            let tor_socks5_port = {
                if let Some(tor) = tor {
                    Some(tor.tor_socks5_port)
                } else {
                    None
                }
            };

            cli::tracing::init(debug, json, data_dir.join("logs"), None)?;

            let init = Context {
                bitcoin_wallet,
                monero_wallet,
                monero_rpc_process,
                tor_socks5_port: tor_socks5_port,
                namespace: XmrBtcNamespace::from_is_testnet(is_testnet),
                db: open_db(data_dir.join("sqlite")).await?,
                env_config,
                seed: Some(seed),
                debug,
                json,
                is_testnet,
                server_address,
            };
            

            Ok(init)
    }

}

impl Serialize for Context {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // 3 is the number of fields in the struct.
        let mut state = serializer.serialize_struct("Context", 3)?;
        state.serialize_field("debug", &self.debug)?;
        state.serialize_field("json", &self.json)?;
        state.end()
    }
}

impl PartialEq for Context {
    fn eq(&self, other: &Self) -> bool {
        self.tor_socks5_port == other.tor_socks5_port &&
        self.namespace == other.namespace &&
        self.debug == other.debug &&
        self.json == other.json &&
        self.server_address == other.server_address
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
          write!(f, "Testing {}", true)
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
pub async fn determine_btc_to_swap<FB, TB, FMG, TMG, FS, TS, FFE, TFE>(
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
#[cfg(test)]
pub mod api_test {
    use super::*;
    use crate::tor::DEFAULT_SOCKS5_PORT;
    use std::str::FromStr;

    pub const MULTI_ADDRESS: &str =
        "/ip4/127.0.0.1/tcp/9939/p2p/12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi";
    pub const MONERO_STAGENET_ADDRESS: &str = "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a";
    pub const BITCOIN_TESTNET_ADDRESS: &str = "tb1qr3em6k3gfnyl8r7q0v7t4tlnyxzgxma3lressv";
    pub const MONERO_MAINNET_ADDRESS: &str = "44Ato7HveWidJYUAVw5QffEcEtSH1DwzSP3FPPkHxNAS4LX9CqgucphTisH978FLHE34YNEx7FcbBfQLQUU8m3NUC4VqsRa";
    pub const BITCOIN_MAINNET_ADDRESS: &str = "bc1qe4epnfklcaa0mun26yz5g8k24em5u9f92hy325";
    pub const SWAP_ID: &str = "ea030832-3be9-454f-bb98-5ea9a788406b";

    impl Context {

        pub async fn default(is_testnet: bool, data_dir: PathBuf, json: bool, debug: bool) -> Result<Context> {

            Ok(Context::build(
                Some(Bitcoin { bitcoin_electrum_rpc_url: None, bitcoin_target_block: None}),
                Some(Monero { monero_daemon_address: None }),
                Some(Tor { tor_socks5_port: DEFAULT_SOCKS5_PORT }),
                Some(data_dir),
                is_testnet,
                debug,
                json,
                None
            ).await?)
        }

    }
    impl Request {

        pub fn buy_xmr(is_testnet: bool) -> Request {

            let seller = Multiaddr::from_str(MULTI_ADDRESS).unwrap();
            let bitcoin_change_address = {
                if is_testnet {
                    bitcoin::Address::from_str(BITCOIN_TESTNET_ADDRESS).unwrap()
                } else {
                    bitcoin::Address::from_str(BITCOIN_MAINNET_ADDRESS).unwrap()
                }
            };

            let monero_receive_address = {
                if is_testnet {
                    monero::Address::from_str(MONERO_STAGENET_ADDRESS).unwrap()
                } else {
                    monero::Address::from_str(MONERO_MAINNET_ADDRESS).unwrap()
                }
            };

            Request {
                params: Params {
                    seller: Some(seller),
                    bitcoin_change_address: Some(bitcoin_change_address),
                    monero_receive_address: Some(monero_receive_address),
                    ..Default::default()

                },
                cmd: Command::BuyXmr
            }
        }

        pub fn resume() -> Request {
            Request {
                params: Params {
                    swap_id: Some(Uuid::from_str(SWAP_ID).unwrap()),
                    ..Default::default()

                },
                cmd: Command::Resume
            }
        }

        pub fn cancel() -> Request {
            Request {
                params: Params {
                    swap_id: Some(Uuid::from_str(SWAP_ID).unwrap()),
                    ..Default::default()

                },
                cmd: Command::Cancel
            }
        }

        pub fn refund() -> Request {
            Request {
                params: Params {
                    swap_id: Some(Uuid::from_str(SWAP_ID).unwrap()),
                    ..Default::default()

                },
                cmd: Command::Refund
            }
        }
    }
}
mod tests {
    use super::*;
}
