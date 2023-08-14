use crate::api::Context;
use crate::bitcoin::{Amount, ExpiredTimelocks, TxLock};
use crate::cli::{list_sellers, EventLoop, SellerStatus};
use crate::libp2p_ext::MultiAddrExt;
use crate::network::quote::{BidQuote, ZeroQuoteReceived};
use crate::network::swarm;
use crate::protocol::bob;
use crate::protocol::bob::swap::is_complete;
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
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use structopt::lazy_static::lazy_static;
use tokio::sync::broadcast::Receiver;
use tokio::sync::RwLock;
use tracing::{debug_span, Instrument};
use uuid::Uuid;

lazy_static! {
    static ref SWAP_LOCK: RwLock<Option<Uuid>> = RwLock::new(None);
}

#[derive(PartialEq, Debug)]
pub struct Request {
    pub cmd: Method,
    pub shutdown: Shutdown,
}

impl Shutdown {
    pub fn new(notify: Receiver<()>) -> Shutdown {
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
    notify: Receiver<()>,
}

impl PartialEq for Shutdown {
    fn eq(&self, other: &Shutdown) -> bool {
        self.shutdown == other.shutdown
    }
}

#[derive(Debug, PartialEq)]
pub enum Method {
    BuyXmr {
        seller: Multiaddr,
        bitcoin_change_address: bitcoin::Address,
        monero_receive_address: monero::Address,
        swap_id: Uuid,
    },
    History,
    RawHistory,
    Config,
    WithdrawBtc {
        amount: Option<Amount>,
        address: bitcoin::Address,
    },
    Balance,
    Resume {
        swap_id: Uuid,
    },
    CancelAndRefund {
        swap_id: Uuid,
    },
    ListSellers {
        rendezvous_point: Multiaddr,
    },
    ExportBitcoinWallet,
    MoneroRecovery {
        swap_id: Uuid,
    },
    StartDaemon {
        server_address: Option<SocketAddr>,
    },
    GetCurrentSwap,
    GetSwapInfo {
        swap_id: Uuid,
    },
}

impl Request {
    pub fn new(shutdownReceiver: Receiver<()>, cmd: Method) -> Request {
        Request {
            cmd,
            shutdown: Shutdown::new(shutdownReceiver),
        }
    }

    fn has_lockable_swap_id(&self) -> Option<Uuid> {
        match self.cmd {
            Method::BuyXmr { swap_id, .. }
            | Method::Resume { swap_id }
            | Method::CancelAndRefund { swap_id } => Some(swap_id),
            _ => None,
        }
    }

