use crate::bitcoin::{Amount, TxLock};
use crate::cli::command::{Bitcoin, Monero, Tor};
use crate::cli::{list_sellers, EventLoop, SellerStatus};
use crate::database::open_db;
use crate::env::{Config as EnvConfig, GetConfig, Mainnet, Testnet};
use crate::fs::system_data_dir;
use crate::libp2p_ext::MultiAddrExt;
use crate::network::quote::{BidQuote, ZeroQuoteReceived};
use crate::network::rendezvous::XmrBtcNamespace;
use crate::network::swarm;
use crate::protocol::bob::{BobState, Swap};
use crate::protocol::{bob, Database};
use crate::seed::Seed;
use crate::{bitcoin, cli, monero, rpc};
use anyhow::{bail, Context as AnyContext, Result};
use comfy_table::Table;
use libp2p::core::Multiaddr;
use qrcode::render::unicode;
use qrcode::QrCode;
use serde::ser::{Serialize, SerializeStruct, Serializer};
use serde_json::json;
use std::cmp::min;
use std::convert::TryInto;
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use uuid::Uuid;
use crate::api::{Config, Context};


#[derive(PartialEq, Debug)]
pub struct Request {
    pub params: Params,
    pub cmd: Method,
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

#[derive(Debug, PartialEq)]
pub enum Method {
    BuyXmr,
    History,
    Config,
    WithdrawBtc,
    Balance,
    Resume,
    Cancel,
    Refund,
    ListSellers,
    ExportBitcoinWallet,
    MoneroRecovery,
    StartDaemon,
}

impl Request {
    pub async fn call(&self, context: Arc<Context>) -> Result<serde_json::Value> {
        let result = match self.cmd {
            Method::BuyXmr => {
                let swap_id = Uuid::new_v4();

                let seed = context.config.seed.as_ref().unwrap();
                let env_config = context.config.env_config;
                let btc = context.bitcoin_wallet.as_ref().unwrap();
                let seller = self.params.seller.clone().unwrap();
                let monero_receive_address = self.params.monero_receive_address.unwrap();
                let bitcoin_change_address = self.params.bitcoin_change_address.clone().unwrap();

                let bitcoin_wallet = btc;
                let seller_peer_id = self
                    .params
                    .seller
                    .as_ref()
                    .unwrap()
                    .extract_peer_id()
                    .context("Seller address must contain peer ID")?;
                context
                    .db
                    .insert_address(seller_peer_id, seller.clone())
                    .await?;

                let behaviour = cli::Behaviour::new(
                    seller_peer_id,
                    env_config,
                    bitcoin_wallet.clone(),
                    (seed.derive_libp2p_identity(), context.config.namespace),
                );
                let mut swarm = swarm::cli(
                    seed.derive_libp2p_identity(),
                    context.config.tor_socks5_port.unwrap(),
                    behaviour,
                )
                .await?;
                swarm.behaviour_mut().add_address(seller_peer_id, seller);

                tracing::debug!(peer_id = %swarm.local_peer_id(), "Network layer initialized");

                let (event_loop, mut event_loop_handle) =
                    EventLoop::new(swap_id, swarm, seller_peer_id)?;
                let event_loop = tokio::spawn(event_loop.run());

                let max_givable = || bitcoin_wallet.max_giveable(TxLock::script_size());
                let estimate_fee = |amount| bitcoin_wallet.estimate_fee(TxLock::weight(), amount);

                let (amount, fees) = match determine_btc_to_swap(
                    context.config.json,
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
                context
                    .db
                    .insert_monero_address(swap_id, monero_receive_address)
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
            Method::History => {
                let swaps = context.db.all().await?;
                let mut vec: Vec<(Uuid, String)> = Vec::new();
                for (swap_id, state) in swaps {
                    let state: BobState = state.try_into()?;
                    vec.push((swap_id, state.to_string()));
                }
                json!({ "swaps": vec })
            }
            Method::Config => {
                //                tracing::info!(path=%data_dir.display(), "Data directory");
                //                tracing::info!(path=%format!("{}/logs", data_dir.display()),
                // "Log files directory");
                // tracing::info!(path=%format!("{}/sqlite", data_dir.display()), "Sqlite file
                // location");
                // tracing::info!(path=%format!("{}/seed.pem", data_dir.display()), "Seed file
                // location");
                // tracing::info!(path=%format!("{}/monero", data_dir.display()),
                // "Monero-wallet-rpc directory");
                // tracing::info!(path=%format!("{}/wallet", data_dir.display()), "Internal
                // bitcoin wallet directory");

                json!({
                    "result": []
                })
            }
            Method::WithdrawBtc => {
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

                bitcoin_wallet
                    .broadcast(signed_tx.clone(), "withdraw")
                    .await?;

                json!({
                    "signed_tx": signed_tx,
                    "amount": amount.as_sat(),
                    "txid": signed_tx.txid(),
                })
            }
            Method::StartDaemon => {
                let addr2 = "127.0.0.1:1234".parse()?;

                let server_handle = {
                    if let Some(addr) = context.config.server_address {
                        let (_addr, handle) = rpc::run_server(addr, context).await?;
                        Some(handle)
                    } else {
                        let (_addr, handle) = rpc::run_server(addr2, context).await?;
                        Some(handle)
                    }
                };
                loop {}
                json!({
                    "result": []
                })
            }
            Method::Balance => {
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
            Method::Resume => {
                let swap_id = self.params.swap_id.unwrap();

                let seller_peer_id = context.db.get_peer_id(swap_id).await?;
                let seller_addresses = context.db.get_addresses(seller_peer_id).await?;

                let seed = context.config.seed.as_ref().unwrap().derive_libp2p_identity();

                let behaviour = cli::Behaviour::new(
                    seller_peer_id,
                    context.config.env_config,
                    Arc::clone(context.bitcoin_wallet.as_ref().unwrap()),
                    (seed.clone(), context.config.namespace),
                );
                let mut swarm = swarm::cli(
                    seed.clone(),
                    context.config.tor_socks5_port.clone().unwrap(),
                    behaviour,
                )
                .await?;
                let our_peer_id = swarm.local_peer_id();

                tracing::debug!(peer_id = %our_peer_id, "Network layer initialized");

                for seller_address in seller_addresses {
                    swarm
                        .behaviour_mut()
                        .add_address(seller_peer_id, seller_address);
                }

                let (event_loop, event_loop_handle) =
                    EventLoop::new(swap_id, swarm, seller_peer_id)?;
                let handle = tokio::spawn(event_loop.run());

                let monero_receive_address = context.db.get_monero_address(swap_id).await?;
                let swap = Swap::from_db(
                    Arc::clone(&context.db),
                    swap_id,
                    Arc::clone(context.bitcoin_wallet.as_ref().unwrap()),
                    Arc::clone(context.monero_wallet.as_ref().unwrap()),
                    context.config.env_config,
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
            Method::Cancel => {
                let bitcoin_wallet = context.bitcoin_wallet.as_ref().unwrap();

                let (txid, _) = cli::cancel(
                    self.params.swap_id.unwrap(),
                    Arc::clone(bitcoin_wallet),
                    Arc::clone(&context.db),
                )
                .await?;

                tracing::debug!("Cancel transaction successfully published with id {}", txid);

                json!({
                    "txid": txid,
                })
            }
            Method::Refund => {
                let bitcoin_wallet = context.bitcoin_wallet.as_ref().unwrap();

                let state = cli::refund(
                    self.params.swap_id.unwrap(),
                    Arc::clone(bitcoin_wallet),
                    Arc::clone(&context.db),
                )
                .await?;

                json!({ "result": state })
            }
            Method::ListSellers => {
                let rendezvous_point = self.params.rendezvous_point.clone().unwrap();
                let rendezvous_node_peer_id = rendezvous_point
                    .extract_peer_id()
                    .context("Rendezvous node address must contain peer ID")?;

                let identity = context.config.seed.as_ref().unwrap().derive_libp2p_identity();

                let sellers = list_sellers(
                    rendezvous_node_peer_id,
                    rendezvous_point,
                    context.config.namespace,
                    context.config.tor_socks5_port.unwrap(),
                    identity,
                )
                .await?;

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

                json!({ "sellers": sellers })
            }
            Method::ExportBitcoinWallet => {
                let bitcoin_wallet = context.bitcoin_wallet.as_ref().unwrap();

                let wallet_export = bitcoin_wallet.wallet_export("cli").await?;
                tracing::info!(descriptor=%wallet_export.to_string(), "Exported bitcoin wallet");
                json!({
                    "result": []
                })
            }
            Method::MoneroRecovery => {
                let swap_state: BobState = context
                    .db
                    .get_state(self.params.swap_id.clone().unwrap())
                    .await?
                    .try_into()?;

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
                            context.config.env_config.monero_network,
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
