use super::tauri_bindings::TauriHandle;
use crate::bitcoin::{wallet, CancelTimelock, ExpiredTimelocks, PunishTimelock, TxLock};
use crate::cli::api::tauri_bindings::{TauriEmitter, TauriSwapProgressEvent};
use crate::cli::api::Context;
use crate::cli::list_sellers::{QuoteWithAddress, UnreachableSeller};
use crate::cli::{list_sellers as list_sellers_impl, EventLoop, SellerStatus};
use crate::common::{get_logs, redact};
use crate::libp2p_ext::MultiAddrExt;
use crate::monero::wallet_rpc::MoneroDaemon;
use crate::network::quote::{BidQuote, ZeroQuoteReceived};
use crate::network::swarm;
use crate::protocol::bob::{BobState, Swap};
use crate::protocol::{bob, State};
use crate::{bitcoin, cli, monero};
use ::bitcoin::address::NetworkUnchecked;
use ::bitcoin::Txid;
use ::monero::Network;
use anyhow::{bail, Context as AnyContext, Result};
use libp2p::core::Multiaddr;
use libp2p::PeerId;
use once_cell::sync::Lazy;
use qrcode::render::unicode;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::cmp::min;
use std::convert::TryInto;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::debug_span;
use tracing::Instrument;
use tracing::Span;
use typeshare::typeshare;
use url::Url;
use uuid::Uuid;

/// This trait is implemented by all types of request args that
/// the CLI can handle.
/// It provides a unified abstraction that can be useful for generics.
#[allow(async_fn_in_trait)]
pub trait Request {
    type Response: Serialize;
    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response>;
}

/// This generates a tracing span which is attached to all logs caused by a swap
fn get_swap_tracing_span(swap_id: Uuid) -> Span {
    debug_span!("swap", swap_id = %swap_id)
}

// BuyXmr
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BuyXmrArgs {
    #[typeshare(serialized_as = "string")]
    pub seller: Multiaddr,
    #[typeshare(serialized_as = "Option<string>")]
    pub bitcoin_change_address: Option<bitcoin::Address<NetworkUnchecked>>,
    #[typeshare(serialized_as = "string")]
    pub monero_receive_address: monero::Address,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct BuyXmrResponse {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
    pub quote: BidQuote,
}

impl Request for BuyXmrArgs {
    type Response = BuyXmrResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let swap_id = Uuid::new_v4();
        let swap_span = get_swap_tracing_span(swap_id);

        buy_xmr(self, swap_id, ctx).instrument(swap_span).await
    }
}

// ResumeSwap
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResumeSwapArgs {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct ResumeSwapResponse {
    pub result: String,
}

impl Request for ResumeSwapArgs {
    type Response = ResumeSwapResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let swap_span = get_swap_tracing_span(self.swap_id);

        resume_swap(self, ctx).instrument(swap_span).await
    }
}

// CancelAndRefund
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CancelAndRefundArgs {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
}

impl Request for CancelAndRefundArgs {
    type Response = serde_json::Value;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let swap_span = get_swap_tracing_span(self.swap_id);

        cancel_and_refund(self, ctx).instrument(swap_span).await
    }
}

// MoneroRecovery
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MoneroRecoveryArgs {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
}

impl Request for MoneroRecoveryArgs {
    type Response = serde_json::Value;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        monero_recovery(self, ctx).await
    }
}

// WithdrawBtc
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WithdrawBtcArgs {
    #[typeshare(serialized_as = "number")]
    #[serde(default, with = "::bitcoin::amount::serde::as_sat::opt")]
    pub amount: Option<bitcoin::Amount>,
    #[typeshare(serialized_as = "string")]
    #[serde(with = "crate::bitcoin::address_serde")]
    pub address: bitcoin::Address,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct WithdrawBtcResponse {
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub amount: bitcoin::Amount,
    pub txid: String,
}

impl Request for WithdrawBtcArgs {
    type Response = WithdrawBtcResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        withdraw_btc(self, ctx).await
    }
}

