use crate::api::Context;
use crate::bitcoin::{Amount, TxLock};
use crate::cli::{list_sellers, EventLoop, SellerStatus};
use crate::libp2p_ext::MultiAddrExt;
use crate::network::quote::{BidQuote, ZeroQuoteReceived};
use crate::network::swarm;
use crate::protocol::bob;
use crate::protocol::bob::{BobState, Swap};
use crate::{bitcoin, cli, monero, rpc};
use anyhow::{bail, Context as AnyContext, Result};
use libp2p::core::Multiaddr;
use qrcode::render::unicode;
use qrcode::QrCode;
use serde_json::json;
use std::cmp::min;
use std::convert::TryInto;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use std::net::SocketAddr;
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(PartialEq, Debug)]
pub struct Request {
    pub params: Params,
    pub cmd: Method,
    pub shutdown: Shutdown,
}

impl Shutdown {
    pub fn new(notify: broadcast::Receiver<()>) -> Shutdown {
        Shutdown {
            shutdown: false,
            notify,
        }
    }

    /// Returns `true` if the shutdown signal has been received.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown
    }

    /// Receive the shutdown notice, waiting if necessary.
    pub async fn recv(&mut self) {
        // If the shutdown signal has already been received, then return
        // immediately.
        if self.shutdown {
            return;
        }

        // Cannot receive a "lag error" as only one value is ever sent.
        let _ = self.notify.recv().await;

        self.shutdown = true;

        // Remember that the signal has been received.
    }
}

#[derive(Debug)]
pub struct Shutdown {
    shutdown: bool,
    notify: broadcast::Receiver<()>,
}

impl PartialEq for Shutdown {
    fn eq(&self, other: &Shutdown) -> bool {
        self.shutdown == other.shutdown
    }
}

#[derive(Default, PartialEq, Debug)]
pub struct Params {
    pub seller: Option<Multiaddr>,
    pub bitcoin_change_address: Option<bitcoin::Address>,
    pub monero_receive_address: Option<monero::Address>,
    pub rendezvous_point: Option<Multiaddr>,
    pub swap_id: Option<Uuid>,
    pub amount: Option<Amount>,
    pub server_address: Option<SocketAddr>,
    pub address: Option<bitcoin::Address>,
}

#[derive(Debug, PartialEq)]
pub enum Method {
    BuyXmr,
    History,
    RawHistory,
    Config,
    WithdrawBtc,
    Balance,
    GetSeller,
    SwapStartDate,
    Resume,
    CancelAndRefund,
    ListSellers,
    ExportBitcoinWallet,
    MoneroRecovery,
    StartDaemon,
}