    async fn handle_cmd(mut self, context: Arc<Context>) -> Result<serde_json::Value> {
        match self.cmd {
            Method::GetSwapInfo { swap_id } => {
                let bitcoin_wallet = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

                let swap_state: BobState = context.db.get_state(swap_id).await?.try_into()?;

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

                let is_completed = is_complete(&swap_state);

                let start_date = context.db.get_swap_start_date(swap_id).await?;

                let state_name = format!("{:?}", swap_state);

                // variable timelock: Option<Result<ExpiredTimelocks>>
                let timelock = match swap_state {
                    BobState::Started { .. }
                    | BobState::SafelyAborted
                    | BobState::SwapSetupCompleted(_) => None,
                    BobState::BtcLocked { state3: state, .. }
                    | BobState::XmrLockProofReceived { state, .. } => {
                        Some(state.expired_timelock(bitcoin_wallet).await)
                    }
                    BobState::XmrLocked(state) | BobState::EncSigSent(state) => {
                        Some(state.expired_timelock(bitcoin_wallet).await)
                    }
                    BobState::CancelTimelockExpired(state) | BobState::BtcCancelled(state) => {
                        Some(state.expired_timelock(bitcoin_wallet).await)
                    }
                    BobState::BtcPunished { .. } => Some(Ok(ExpiredTimelocks::Punish)),
                    // swap is already finished
                    BobState::BtcRefunded(_)
                    | BobState::BtcRedeemed(_)
                    | BobState::XmrRedeemed { .. } => None,
                };

                // Add txids
                Ok(json!({
                    "seller": {
                        "peerId": peerId.to_string(),
                        "addresses": addresses
                    },
                    "completed": is_completed,
                    "startDate": start_date,
                    // If none return null, if some unwrap and return as json
                    "timelock": timelock.map(|tl| tl.map(|tl| json!(tl)).unwrap_or(json!(null))).unwrap_or(json!(null)),
                    "stateName": state_name,
                }))
            }
            Method::BuyXmr {
                seller,
                bitcoin_change_address,
                monero_receive_address,
                swap_id,
            } => {
                let seed = context.config.seed.as_ref().context("Could not get seed")?;
                let env_config = context.config.env_config;
                let btc = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

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
                Ok(json!({
                    "empty": "true"
                }))
            }
            Method::History => {
                let swaps = context.db.all().await?;
                let mut vec: Vec<(Uuid, String)> = Vec::new();
                for (swap_id, state) in swaps {
                    let state: BobState = state.try_into()?;
                    vec.push((swap_id, state.to_string()));
                }

                Ok(json!({ "swaps": vec }))
            }
            Method::RawHistory => {
                let raw_history = context.db.raw_all().await?;
                Ok(json!({ "raw_history": raw_history }))
            }
            Method::Config => {
                let data_dir_display = context.config.data_dir.display();
                tracing::info!(path=%data_dir_display, "Data directory");
                tracing::info!(path=%format!("{}/logs", data_dir_display), "Log files directory");
                tracing::info!(path=%format!("{}/sqlite", data_dir_display), "Sqlite file location");
                tracing::info!(path=%format!("{}/seed.pem", data_dir_display), "Seed file location");
                tracing::info!(path=%format!("{}/monero", data_dir_display), "Monero-wallet-rpc directory");
                tracing::info!(path=%format!("{}/wallet", data_dir_display), "Internal bitcoin wallet directory");

                Ok(json!({
                    "log_files": format!("{}/logs", data_dir_display),
                    "sqlite": format!("{}/sqlite", data_dir_display),
                    "seed": format!("{}/seed.pem", data_dir_display),
                    "monero-wallet-rpc": format!("{}/monero", data_dir_display),
                    "bitcoin_wallet": format!("{}/wallet", data_dir_display),
                }))
            }
            Method::WithdrawBtc { address, amount } => {
                let bitcoin_wallet = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

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

                bitcoin_wallet
                    .broadcast(signed_tx.clone(), "withdraw")
                    .await?;

                Ok(json!({
                    "signed_tx": signed_tx,
                    "amount": amount.to_sat(),
                    "txid": signed_tx.txid(),
                }))
            }
            Method::StartDaemon { server_address } => {
                // Default to 127.0.0.1:1234
                let server_address = server_address.unwrap_or("127.0.0.1:1234".parse().unwrap());

                let (_, server_handle) =
                    rpc::run_server(server_address, Arc::clone(&context)).await?;

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

                Ok(json!({
                    "balance": bitcoin_balance.to_sat()
                }))
            }
            Method::Resume { swap_id } => {
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
                Ok(json!({
                    "result": []
                }))
            }
            Method::CancelAndRefund { swap_id } => {
                let bitcoin_wallet = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

                let state = cli::cancel_and_refund(
                    swap_id,
                    Arc::clone(bitcoin_wallet),
                    Arc::clone(&context.db),
                )
                .await?;

                Ok(json!({
                    "result": state,
                }))
            }
            Method::ListSellers { rendezvous_point } => {
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

                Ok(json!({ "sellers": sellers }))
            }
            Method::ExportBitcoinWallet => {
                let bitcoin_wallet = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

                let wallet_export = bitcoin_wallet.wallet_export("cli").await?;
                tracing::info!(descriptor=%wallet_export.to_string(), "Exported bitcoin wallet");
                Ok(json!({
                    "descriptor": wallet_export.to_string(),
                }))
            }
            Method::MoneroRecovery { swap_id } => {
                let swap_state: BobState = context.db.get_state(swap_id).await?.try_into()?;

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
                Ok(json!({
                    "result": []
                }))
            }
            Method::GetCurrentSwap => Ok(json!({
                "swap_id": SWAP_LOCK.read().await.clone()
            })),
        }
    }

    pub async fn call(self, context: Arc<Context>) -> Result<serde_json::Value> {
        // If the swap ID is set, we add it to the span
        let call_span = debug_span!(
            "cmd",
            method = ?self.cmd,
        );

        if let Some(swap_id) = self.has_lockable_swap_id() {
            println!("taking lock for swap_id: {}", swap_id);
            let mut guard = SWAP_LOCK.write().await;
            if let Some(running_swap_id) = guard.as_ref() {
                bail!("Another swap is already running: {}", running_swap_id);
            }
            let _ = guard.insert(swap_id.clone());
            drop(guard);

            let result = self.handle_cmd(context).instrument(call_span).await;

            SWAP_LOCK.write().await.take();

            println!("releasing lock for swap_id: {}", swap_id);

            return result;
        }
        self.handle_cmd(context).instrument(call_span).await
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