// ListSellers
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListSellersArgs {
    /// The rendezvous points to search for sellers
    /// The address must contain a peer ID
    #[typeshare(serialized_as = "Vec<string>")]
    pub rendezvous_points: Vec<Multiaddr>,
}

#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize)]
pub struct ListSellersResponse {
    sellers: Vec<SellerStatus>,
}

impl Request for ListSellersArgs {
    type Response = ListSellersResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        list_sellers(self, ctx).await
    }
}

// GetSwapInfo
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetSwapInfoArgs {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
}

#[typeshare]
#[derive(Serialize)]
pub struct GetSwapInfoResponse {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
    pub seller: AliceAddress,
    pub completed: bool,
    pub start_date: String,
    #[typeshare(serialized_as = "string")]
    pub state_name: String,
    #[typeshare(serialized_as = "number")]
    pub xmr_amount: monero::Amount,
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub btc_amount: bitcoin::Amount,
    #[typeshare(serialized_as = "string")]
    pub tx_lock_id: Txid,
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_cancel_fee: bitcoin::Amount,
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_refund_fee: bitcoin::Amount,
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_lock_fee: bitcoin::Amount,
    pub btc_refund_address: String,
    pub cancel_timelock: CancelTimelock,
    pub punish_timelock: PunishTimelock,
    pub timelock: Option<ExpiredTimelocks>,
}

impl Request for GetSwapInfoArgs {
    type Response = GetSwapInfoResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_swap_info(self, ctx).await
    }
}

// Balance
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BalanceArgs {
    pub force_refresh: bool,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BalanceResponse {
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub balance: bitcoin::Amount,
}

impl Request for BalanceArgs {
    type Response = BalanceResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_balance(self, ctx).await
    }
}

// GetHistory
#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetHistoryArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetHistoryEntry {
    #[typeshare(serialized_as = "string")]
    swap_id: Uuid,
    state: String,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetHistoryResponse {
    pub swaps: Vec<GetHistoryEntry>,
}

impl Request for GetHistoryArgs {
    type Response = GetHistoryResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_history(ctx).await
    }
}

// Additional structs
#[typeshare]
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct AliceAddress {
    #[typeshare(serialized_as = "string")]
    pub peer_id: PeerId,
    pub addresses: Vec<String>,
}

// Suspend current swap
#[derive(Debug, Deserialize)]
pub struct SuspendCurrentSwapArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct SuspendCurrentSwapResponse {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
}

impl Request for SuspendCurrentSwapArgs {
    type Response = SuspendCurrentSwapResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        suspend_current_swap(ctx).await
    }
}

pub struct GetCurrentSwapArgs;

impl Request for GetCurrentSwapArgs {
    type Response = serde_json::Value;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_current_swap(ctx).await
    }
}

pub struct GetConfig;

impl Request for GetConfig {
    type Response = serde_json::Value;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_config(ctx).await
    }
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct ExportBitcoinWalletArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct ExportBitcoinWalletResponse {
    #[typeshare(serialized_as = "object")]
    pub wallet_descriptor: serde_json::Value,
}

impl Request for ExportBitcoinWalletArgs {
    type Response = ExportBitcoinWalletResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_descriptor = export_bitcoin_wallet(ctx).await?;
        Ok(ExportBitcoinWalletResponse { wallet_descriptor })
    }
}

pub struct GetConfigArgs;

impl Request for GetConfigArgs {
    type Response = serde_json::Value;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_config(ctx).await
    }
}

pub struct GetSwapInfosAllArgs;

impl Request for GetSwapInfosAllArgs {
    type Response = Vec<GetSwapInfoResponse>;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_swap_infos_all(ctx).await
    }
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetLogsArgs {
    #[typeshare(serialized_as = "Option<string>")]
    pub swap_id: Option<Uuid>,
    pub redact: bool,
    #[typeshare(serialized_as = "Option<string>")]
    pub logs_dir: Option<PathBuf>,
}

#[typeshare]
#[derive(Serialize, Debug)]
pub struct GetLogsResponse {
    logs: Vec<String>,
}

