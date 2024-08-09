use crate::api::Context;
use crate::bitcoin::{Amount, ExpiredTimelocks, TxLock};
use crate::cli::{list_sellers as list_sellers_impl, EventLoop, SellerStatus};
use crate::libp2p_ext::MultiAddrExt;
use crate::network::quote::{BidQuote, ZeroQuoteReceived};
use crate::network::swarm;
use crate::protocol::bob::{BobState, Swap};
use crate::protocol::{bob, State};
use crate::{bitcoin, cli, monero, rpc};
use ::bitcoin::Txid;
use anyhow::{bail, Context as AnyContext, Result};
use libp2p::core::Multiaddr;
use qrcode::render::unicode;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value as JsonValue;
use std::cmp::min;
use std::convert::TryInto;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug_span, field, Instrument, Span};
use uuid::Uuid;

#[derive(PartialEq, Debug)]
pub struct Request {
    pub cmd: Method,
    pub log_reference: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct BuyXmrArgs {
    pub seller: Multiaddr,
    pub bitcoin_change_address: bitcoin::Address,
    pub monero_receive_address: monero::Address,
    pub swap_id: Uuid,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ResumeArgs {
    pub swap_id: Uuid,
}

#[derive(Debug, Eq, PartialEq)]
pub struct CancelAndRefundArgs {
    pub swap_id: Uuid,
}

#[derive(Debug, Eq, PartialEq)]
pub struct MoneroRecoveryArgs {
    pub swap_id: Uuid,
}

#[derive(Debug, Eq, PartialEq)]
pub struct WithdrawBtcArgs {
    pub amount: Option<Amount>,
    pub address: bitcoin::Address,
}

#[derive(Debug, Eq, PartialEq)]
pub struct BalanceArgs {
    pub force_refresh: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ListSellersArgs {
    pub rendezvous_point: Multiaddr,
}

#[derive(Debug, Eq, PartialEq)]
pub struct StartDaemonArgs {
    pub server_address: Option<SocketAddr>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct GetSwapInfoArgs {
    pub swap_id: Uuid,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ResumeSwapResponse {
    pub result: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BalanceResponse {
    pub balance: u64, // in satoshis
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BuyXmrResponse {
    pub swap_id: String,
    pub quote: BidQuote, // You'll need to import or define BidQuote
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetHistoryResponse {
    swaps: Vec<(Uuid, String)>,
}

#[derive(Serialize)]
pub struct GetSwapInfoResponse {
    pub swap_id: Uuid,
    pub seller: Seller,
    pub completed: bool,
    pub start_date: String,
    pub state_name: String,
    pub xmr_amount: u64,
    pub btc_amount: u64,
    pub tx_lock_id: Txid,
    pub tx_cancel_fee: u64,
    pub tx_refund_fee: u64,
    pub tx_lock_fee: u64,
    pub btc_refund_address: String,
    pub cancel_timelock: u32,
    pub punish_timelock: u32,
    pub timelock: Option<ExpiredTimelocks>,
}

#[derive(Serialize, Deserialize)]
pub struct Seller {
    pub peer_id: String,
    pub addresses: Vec<Multiaddr>,
}

// TODO: We probably dont even need this.
// We can just call the method directly from the RPC server, the CLI and the Tauri connector
#[derive(Debug, PartialEq)]
pub enum Method {
    BuyXmr(BuyXmrArgs),
    Resume(ResumeArgs),
    CancelAndRefund(CancelAndRefundArgs),
    MoneroRecovery(MoneroRecoveryArgs),
    History,
    Config,
    WithdrawBtc(WithdrawBtcArgs),
    Balance(BalanceArgs),
    ListSellers(ListSellersArgs),
    ExportBitcoinWallet,
    SuspendCurrentSwap,
    StartDaemon(StartDaemonArgs),
    GetCurrentSwap,
    GetSwapInfo(GetSwapInfoArgs),
    GetRawStates,
}

#[tracing::instrument(fields(method = "suspend_current_swap"), skip(context))]
pub async fn suspend_current_swap(context: Arc<Context>) -> Result<serde_json::Value> {
    let swap_id = context.swap_lock.get_current_swap_id().await;

    if let Some(id_value) = swap_id {
        context.swap_lock.send_suspend_signal().await?;

        Ok(json!({ "swapId": id_value }))
    } else {
        bail!("No swap is currently running")
    }
}

#[tracing::instrument(fields(method = "get_swap_infos_all"), skip(context))]
pub async fn get_swap_infos_all(context: Arc<Context>) -> Result<Vec<GetSwapInfoResponse>> {
    let swap_ids = context.db.all().await?;
    let mut swap_infos = Vec::new();

    for (swap_id, _) in swap_ids {
        let swap_info = get_swap_info(GetSwapInfoArgs { swap_id }, context.clone()).await?;
        swap_infos.push(swap_info);
    }

    Ok(swap_infos)
}

#[tracing::instrument(fields(method = "get_swap_info"), skip(context))]
pub async fn get_swap_info(
    args: GetSwapInfoArgs,
    context: Arc<Context>,
) -> Result<GetSwapInfoResponse> {
    let bitcoin_wallet = context
        .bitcoin_wallet
        .as_ref()
        .context("Could not get Bitcoin wallet")?;

    let state = context.db.get_state(args.swap_id).await?;
    let is_completed = state.swap_finished();

    let peerId = context
        .db
        .get_peer_id(args.swap_id)
        .await
        .with_context(|| "Could not get PeerID")?;

    let addresses = context
        .db
        .get_addresses(peerId)
        .await
        .with_context(|| "Could not get addressess")?;

    let start_date = context.db.get_swap_start_date(args.swap_id).await?;

    let swap_state: BobState = state.try_into()?;
    let state_name = format!("{}", swap_state);

    let (
        xmr_amount,
        btc_amount,
        tx_lock_id,
        tx_cancel_fee,
        tx_refund_fee,
        tx_lock_fee,
        btc_refund_address,
        cancel_timelock,
        punish_timelock,
    ) = context
        .db
        .get_states(args.swap_id)
        .await?
        .iter()
        .find_map(|state| {
            if let State::Bob(BobState::SwapSetupCompleted(state2)) = state {
                let xmr_amount = state2.xmr;
                let btc_amount = state2.tx_lock.lock_amount().to_sat();
                let tx_cancel_fee = state2.tx_cancel_fee.to_sat();
                let tx_refund_fee = state2.tx_refund_fee.to_sat();
                let tx_lock_id = state2.tx_lock.txid();
                let btc_refund_address = state2.refund_address.to_string();

                if let Ok(tx_lock_fee) = state2.tx_lock.fee() {
                    let tx_lock_fee = tx_lock_fee.to_sat();

                    Some((
                        xmr_amount,
                        btc_amount,
                        tx_lock_id,
                        tx_cancel_fee,
                        tx_refund_fee,
                        tx_lock_fee,
                        btc_refund_address,
                        state2.cancel_timelock,
                        state2.punish_timelock,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .with_context(|| "Did not find SwapSetupCompleted state for swap")?;

    let timelock = match swap_state {
        BobState::Started { .. } | BobState::SafelyAborted | BobState::SwapSetupCompleted(_) => {
            None
        }
        BobState::BtcLocked { state3: state, .. }
        | BobState::XmrLockProofReceived { state, .. } => {
            Some(state.expired_timelock(bitcoin_wallet).await?)
        }
        BobState::XmrLocked(state) | BobState::EncSigSent(state) => {
            Some(state.expired_timelock(bitcoin_wallet).await?)
        }
        BobState::CancelTimelockExpired(state) | BobState::BtcCancelled(state) => {
            Some(state.expired_timelock(bitcoin_wallet).await?)
        }
        BobState::BtcPunished { .. } => Some(ExpiredTimelocks::Punish),
        BobState::BtcRefunded(_) | BobState::BtcRedeemed(_) | BobState::XmrRedeemed { .. } => None,
    };

    Ok(GetSwapInfoResponse {
        swap_id: args.swap_id,
        seller: Seller {
            peer_id: peerId.to_string(),
            addresses,
        },
        completed: is_completed,
        start_date,
        state_name,
        xmr_amount: xmr_amount.as_piconero(),
        btc_amount,
        tx_lock_id,
        tx_cancel_fee,
        tx_refund_fee,
        tx_lock_fee,
        btc_refund_address: btc_refund_address.to_string(),
        cancel_timelock: cancel_timelock.into(),
        punish_timelock: punish_timelock.into(),
        timelock,
    })
}

#[tracing::instrument(fields(method = "buy_xmr"), skip(context))]
pub async fn buy_xmr(
    buy_xmr: BuyXmrArgs,
    context: Arc<Context>,
) -> Result<serde_json::Value, anyhow::Error> {
    let BuyXmrArgs {
        seller,
        bitcoin_change_address,
        monero_receive_address,
        swap_id,
    } = buy_xmr;
    let bitcoin_wallet = Arc::clone(
        context
            .bitcoin_wallet
            .as_ref()
            .expect("Could not find Bitcoin wallet"),
    );
    let monero_wallet = Arc::clone(
        context
            .monero_wallet
            .as_ref()
            .context("Could not get Monero wallet")?,
    );
    let env_config = context.config.env_config;
    let seed = context.config.seed.clone().context("Could not get seed")?;

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
        context.config.tor_socks5_port,
        behaviour,
    )
    .await?;

    swarm.behaviour_mut().add_address(seller_peer_id, seller);

    context
        .db
        .insert_monero_address(swap_id, monero_receive_address)
        .await?;

    tracing::debug!(peer_id = %swarm.local_peer_id(), "Network layer initialized");

    context.swap_lock.acquire_swap_lock(swap_id).await?;

    let initialize_swap = tokio::select! {
        biased;
        _ = context.swap_lock.listen_for_swap_force_suspension() => {
            tracing::debug!("Shutdown signal received, exiting");
            context.swap_lock.release_swap_lock().await.expect("Shutdown signal received but failed to release swap lock. The swap process has been terminated but the swap lock is still active.");
            bail!("Shutdown signal received");
        },
        result = async {
            let (event_loop, mut event_loop_handle) =
                EventLoop::new(swap_id, swarm, seller_peer_id, context.db.clone())?;
            let event_loop = tokio::spawn(event_loop.run().in_current_span());

            let bid_quote = event_loop_handle.request_quote().await?;

            Ok::<_, anyhow::Error>((event_loop, event_loop_handle, bid_quote))
        } => {
            result
        },
    };

    let (event_loop, event_loop_handle, bid_quote) = match initialize_swap {
        Ok(result) => result,
        Err(error) => {
            tracing::error!(%swap_id, "Swap initialization failed: {:#}", error);
            context
                .swap_lock
                .release_swap_lock()
                .await
                .expect("Could not release swap lock");
            bail!(error);
        }
    };

    context.tasks.clone().spawn(async move {
        tokio::select! {
            biased;
            _ = context.swap_lock.listen_for_swap_force_suspension() => {
                tracing::debug!("Shutdown signal received, exiting");
                context.swap_lock.release_swap_lock().await.expect("Shutdown signal received but failed to release swap lock. The swap process has been terminated but the swap lock is still active.");
                bail!("Shutdown signal received");
            },
            event_loop_result = event_loop => {
                match event_loop_result {
                    Ok(_) => {
                        tracing::debug!(%swap_id, "EventLoop completed")
                    }
                    Err(error) => {
                        tracing::error!(%swap_id, "EventLoop failed: {:#}", error)
                    }
                }
            },
            swap_result = async {
                let max_givable = || bitcoin_wallet.max_giveable(TxLock::script_size());
                let estimate_fee = |amount| bitcoin_wallet.estimate_fee(TxLock::weight(), amount);

                let determine_amount = determine_btc_to_swap(
                    context.config.json,
                    bid_quote,
                    bitcoin_wallet.new_address(),
                    || bitcoin_wallet.balance(),
                    max_givable,
                    || bitcoin_wallet.sync(),
                    estimate_fee,
                );

                let (amount, fees) = match determine_amount.await {
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

                let swap = Swap::new(
                    Arc::clone(&context.db),
                    swap_id,
                    Arc::clone(&bitcoin_wallet),
                    monero_wallet,
                    env_config,
                    event_loop_handle,
                    monero_receive_address,
                    bitcoin_change_address,
                    amount,
                );

                bob::run(swap).await
            } => {
                match swap_result {
                    Ok(state) => {
                        tracing::debug!(%swap_id, state=%state, "Swap completed")
                    }
                    Err(error) => {
                        tracing::error!(%swap_id, "Failed to complete swap: {:#}", error)
                    }
                }
            },
        };
        tracing::debug!(%swap_id, "Swap completed");

        context
            .swap_lock
            .release_swap_lock()
            .await
            .expect("Could not release swap lock");
        Ok::<_, anyhow::Error>(())
    }.in_current_span()).await;

    Ok(json!({
        "swapId": swap_id.to_string(),
        "quote": bid_quote,
    }))
}

#[tracing::instrument(fields(method = "resume_swap"), skip(context))]
pub async fn resume_swap(resume: ResumeArgs, context: Arc<Context>) -> Result<serde_json::Value> {
    let ResumeArgs { swap_id } = resume;
    context.swap_lock.acquire_swap_lock(swap_id).await?;

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
    let mut swarm = swarm::cli(seed.clone(), context.config.tor_socks5_port, behaviour).await?;
    let our_peer_id = swarm.local_peer_id();

    tracing::debug!(peer_id = %our_peer_id, "Network layer initialized");

    for seller_address in seller_addresses {
        swarm
            .behaviour_mut()
            .add_address(seller_peer_id, seller_address);
    }

    let (event_loop, event_loop_handle) =
        EventLoop::new(swap_id, swarm, seller_peer_id, context.db.clone())?;
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

    context.tasks.clone().spawn(
        async move {
            let handle = tokio::spawn(event_loop.run().in_current_span());
            tokio::select! {
                biased;
                _ = context.swap_lock.listen_for_swap_force_suspension() => {
                     tracing::debug!("Shutdown signal received, exiting");
                    context.swap_lock.release_swap_lock().await.expect("Shutdown signal received but failed to release swap lock. The swap process has been terminated but the swap lock is still active.");
                    bail!("Shutdown signal received");
                },

                event_loop_result = handle => {
                    match event_loop_result {
                        Ok(_) => {
                            tracing::debug!(%swap_id, "EventLoop completed during swap resume")
                        }
                        Err(error) => {
                            tracing::error!(%swap_id, "EventLoop failed during swap resume: {:#}", error)
                        }
                    }
                },
                swap_result = bob::run(swap) => {
                    match swap_result {
                        Ok(state) => {
                            tracing::debug!(%swap_id, state=%state, "Swap completed after resuming")
                        }
                        Err(error) => {
                            tracing::error!(%swap_id, "Failed to resume swap: {:#}", error)
                        }
                    }

                }
            }
            context
                .swap_lock
                .release_swap_lock()
                .await
                .expect("Could not release swap lock");
            Ok::<(), anyhow::Error>(())
        }
        .in_current_span(),
    ).await;
    Ok(json!({
        "result": "ok",
    }))
}

#[tracing::instrument(fields(method = "cancel_and_refund"), skip(context))]
pub async fn cancel_and_refund(
    cancel_and_refund: CancelAndRefundArgs,
    context: Arc<Context>,
) -> Result<serde_json::Value> {
    let CancelAndRefundArgs { swap_id } = cancel_and_refund;
    let bitcoin_wallet = context
        .bitcoin_wallet
        .as_ref()
        .context("Could not get Bitcoin wallet")?;

    context.swap_lock.acquire_swap_lock(swap_id).await?;

    let state =
        cli::cancel_and_refund(swap_id, Arc::clone(bitcoin_wallet), Arc::clone(&context.db)).await;

    context
        .swap_lock
        .release_swap_lock()
        .await
        .expect("Could not release swap lock");

    state.map(|state| {
        json!({
            "result": state,
        })
    })
}

#[tracing::instrument(fields(method = "get_history"), skip(context))]
pub async fn get_history(context: Arc<Context>) -> Result<GetHistoryResponse> {
    let swaps = context.db.all().await?;
    let mut vec: Vec<(Uuid, String)> = Vec::new();
    for (swap_id, state) in swaps {
        let state: BobState = state.try_into()?;
        vec.push((swap_id, state.to_string()));
    }

    Ok(GetHistoryResponse { swaps: vec })
}

#[tracing::instrument(fields(method = "get_raw_states"), skip(context))]
pub async fn get_raw_states(context: Arc<Context>) -> Result<serde_json::Value> {
    let raw_history = context.db.raw_all().await?;

    Ok(json!({ "raw_states": raw_history }))
}

#[tracing::instrument(fields(method = "get_config"), skip(context))]
pub async fn get_config(context: Arc<Context>) -> Result<serde_json::Value> {
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

#[tracing::instrument(fields(method = "withdraw_btc"), skip(context))]
pub async fn withdraw_btc(
    withdraw_btc: WithdrawBtcArgs,
    context: Arc<Context>,
) -> Result<serde_json::Value> {
    let WithdrawBtcArgs { address, amount } = withdraw_btc;
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

#[tracing::instrument(fields(method = "start_daemon"), skip(context))]
pub async fn start_daemon(
    start_daemon: StartDaemonArgs,
    context: Arc<Context>,
) -> Result<serde_json::Value> {
    let StartDaemonArgs { server_address } = start_daemon;
    // Default to 127.0.0.1:1234
    let server_address = server_address.unwrap_or("127.0.0.1:1234".parse()?);

    let (addr, server_handle) = rpc::run_server(server_address, context).await?;

    tracing::info!(%addr, "Started RPC server");

    server_handle.stopped().await;

    tracing::info!("Stopped RPC server");

    Ok(json!({}))
}

#[tracing::instrument(fields(method = "get_balance"), skip(context))]
pub async fn get_balance(balance: BalanceArgs, context: Arc<Context>) -> Result<BalanceResponse> {
    let BalanceArgs { force_refresh } = balance;
    let bitcoin_wallet = context
        .bitcoin_wallet
        .as_ref()
        .context("Could not get Bitcoin wallet")?;

    if force_refresh {
        bitcoin_wallet.sync().await?;
    }

    let bitcoin_balance = bitcoin_wallet.balance().await?;

    if force_refresh {
        tracing::info!(
            balance = %bitcoin_balance,
            "Checked Bitcoin balance",
        );
    } else {
        tracing::debug!(
            balance = %bitcoin_balance,
            "Current Bitcoin balance as of last sync",
        );
    }

    Ok(BalanceResponse {
        balance: bitcoin_balance.to_sat(),
    })
}

#[tracing::instrument(fields(method = "list_sellers"), skip(context))]
pub async fn list_sellers(
    list_sellers: ListSellersArgs,
    context: Arc<Context>,
) -> Result<serde_json::Value> {
    let ListSellersArgs { rendezvous_point } = list_sellers;
    let rendezvous_node_peer_id = rendezvous_point
        .extract_peer_id()
        .context("Rendezvous node address must contain peer ID")?;

    let identity = context
        .config
        .seed
        .as_ref()
        .context("Cannot extract seed")?
        .derive_libp2p_identity();

    let sellers = list_sellers_impl(
        rendezvous_node_peer_id,
        rendezvous_point,
        context.config.namespace,
        context.config.tor_socks5_port,
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

#[tracing::instrument(fields(method = "export_bitcoin_wallet"), skip(context))]
pub async fn export_bitcoin_wallet(context: Arc<Context>) -> Result<serde_json::Value> {
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

#[tracing::instrument(fields(method = "monero_recovery"), skip(context))]
pub async fn monero_recovery(
    monero_recovery: MoneroRecoveryArgs,
    context: Arc<Context>,
) -> Result<serde_json::Value> {
    let MoneroRecoveryArgs { swap_id } = monero_recovery;
    let swap_state: BobState = context.db.get_state(swap_id).await?.try_into()?;

    if let BobState::BtcRedeemed(state5) = swap_state {
        let (spend_key, view_key) = state5.xmr_keys();
        let restore_height = state5.monero_wallet_restore_blockheight.height;

        let address = monero::Address::standard(
            context.config.env_config.monero_network,
            monero::PublicKey::from_private_key(&spend_key),
            monero::PublicKey::from(view_key.public()),
        );

        tracing::info!(restore_height=%restore_height, address=%address, spend_key=%spend_key, view_key=%view_key, "Monero recovery information");

        Ok(json!({
            "address": address,
            "spend_key": spend_key.to_string(),
            "view_key": view_key.to_string(),
            "restore_height": state5.monero_wallet_restore_blockheight.height,
        }))
    } else {
        bail!(
            "Cannot print monero recovery information in state {}, only possible for BtcRedeemed",
            swap_state
        )
    }
}

#[tracing::instrument(fields(method = "get_current_swap"), skip(context))]
pub async fn get_current_swap(context: Arc<Context>) -> Result<serde_json::Value> {
    Ok(json!({
        "swap_id": context.swap_lock.get_current_swap_id().await
    }))
}

impl Request {
    pub fn new(cmd: Method) -> Request {
        Request {
            cmd,
            log_reference: None,
        }
    }

    pub fn with_id(cmd: Method, id: Option<String>) -> Request {
        Request {
            cmd,
            log_reference: id,
        }
    }

    async fn handle_cmd(self, context: Arc<Context>) -> Result<Box<dyn erased_serde::Serialize>> {
        match self.cmd {
            Method::Balance(args) => {
                let response = get_balance(args, context).await?;
                Ok(Box::new(response) as Box<dyn erased_serde::Serialize>)
            }
            _ => todo!(),
        }
    }

    pub async fn call(self, context: Arc<Context>) -> Result<JsonValue> {
        unreachable!("This function should never be called")
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
    bid_quote: BidQuote,
    get_new_address: impl Future<Output = Result<bitcoin::Address>>,
    balance: FB,
    max_giveable_fn: FMG,
    sync: FS,
    estimate_fee: FFE,
) -> Result<(Amount, Amount)>
where
    TB: Future<Output = Result<Amount>>,
    FB: Fn() -> TB,
    TMG: Future<Output = Result<Amount>>,
    FMG: Fn() -> TMG,
    TS: Future<Output = Result<()>>,
    FS: Fn() -> TS,
    FFE: Fn(Amount) -> TFE,
    TFE: Future<Output = Result<Amount>>,
{
    if bid_quote.max_quantity == Amount::ZERO {
        bail!(ZeroQuoteReceived)
    }

    tracing::info!(
        price = %bid_quote.price,
        minimum_amount = %bid_quote.min_quantity,
        maximum_amount = %bid_quote.max_quantity,
        "Received quote",
    );

    sync().await?;
    let mut max_giveable = max_giveable_fn().await?;

    if max_giveable == Amount::ZERO || max_giveable < bid_quote.min_quantity {
        let deposit_address = get_new_address.await?;
        let minimum_amount = bid_quote.min_quantity;
        let maximum_amount = bid_quote.max_quantity;

        if !json {
            eprintln!("{}", qr_code(&deposit_address)?);
        }

        loop {
            let min_outstanding = bid_quote.min_quantity - max_giveable;
            let min_bitcoin_lock_tx_fee = estimate_fee(min_outstanding).await?;
            let min_deposit_until_swap_will_start = min_outstanding + min_bitcoin_lock_tx_fee;
            let max_deposit_until_maximum_amount_is_reached =
                maximum_amount - max_giveable + min_bitcoin_lock_tx_fee;

            tracing::info!(
                "Deposit at least {} to cover the min quantity with fee!",
                min_deposit_until_swap_will_start
            );
            tracing::info!(
                %deposit_address,
                %min_deposit_until_swap_will_start,
                %max_deposit_until_maximum_amount_is_reached,
                %max_giveable,
                %minimum_amount,
                %maximum_amount,
                %min_bitcoin_lock_tx_fee,
                price = %bid_quote.price,
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
                tracing::info!("Deposited amount is not enough to cover `min_quantity` when accounting for network fees");
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