impl Request {
    pub async fn call(&mut self, context: Arc<Context>) -> Result<serde_json::Value> {
        let result = match self.cmd {
            Method::BuyXmr => {
                let swap_id = Uuid::new_v4();

                let seed = context.config.seed.as_ref().context("Could not get seed")?;
                let env_config = context.config.env_config;
                let btc = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;
                let seller = self
                    .params
                    .seller
                    .clone()
                    .context("Parameter seller is missing")?;
                let monero_receive_address = self
                    .params
                    .monero_receive_address
                    .context("Parameter monero_receive_address is missing")?;
                let bitcoin_change_address = self
                    .params
                    .bitcoin_change_address
                    .clone()
                    .context("Parameter bitcoin_change_address is missing")?;

                let bitcoin_wallet = btc;
                let seller_peer_id = seller
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
                    context
                        .config
                        .tor_socks5_port
                        .context("Could not get Tor SOCKS5 port")?,
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
                let monero_wallet = context
                    .monero_wallet
                    .as_ref()
                    .context("Could not get Monero wallet")?;

                let swap = Swap::new(
                    Arc::clone(&context.db),
                    swap_id,
                    Arc::clone(bitcoin_wallet),
                    Arc::clone(monero_wallet),
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
            Method::RawHistory => {
                let raw_history = context.db.raw_all().await?;
                json!({ "raw_history": raw_history })
            }
            Method::GetSeller => {
                let swap_id = self.params.swap_id.context("Parameter swap_id is needed")?;
                let peerId = context
                    .db
                    .get_peer_id(swap_id)
                    .await
                    .with_context(|| "Could not get PeerID")?;

                let addresses = context
                    .db
                    .get_addresses(peerId)
                    .await
                    .with_context(|| "Could not get addressess")?;

                json!({
                    "peerId": peerId.to_base58(),
                    "addresses": addresses
                })
            }
            Method::SwapStartDate => {
                let swap_id = self
                    .params
                    .swap_id
                    .context("Parameter swap_id is missing")?;

                let start_date = context.db.get_swap_start_date(swap_id).await?;

                json!({
                    "start_date": start_date,
                })
            }
            Method::Config => {
                let data_dir_display = context.config.data_dir.display();
                tracing::info!(path=%data_dir_display, "Data directory");
                tracing::info!(path=%format!("{}/logs", data_dir_display), "Log files directory");
                tracing::info!(path=%format!("{}/sqlite", data_dir_display), "Sqlite file location");
                tracing::info!(path=%format!("{}/seed.pem", data_dir_display), "Seed file location");
                tracing::info!(path=%format!("{}/monero", data_dir_display), "Monero-wallet-rpc directory");
                tracing::info!(path=%format!("{}/wallet", data_dir_display), "Internal bitcoin wallet directory");

                json!({
                    "log_files": format!("{}/logs", data_dir_display),
                    "sqlite": format!("{}/sqlite", data_dir_display),
                    "seed": format!("{}/seed.pem", data_dir_display),
                    "monero-wallet-rpc": format!("{}/monero", data_dir_display),
                    "bitcoin_wallet": format!("{}/wallet", data_dir_display),
                })
            }
            Method::WithdrawBtc => {
                let bitcoin_wallet = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

                let address = self
                    .params
                    .address
                    .clone()
                    .context("Parameter address is missing")?;

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
                    "amount": amount.to_sat(),
                    "txid": signed_tx.txid(),
                })
            }
            Method::StartDaemon => {
                let server_address = match self.params.server_address {
                    Some(address) => address,
                    None => {
                        "127.0.0.1:3456".parse()?
                    }
                };


                let (_, server_handle) = rpc::run_server(server_address, Arc::clone(&context)).await?;

                loop {
                    tokio::select! {
                        _ = self.shutdown.recv() => {
                            server_handle.stop()?;
                            return Ok(json!({
                                "result": []
                            }))
                        }
                    }
                }
            }
            Method::Balance => {
                let bitcoin_wallet = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

                bitcoin_wallet.sync().await?;
                let bitcoin_balance = bitcoin_wallet.balance().await?;
                tracing::info!(
                    balance = %bitcoin_balance,
                    "Checked Bitcoin balance",
                );

                json!({
                    "balance": bitcoin_balance.to_sat()
                })
            }
            Method::Resume => {
                let swap_id = self
                    .params
                    .swap_id
                    .context("Parameter swap_id is missing")?;

                let seller_peer_id = context.db.get_peer_id(swap_id).await?;
                let seller_addresses = context.db.get_addresses(seller_peer_id).await?;

                let seed = context
                    .config
                    .seed
                    .as_ref()
                    .context("Could not get seed")?
                    .derive_libp2p_identity();

                let behaviour = cli::Behaviour::new(
                    seller_peer_id,
                    context.config.env_config,
                    Arc::clone(
                        context
                            .bitcoin_wallet
                            .as_ref()
                            .context("Could not get Bitcoin wallet")?,
                    ),
                    (seed.clone(), context.config.namespace),
                );
                let mut swarm = swarm::cli(
                    seed.clone(),
                    context
                        .config
                        .tor_socks5_port
                        .context("Could not get Tor SOCKS5 port")?,
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
                    Arc::clone(
                        context
                            .bitcoin_wallet
                            .as_ref()
                            .context("Could not get Bitcoin wallet")?,
                    ),
                    Arc::clone(
                        context
                            .monero_wallet
                            .as_ref()
                            .context("Could not get Monero wallet")?,
                    ),
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
            Method::CancelAndRefund => {
                let bitcoin_wallet = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

                let state = cli::cancel_and_refund(
                    self.params
                        .swap_id
                        .context("Parameter swap_id is missing")?,
                    Arc::clone(bitcoin_wallet),
                    Arc::clone(&context.db),
                )
                .await?;

                json!({
                    "result": state,
                })
            }
            Method::ListSellers => {
                let rendezvous_point = self
                    .params
                    .rendezvous_point
                    .clone()
                    .context("Parameter rendezvous_point is missing")?;
                let rendezvous_node_peer_id = rendezvous_point
                    .extract_peer_id()
                    .context("Rendezvous node address must contain peer ID")?;

                let identity = context
                    .config
                    .seed
                    .as_ref()
                    .context("Cannot extract seed")?
                    .derive_libp2p_identity();

                let sellers = list_sellers(
                    rendezvous_node_peer_id,
                    rendezvous_point,
                    context.config.namespace,
                    context
                        .config
                        .tor_socks5_port
                        .context("Could not get Tor SOCKS5 port")?,
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
                let bitcoin_wallet = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

                let wallet_export = bitcoin_wallet.wallet_export("cli").await?;
                tracing::info!(descriptor=%wallet_export.to_string(), "Exported bitcoin wallet");
                json!({
                    "result": []
                })
            }
            Method::MoneroRecovery => {
                let swap_state: BobState = context
                    .db
                    .get_state(
                        self.params
                            .swap_id
                            .context("Parameter swap_id is missing")?,
                    )
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