impl Request for GetLogsArgs {
    type Response = GetLogsResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let dir = self.logs_dir.unwrap_or(ctx.config.data_dir.join("logs"));
        let logs = get_logs(dir, self.swap_id, self.redact).await?;

        for msg in &logs {
            println!("{msg}");
        }

        Ok(GetLogsResponse { logs })
    }
}

/// Best effort redaction of logs, e.g. wallet addresses, swap-ids
#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct RedactArgs {
    pub text: String,
}

#[typeshare]
#[derive(Serialize, Debug)]
pub struct RedactResponse {
    pub text: String,
}

impl Request for RedactArgs {
    type Response = RedactResponse;

    async fn request(self, _: Arc<Context>) -> Result<Self::Response> {
        Ok(RedactResponse {
            text: redact(&self.text),
        })
    }
}

#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetMoneroAddressesArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetMoneroAddressesResponse {
    #[typeshare(serialized_as = "Vec<String>")]
    pub addresses: Vec<monero::Address>,
}

impl Request for GetMoneroAddressesArgs {
    type Response = GetMoneroAddressesResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let addresses = ctx.db.get_monero_addresses().await?;
        Ok(GetMoneroAddressesResponse { addresses })
    }
}

#[tracing::instrument(fields(method = "suspend_current_swap"), skip(context))]
pub async fn suspend_current_swap(context: Arc<Context>) -> Result<SuspendCurrentSwapResponse> {
    let swap_id = context.swap_lock.get_current_swap_id().await;

    if let Some(id_value) = swap_id {
        context.swap_lock.send_suspend_signal().await?;

        Ok(SuspendCurrentSwapResponse { swap_id: id_value })
    } else {
        bail!("No swap is currently running")
    }
}

#[tracing::instrument(fields(method = "get_swap_infos_all"), skip(context))]
pub async fn get_swap_infos_all(context: Arc<Context>) -> Result<Vec<GetSwapInfoResponse>> {
    let swap_ids = context.db.all().await?;
    let mut swap_infos = Vec::new();

    for (swap_id, _) in swap_ids {
        match get_swap_info(GetSwapInfoArgs { swap_id }, context.clone()).await {
            Ok(swap_info) => swap_infos.push(swap_info),
            Err(error) => {
                tracing::error!(%swap_id, %error, "Failed to get swap info");
            }
        }
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

    let peer_id = context
        .db
        .get_peer_id(args.swap_id)
        .await
        .with_context(|| "Could not get PeerID")?;

    let addresses = context
        .db
        .get_addresses(peer_id)
        .await
        .with_context(|| "Could not get addressess")?;

    let start_date = context.db.get_swap_start_date(args.swap_id).await?;

    let swap_state: BobState = state.try_into()?;

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
            let State::Bob(BobState::SwapSetupCompleted(state2)) = state else {
                return None;
            };

            let xmr_amount = state2.xmr;
            let btc_amount = state2.tx_lock.lock_amount();
            let tx_cancel_fee = state2.tx_cancel_fee;
            let tx_refund_fee = state2.tx_refund_fee;
            let tx_lock_id = state2.tx_lock.txid();
            let btc_refund_address = state2.refund_address.to_string();

            let Ok(tx_lock_fee) = state2.tx_lock.fee() else {
                return None;
            };

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
        })
        .with_context(|| "Did not find SwapSetupCompleted state for swap")?;

    let timelock = swap_state.expired_timelocks(bitcoin_wallet.clone()).await?;

    Ok(GetSwapInfoResponse {
        swap_id: args.swap_id,
        seller: AliceAddress {
            peer_id,
            addresses: addresses.iter().map(|a| a.to_string()).collect(),
        },
        completed: is_completed,
        start_date,
        state_name: format!("{}", swap_state),
        xmr_amount,
        btc_amount,
        tx_lock_id,
        tx_cancel_fee,
        tx_refund_fee,
        tx_lock_fee,
        btc_refund_address: btc_refund_address.to_string(),
        cancel_timelock,
        punish_timelock,
        timelock,
    })
}

#[tracing::instrument(fields(method = "buy_xmr"), skip(context))]
pub async fn buy_xmr(
    buy_xmr: BuyXmrArgs,
    swap_id: Uuid,
    context: Arc<Context>,
) -> Result<BuyXmrResponse, anyhow::Error> {
    let BuyXmrArgs {
        seller,
        bitcoin_change_address,
        monero_receive_address,
    } = buy_xmr;

    let bitcoin_wallet = Arc::clone(
        context
            .bitcoin_wallet
            .as_ref()
            .expect("Could not find Bitcoin wallet"),
    );

    let bitcoin_change_address = match bitcoin_change_address {
        Some(addr) => addr
            .require_network(bitcoin_wallet.network())
            .context("Address is not on the correct network")?,
        None => {
            let internal_wallet_address = bitcoin_wallet.new_address().await?;

            tracing::info!(
                internal_wallet_address=%internal_wallet_address,
                "No --change-address supplied. Any change will be received to the internal wallet."
            );

            internal_wallet_address
        }
    };

    let monero_wallet = Arc::clone(
        context
            .monero_manager
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
        context.tor_client.clone(),
        behaviour,
    )
    .await?;

    swarm.add_peer_address(seller_peer_id, seller);

    context
        .db
        .insert_monero_address(swap_id, monero_receive_address)
        .await?;

    tracing::debug!(peer_id = %swarm.local_peer_id(), "Network layer initialized");

    context.swap_lock.acquire_swap_lock(swap_id).await?;

    context
        .tauri_handle
        .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::RequestingQuote);

    let initialize_swap = tokio::select! {
        biased;
        _ = context.swap_lock.listen_for_swap_force_suspension() => {
            tracing::debug!("Shutdown signal received, exiting");
            context.swap_lock.release_swap_lock().await.expect("Shutdown signal received but failed to release swap lock. The swap process has been terminated but the swap lock is still active.");

            context.tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);

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

            context
                .tauri_handle
                .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);

            bail!(error);
        }
    };

    context
        .tauri_handle
        .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::ReceivedQuote(bid_quote));

    context.tasks.clone().spawn(async move {
        tokio::select! {
            biased;
            _ = context.swap_lock.listen_for_swap_force_suspension() => {
                tracing::debug!("Shutdown signal received, exiting");
                context.swap_lock.release_swap_lock().await.expect("Shutdown signal received but failed to release swap lock. The swap process has been terminated but the swap lock is still active.");

                context.tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);

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
                let max_givable = || async {
                    let (amount, fee) = bitcoin_wallet.max_giveable(TxLock::script_size()).await?;
                    Ok((amount, fee))
                };

                let determine_amount = determine_btc_to_swap(
                    context.config.json,
                    bid_quote,
                    bitcoin_wallet.new_address(),
                    || bitcoin_wallet.balance(),
                    max_givable,
                    || bitcoin_wallet.sync(),
                    context.tauri_handle.clone(),
                    Some(swap_id)
                );

                let (tx_lock_amount, tx_lock_fee) = match determine_amount.await {
                    Ok(val) => val,
                    Err(error) => match error.downcast::<ZeroQuoteReceived>() {
                        Ok(_) => {
                            bail!("Seller's XMR balance is currently too low to initiate a swap, please try again later")
                        }
                        Err(other) => bail!(other),
                    },
                };

                tracing::info!(%tx_lock_amount, %tx_lock_fee, "Determined swap amount");

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
                    tx_lock_amount,
                    tx_lock_fee
                ).with_event_emitter(context.tauri_handle.clone());

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

        context.tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);

        Ok::<_, anyhow::Error>(())
    }.in_current_span()).await;

    Ok(BuyXmrResponse {
        swap_id,
        quote: bid_quote,
    })
}

#[tracing::instrument(fields(method = "resume_swap"), skip(context))]
pub async fn resume_swap(
    resume: ResumeSwapArgs,
    context: Arc<Context>,
) -> Result<ResumeSwapResponse> {
    let ResumeSwapArgs { swap_id } = resume;

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
    let mut swarm = swarm::cli(seed.clone(), context.tor_client.clone(), behaviour).await?;
    let our_peer_id = swarm.local_peer_id();

    tracing::debug!(peer_id = %our_peer_id, "Network layer initialized");

    // Fetch the seller's addresses from the database and add them to the swarm
    for seller_address in seller_addresses {
        swarm.add_peer_address(seller_peer_id, seller_address);
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
        context
            .monero_manager
            .as_ref()
            .context("Could not get Monero wallet manager")?
            .clone(),
        context.config.env_config,
        event_loop_handle,
        monero_receive_address,
    )
    .await?
    .with_event_emitter(context.tauri_handle.clone());

    context.swap_lock.acquire_swap_lock(swap_id).await?;

    context
        .tauri_handle
        .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Resuming);

    context.tasks.clone().spawn(
        async move {
            let handle = tokio::spawn(event_loop.run().in_current_span());
            tokio::select! {
                biased;
                _ = context.swap_lock.listen_for_swap_force_suspension() => {
                     tracing::debug!("Shutdown signal received, exiting");
                    context.swap_lock.release_swap_lock().await.expect("Shutdown signal received but failed to release swap lock. The swap process has been terminated but the swap lock is still active.");

                    context.tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);

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

            context.tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);

            Ok::<(), anyhow::Error>(())
        }
        .in_current_span(),
    ).await;

    Ok(ResumeSwapResponse {
        result: "OK".to_string(),
    })
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

    context
        .tauri_handle
        .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);

    state.map(|state| {
        json!({
            "result": state,
        })
    })
}

#[tracing::instrument(fields(method = "get_history"), skip(context))]
pub async fn get_history(context: Arc<Context>) -> Result<GetHistoryResponse> {
    let swaps = context.db.all().await?;
    let mut vec: Vec<GetHistoryEntry> = Vec::new();
    for (swap_id, state) in swaps {
        let state: BobState = state.try_into()?;
        vec.push(GetHistoryEntry {
            swap_id,
            state: state.to_string(),
        })
    }

    Ok(GetHistoryResponse { swaps: vec })
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
) -> Result<WithdrawBtcResponse> {
    let WithdrawBtcArgs { address, amount } = withdraw_btc;
    let bitcoin_wallet = context
        .bitcoin_wallet
        .as_ref()
        .context("Could not get Bitcoin wallet")?;

    let (withdraw_tx_unsigned, amount) = match amount {
        Some(amount) => {
            let withdraw_tx_unsigned = bitcoin_wallet
                .send_to_address_dynamic_fee(address, amount, None)
                .await?;

            (withdraw_tx_unsigned, amount)
        }
        None => {
            let (max_giveable, spending_fee) = bitcoin_wallet
                .max_giveable(address.script_pubkey().len())
                .await?;

            let withdraw_tx_unsigned = bitcoin_wallet
                .send_to_address(address, max_giveable, spending_fee, None)
                .await?;

            (withdraw_tx_unsigned, max_giveable)
        }
    };

    let withdraw_tx = bitcoin_wallet
        .sign_and_finalize(withdraw_tx_unsigned)
        .await?;

    bitcoin_wallet
        .broadcast(withdraw_tx.clone(), "withdraw")
        .await?;

    let txid = withdraw_tx.compute_txid();

    Ok(WithdrawBtcResponse {
        txid: txid.to_string(),
        amount,
    })
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
        balance: bitcoin_balance,
    })
}

#[tracing::instrument(fields(method = "list_sellers"), skip(context))]
pub async fn list_sellers(
    list_sellers: ListSellersArgs,
    context: Arc<Context>,
) -> Result<ListSellersResponse> {
    let ListSellersArgs { rendezvous_points } = list_sellers;
    let rendezvous_nodes: Vec<_> = rendezvous_points
        .iter()
        .filter_map(|rendezvous_point| rendezvous_point.split_peer_id())
        .collect();

    let identity = context
        .config
        .seed
        .as_ref()
        .context("Cannot extract seed")?
        .derive_libp2p_identity();

    let sellers = list_sellers_impl(
        rendezvous_nodes,
        context.config.namespace,
        context.tor_client.clone(),
        identity,
        Some(context.db.clone()),
        context.tauri_handle(),
    )
    .await?;

    for seller in &sellers {
        match seller {
            SellerStatus::Online(QuoteWithAddress {
                quote,
                multiaddr,
                peer_id,
                version,
            }) => {
                tracing::debug!(
                    status = "Online",
                    price = %quote.price.to_string(),
                    min_quantity = %quote.min_quantity.to_string(),
                    max_quantity = %quote.max_quantity.to_string(),
                    address = %multiaddr.clone().to_string(),
                    peer_id = %peer_id,
                    version = %version,
                    "Fetched peer status"
                );

                // Add the peer as known to the database
                // This'll allow us to later request a quote again
                // without having to re-discover the peer at the rendezvous point
                context
                    .db
                    .insert_address(*peer_id, multiaddr.clone())
                    .await?;
            }
            SellerStatus::Unreachable(UnreachableSeller { peer_id }) => {
                tracing::debug!(
                    status = "Unreachable",
                    peer_id = %peer_id.to_string(),
                    "Fetched peer status"
                );
            }
        }
    }

    Ok(ListSellersResponse { sellers })
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
        "swap_id": context.swap_lock.get_current_swap_id().await,
    }))
}

pub async fn resolve_approval_request(
    resolve_approval: ResolveApprovalArgs,
    ctx: Arc<Context>,
) -> Result<ResolveApprovalResponse> {
    let request_id = Uuid::parse_str(&resolve_approval.request_id).context("Invalid request ID")?;

    if let Some(handle) = ctx.tauri_handle.clone() {
        handle
            .resolve_approval(request_id, resolve_approval.accept)
            .await?;
    } else {
        bail!("Cannot resolve approval without a Tauri handle");
    }

    Ok(ResolveApprovalResponse { success: true })
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

#[allow(clippy::too_many_arguments)]
pub async fn determine_btc_to_swap<FB, TB, FMG, TMG, FS, TS>(
    json: bool,
    bid_quote: BidQuote,
    get_new_address: impl Future<Output = Result<bitcoin::Address>>,
    balance: FB,
    max_giveable_fn: FMG,
    sync: FS,
    event_emitter: Option<TauriHandle>,
    swap_id: Option<Uuid>,
) -> Result<(bitcoin::Amount, bitcoin::Amount)>
where
    TB: Future<Output = Result<bitcoin::Amount>>,
    FB: Fn() -> TB,
    TMG: Future<Output = Result<(bitcoin::Amount, bitcoin::Amount)>>,
    FMG: Fn() -> TMG,
    TS: Future<Output = Result<()>>,
    FS: Fn() -> TS,
{
    if bid_quote.max_quantity == bitcoin::Amount::ZERO {
        bail!(ZeroQuoteReceived)
    }

    tracing::info!(
        price = %bid_quote.price,
        minimum_amount = %bid_quote.min_quantity,
        maximum_amount = %bid_quote.max_quantity,
        "Received quote",
    );

    sync().await.context("Failed to sync of Bitcoin wallet")?;
    let (mut max_giveable, mut spending_fee) = max_giveable_fn().await?;

    if max_giveable == bitcoin::Amount::ZERO || max_giveable < bid_quote.min_quantity {
        let deposit_address = get_new_address.await?;
        let minimum_amount = bid_quote.min_quantity;
        let maximum_amount = bid_quote.max_quantity;

        // To avoid any issus, we clip maximum_amount to never go above the
        // total maximim Bitcoin supply
        let maximum_amount = maximum_amount.min(bitcoin::Amount::MAX_MONEY);

        if !json {
            eprintln!("{}", qr_code(&deposit_address)?);
        }

        loop {
            let min_outstanding = bid_quote.min_quantity - max_giveable;
            let min_bitcoin_lock_tx_fee = spending_fee;
            let min_deposit_until_swap_will_start = min_outstanding + min_bitcoin_lock_tx_fee;
            let max_deposit_until_maximum_amount_is_reached = maximum_amount
                .checked_sub(max_giveable)
                .context("Overflow when subtracting max_giveable from maximum_amount")?
                .checked_add(min_bitcoin_lock_tx_fee)
                .context(format!("Overflow when adding min_bitcoin_lock_tx_fee ({min_bitcoin_lock_tx_fee}) to max_giveable ({max_giveable}) with maximum_amount ({maximum_amount})"))?;

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

            if let Some(swap_id) = swap_id {
                event_emitter.emit_swap_progress_event(
                    swap_id,
                    TauriSwapProgressEvent::WaitingForBtcDeposit {
                        deposit_address: deposit_address.clone(),
                        max_giveable,
                        min_deposit_until_swap_will_start,
                        max_deposit_until_maximum_amount_is_reached,
                        min_bitcoin_lock_tx_fee,
                        quote: bid_quote,
                    },
                );
            }

            (max_giveable, spending_fee) = loop {
                sync()
                    .await
                    .context("Failed to sync Bitcoin wallet while waiting for deposit")?;
                let (new_max_givable, new_fee) = max_giveable_fn().await?;

                if new_max_givable > max_giveable {
                    break (new_max_givable, new_fee);
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

#[typeshare]
#[derive(Deserialize, Serialize)]
pub struct CheckMoneroNodeArgs {
    pub url: String,
    pub network: String,
}

#[typeshare]
#[derive(Deserialize, Serialize)]
pub struct CheckMoneroNodeResponse {
    pub available: bool,
}

#[typeshare]
#[derive(Deserialize, Serialize)]
pub struct GetDataDirArgs {
    pub is_testnet: bool,
}

#[derive(Error, Debug)]
#[error("this is not one of the known monero networks")]
struct UnknownMoneroNetwork(String);

impl CheckMoneroNodeArgs {
    pub async fn request(self) -> Result<CheckMoneroNodeResponse> {
        let url = self.url.clone();
        let network_str = self.network.clone();

        let network = match self.network.to_lowercase().as_str() {
            // When the GUI says testnet, it means monero stagenet
            "mainnet" => Network::Mainnet,
            "testnet" => Network::Stagenet,
            otherwise => anyhow::bail!(UnknownMoneroNetwork(otherwise.to_string())),
        };

        static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
            reqwest::Client::builder()
                // This function is called very frequently, so we set the timeout to be short
                .timeout(Duration::from_secs(5))
                .https_only(false)
                .build()
                .expect("reqwest client to work")
        });

        let Ok(monero_daemon) = MoneroDaemon::from_str(self.url, network) else {
            return Ok(CheckMoneroNodeResponse { available: false });
        };

        match monero_daemon.is_available(&CLIENT).await {
            Ok(available) => Ok(CheckMoneroNodeResponse { available }),
            Err(e) => {
                tracing::error!(
                    url = %url,
                    network = %network_str,
                    error = ?e,
                    error_chain = %format!("{:#}", e),
                    "Failed to check monero node availability"
                );

                Ok(CheckMoneroNodeResponse { available: false })
            }
        }
    }
}

#[typeshare]
#[derive(Deserialize, Clone)]
pub struct CheckElectrumNodeArgs {
    pub url: String,
}

#[typeshare]
#[derive(Serialize, Clone)]
pub struct CheckElectrumNodeResponse {
    pub available: bool,
}

impl CheckElectrumNodeArgs {
    pub async fn request(self) -> Result<CheckElectrumNodeResponse> {
        // Check if the URL is valid
        let Ok(url) = Url::parse(&self.url) else {
            return Ok(CheckElectrumNodeResponse { available: false });
        };

        // Check if the node is available
        let res = wallet::Client::new(&[url.as_str().to_string()], Duration::from_secs(60)).await;

        Ok(CheckElectrumNodeResponse {
            available: res.is_ok(),
        })
    }
}

#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResolveApprovalArgs {
    pub request_id: String,
    pub accept: bool,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct ResolveApprovalResponse {
    pub success: bool,
}

impl Request for ResolveApprovalArgs {
    type Response = ResolveApprovalResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        resolve_approval_request(self, ctx).await
    }
}
