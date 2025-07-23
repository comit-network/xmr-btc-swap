use super::tauri_bindings::TauriHandle;
use crate::bitcoin::{wallet, CancelTimelock, ExpiredTimelocks, PunishTimelock};
use crate::cli::api::tauri_bindings::{
    ApprovalRequestType, SelectMakerDetails, SendMoneroDetails, TauriEmitter,
    TauriSwapProgressEvent, SelectOfferApprovalRequest,
};
use crate::cli::api::Context;
use crate::cli::list_sellers::{list_sellers_init, QuoteWithAddress, UnreachableSeller};
use crate::cli::{list_sellers as list_sellers_impl, EventLoop, SellerStatus};
use crate::common::{get_logs, redact};
use crate::libp2p_ext::MultiAddrExt;
use crate::monero::wallet_rpc::MoneroDaemon;
use crate::monero::MoneroAddressPool;
use crate::network::quote::BidQuote;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::network::swarm;
use crate::protocol::bob::{BobState, Swap};
use crate::protocol::{bob, Database, State};
use crate::{bitcoin, cli, monero};
use ::bitcoin::address::NetworkUnchecked;
use ::bitcoin::Txid;
use ::monero::Network;
use anyhow::{bail, Context as AnyContext, Result};
use arti_client::TorClient;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use libp2p::core::Multiaddr;
use libp2p::{identity, PeerId};
use monero_seed::{Language, Seed as MoneroSeed};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::TryInto;
use std::future::Future;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio_util::task::AbortOnDropHandle;
use tor_rtcompat::tokio::TokioRustlsRuntime;
use tracing::debug_span;
use tracing::Instrument;
use tracing::Span;
use typeshare::typeshare;
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

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
    #[typeshare(serialized_as = "Vec<string>")]
    pub rendezvous_points: Vec<Multiaddr>,
    #[typeshare(serialized_as = "Vec<string>")]
    pub sellers: Vec<Multiaddr>,
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
    #[serde(with = "swap_serde::bitcoin::address_serde")]
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
    pub monero_receive_pool: MoneroAddressPool,
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
    // If no swap was running, we still return Ok(...) but this is set to None
    #[typeshare(serialized_as = "Option<string>")]
    pub swap_id: Option<Uuid>,
}

impl Request for SuspendCurrentSwapArgs {
    type Response = SuspendCurrentSwapResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        suspend_current_swap(ctx).await
    }
}

#[typeshare]
#[derive(Debug, Serialize, Deserialize)]
pub struct GetCurrentSwapArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetCurrentSwapResponse {
    #[typeshare(serialized_as = "Option<string>")]
    pub swap_id: Option<Uuid>,
}

impl Request for GetCurrentSwapArgs {
    type Response = GetCurrentSwapResponse;

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
        let dir = self.logs_dir.unwrap_or(ctx.config.log_dir.clone());
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
pub struct GetRestoreHeightArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetRestoreHeightResponse {
    #[typeshare(serialized_as = "number")]
    pub height: u64,
}

impl Request for GetRestoreHeightArgs {
    type Response = GetRestoreHeightResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet = ctx
            .monero_manager
            .as_ref()
            .context("Monero wallet manager not available")?;
        let wallet = wallet.main_wallet().await;
        let height = wallet.get_restore_height().await?;

        Ok(GetRestoreHeightResponse { height })
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

#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetMoneroHistoryArgs;

#[typeshare]
#[derive(Serialize, Clone, Deserialize, Debug)]
pub struct GetMoneroHistoryResponse {
    pub transactions: Vec<monero_sys::TransactionInfo>,
}

impl Request for GetMoneroHistoryArgs {
    type Response = GetMoneroHistoryResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet = ctx
            .monero_manager
            .as_ref()
            .context("Monero wallet manager not available")?;
        let wallet = wallet.main_wallet().await;

        let transactions = wallet.history().await;
        Ok(GetMoneroHistoryResponse { transactions })
    }
}

#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetMoneroMainAddressArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetMoneroMainAddressResponse {
    #[typeshare(serialized_as = "String")]
    pub address: monero::Address,
}

impl Request for GetMoneroMainAddressArgs {
    type Response = GetMoneroMainAddressResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet = ctx
            .monero_manager
            .as_ref()
            .context("Monero wallet manager not available")?;
        let wallet = wallet.main_wallet().await;
        let address = wallet.main_address().await;
        Ok(GetMoneroMainAddressResponse { address })
    }
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Date {
    #[typeshare(serialized_as = "number")]
    pub year: u16,
    #[typeshare(serialized_as = "number")]
    pub month: u8,
    #[typeshare(serialized_as = "number")]
    pub day: u8,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "height")]
pub enum SetRestoreHeightArgs {
    #[typeshare(serialized_as = "number")]
    Height(u32),
    #[typeshare(serialized_as = "object")]
    Date(Date),
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct SetRestoreHeightResponse {
    pub success: bool,
}

impl Request for SetRestoreHeightArgs {
    type Response = SetRestoreHeightResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet = ctx
            .monero_manager
            .as_ref()
            .context("Monero wallet manager not available")?;
        let wallet = wallet.main_wallet().await;

        let height = match self {
            SetRestoreHeightArgs::Height(height) => height as u64,
            SetRestoreHeightArgs::Date(date) => {
                let year: u16 = date.year;
                let month: u8 = date.month;
                let day: u8 = date.day;

                // Validate ranges
                if month < 1 || month > 12 {
                    bail!("Month must be between 1 and 12");
                }
                if day < 1 || day > 31 {
                    bail!("Day must be between 1 and 31");
                }

                tracing::info!(
                    "Getting blockchain height for date: {}-{}-{}",
                    year,
                    month,
                    day
                );

                let height = wallet
                    .get_blockchain_height_by_date(year, month, day)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to get blockchain height for date {}-{}-{}",
                            year, month, day
                        )
                    })?;
                tracing::info!(
                    "Blockchain height for date {}-{}-{}: {}",
                    year,
                    month,
                    day,
                    height
                );

                height
            }
        };

        wallet.set_restore_height(height).await?;

        wallet.pause_refresh().await;
        wallet.stop().await;
        tracing::debug!("Background refresh stopped");

        wallet.rescan_blockchain_async().await;
        wallet.start_refresh().await;
        tracing::info!("Rescanning blockchain from height {} completed", height);

        Ok(SetRestoreHeightResponse { success: true })
    }
}

// New request type for Monero balance
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetMoneroBalanceArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetMoneroBalanceResponse {
    #[typeshare(serialized_as = "string")]
    pub total_balance: crate::monero::Amount,
    #[typeshare(serialized_as = "string")]
    pub unlocked_balance: crate::monero::Amount,
}

impl Request for GetMoneroBalanceArgs {
    type Response = GetMoneroBalanceResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_manager = ctx
            .monero_manager
            .as_ref()
            .context("Monero wallet manager not available")?;
        let wallet = wallet_manager.main_wallet().await;

        let total_balance = wallet.total_balance().await;
        let unlocked_balance = wallet.unlocked_balance().await;

        Ok(GetMoneroBalanceResponse {
            total_balance: crate::monero::Amount::from_piconero(total_balance.as_pico()),
            unlocked_balance: crate::monero::Amount::from_piconero(unlocked_balance.as_pico()),
        })
    }
}

#[typeshare]
#[derive(Debug, Serialize, Deserialize)]
pub struct SendMoneroArgs {
    #[typeshare(serialized_as = "String")]
    pub address: String,
    pub amount: SendMoneroAmount,
}

#[typeshare]
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "amount")]
pub enum SendMoneroAmount {
    Sweep,
    Specific(crate::monero::Amount),
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct SendMoneroResponse {
    pub tx_hash: String,
    pub address: String,
    pub amount_sent: crate::monero::Amount,
    pub fee: crate::monero::Amount,
}

impl Request for SendMoneroArgs {
    type Response = SendMoneroResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_manager = ctx
            .monero_manager
            .as_ref()
            .context("Monero wallet manager not available")?;
        let wallet = wallet_manager.main_wallet().await;

        // Parse the address
        let address = monero::Address::from_str(&self.address)
            .map_err(|e| anyhow::anyhow!("Invalid Monero address: {}", e))?;

        let tauri_handle = ctx
            .tauri_handle()
            .context("Tauri needs to be available to approve transactions")?;

        // This is a closure that will be called by the monero-sys library to get approval for the transaction
        // It sends an approval request to the frontend and returns true if the user approves the transaction
        let approval_callback: Arc<
            dyn Fn(
                    String,
                    ::monero::Amount,
                    ::monero::Amount,
                )
                    -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>>
                + Send
                + Sync,
        > = std::sync::Arc::new(
            move |_txid: String, amount: ::monero::Amount, fee: ::monero::Amount| {
                let tauri_handle = tauri_handle.clone();

                Box::pin(async move {
                    let details = SendMoneroDetails {
                        address: address.to_string(),
                        amount: amount.into(),
                        fee: fee.into(),
                    };

                    tauri_handle
                        .request_approval::<bool>(
                            ApprovalRequestType::SendMonero(details),
                            Some(60 * 5),
                        )
                        .await
                        .unwrap_or(false)
                })
            },
        );

        let amount = match self.amount {
            SendMoneroAmount::Sweep => None,
            SendMoneroAmount::Specific(amount) => Some(amount.into()),
        };

        // This is the actual call to the monero-sys library to send the transaction
        // monero-sys will call the approval callback after it has constructed and signed the transaction
        // once the user approves, the transaction is published
        let (receipt, amount_sent, fee) = wallet
            .transfer_with_approval(&address, amount, approval_callback)
            .await?
            .context("Transaction was not approved by user")?;

        Ok(SendMoneroResponse {
            tx_hash: receipt.txid,
            address: address.to_string(),
            amount_sent: amount_sent.into(),
            fee: fee.into(),
        })
    }
}

#[tracing::instrument(fields(method = "suspend_current_swap"), skip(context))]
pub async fn suspend_current_swap(context: Arc<Context>) -> Result<SuspendCurrentSwapResponse> {
    let swap_id = context.swap_lock.get_current_swap_id().await;

    if let Some(id_value) = swap_id {
        context.swap_lock.send_suspend_signal().await?;

        Ok(SuspendCurrentSwapResponse {
            swap_id: Some(id_value),
        })
    } else {
        // If no swap was running, we still return Ok(...) with None
        Ok(SuspendCurrentSwapResponse { swap_id: None })
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

    let monero_receive_pool = context.db.get_monero_address_pool(args.swap_id).await?;

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
        monero_receive_pool,
    })
}

#[tracing::instrument(fields(method = "buy_xmr"), skip(context))]
pub async fn buy_xmr(
    buy_xmr: BuyXmrArgs,
    swap_id: Uuid,
    context: Arc<Context>,
) -> Result<BuyXmrResponse, anyhow::Error> {
    let _span = get_swap_tracing_span(swap_id);

    let BuyXmrArgs {
        rendezvous_points,
        sellers,
    } = buy_xmr;

    let bitcoin_wallet = Arc::clone(
        context
            .bitcoin_wallet
            .as_ref()
            .expect("Could not find Bitcoin wallet"),
    );

    let monero_wallet = Arc::clone(
        context
            .monero_manager
            .as_ref()
            .context("Could not get Monero wallet")?,
    );

    let env_config = context.config.env_config;
    let seed = context.config.seed.clone().context("Could not get seed")?;

    // Prepare variables for the quote fetching process
    let identity = seed.derive_libp2p_identity();
    let namespace = context.config.namespace;
    let tor_client = context.tor_client.clone();
    let db = Some(context.db.clone());
    let tauri_handle = context.tauri_handle.clone();

    // Wait for the user to approve a seller and to deposit coins
    // Calling determine_btc_to_swap
    let address_len = bitcoin_wallet.new_address().await?.script_pubkey().len();

    let bitcoin_wallet_for_closures = Arc::clone(&bitcoin_wallet);

    let rendezvous_points_clone = rendezvous_points.clone();
    let sellers_clone = sellers.clone();

    // Acquire the lock before the user has selected a maker and we already have funds in the wallet
    // because we need to be able to cancel the determine_btc_to_swap(..)
    context.swap_lock.acquire_swap_lock(swap_id).await?;

    let (seller_multiaddr, seller_peer_id, quote, tx_lock_amount, tx_lock_fee, bitcoin_change_address, monero_receive_pool) = tokio::select! {
        result = determine_btc_to_swap(
            move || {
                let rendezvous_points = rendezvous_points_clone.clone();
                let sellers = sellers_clone.clone();
                let namespace = namespace;
                let identity = identity.clone();
                let db = db.clone();
                let tor_client = tor_client.clone();
                let tauri_handle = tauri_handle.clone();

                Box::pin(async move {
                    fetch_quotes_task(
                        rendezvous_points,
                        namespace,
                        sellers,
                        identity,
                        db,
                        tor_client,
                        tauri_handle,
                    ).await
                })
            },
            bitcoin_wallet.new_address(),
            {
                let wallet = Arc::clone(&bitcoin_wallet_for_closures);
                move || {
                    let w = wallet.clone();
                    async move { w.balance().await }
                }
            },
            {
                let wallet = Arc::clone(&bitcoin_wallet_for_closures);
                move || {
                    let w = wallet.clone();
                    async move { w.max_giveable(address_len).await }
                }
            },
            {
                let wallet = Arc::clone(&bitcoin_wallet_for_closures);
                move || {
                    let w = wallet.clone();
                    async move { w.sync().await }
                }
            },
            context.tauri_handle.clone(),
            swap_id,
            |quote_with_address| {
                let tauri_handle = context.tauri_handle.clone();
                Box::new(async move {
                    let details = SelectMakerDetails {
                        swap_id,
                        btc_amount_to_swap: quote_with_address.quote.max_quantity,
                        maker: quote_with_address,
                    };

                    tauri_handle.request_maker_selection(details, 300).await
                }) as Box<dyn Future<Output = Result<Option<SelectOfferApprovalRequest>>> + Send>
            },
        ) => {
            result?
        }
        _ = context.swap_lock.listen_for_swap_force_suspension() => {
            context.swap_lock.release_swap_lock().await.expect("Shutdown signal received but failed to release swap lock. The swap process has been terminated but the swap lock is still active.");
            context.tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);
            bail!("Shutdown signal received");
        },
    };

    monero_receive_pool.assert_network(context.config.env_config.monero_network)?;
    monero_receive_pool.assert_sum_to_one()?;

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

    // Clone bitcoin_change_address before moving it in the emit call
    let bitcoin_change_address_for_spawn = bitcoin_change_address.clone();

    // Insert the peer_id into the database
    context.db.insert_peer_id(swap_id, seller_peer_id).await?;

    context
        .db
        .insert_address(seller_peer_id, seller_multiaddr.clone())
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

    swarm.add_peer_address(seller_peer_id, seller_multiaddr.clone());

    context
        .db
        .insert_monero_address_pool(swap_id, monero_receive_pool.clone())
        .await?;

    tracing::debug!(peer_id = %swarm.local_peer_id(), "Network layer initialized");

    context.tauri_handle.emit_swap_progress_event(
        swap_id,
        TauriSwapProgressEvent::ReceivedQuote(quote.clone()),
    );

    // Now create the event loop we use for the swap
    let (event_loop, event_loop_handle) =
        EventLoop::new(swap_id, swarm, seller_peer_id, context.db.clone())?;
    let event_loop = tokio::spawn(event_loop.run().in_current_span());

    context
        .tauri_handle
        .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::ReceivedQuote(quote));

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
                let swap = Swap::new(
                    Arc::clone(&context.db),
                    swap_id,
                    Arc::clone(&bitcoin_wallet),
                    monero_wallet,
                    env_config,
                    event_loop_handle,
                    monero_receive_pool.clone(),
                    bitcoin_change_address_for_spawn,
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

    Ok(BuyXmrResponse { swap_id, quote })
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

    let monero_receive_pool = context.db.get_monero_address_pool(swap_id).await?;

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
        monero_receive_pool,
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
                tracing::trace!(
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
                tracing::trace!(
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
pub async fn get_current_swap(context: Arc<Context>) -> Result<GetCurrentSwapResponse> {
    let swap_id = context.swap_lock.get_current_swap_id().await;
    Ok(GetCurrentSwapResponse { swap_id })
}

pub async fn fetch_quotes_task(
    rendezvous_points: Vec<Multiaddr>,
    namespace: XmrBtcNamespace,
    sellers: Vec<Multiaddr>,
    identity: identity::Keypair,
    db: Option<Arc<dyn Database + Send + Sync>>,
    tor_client: Option<Arc<TorClient<TokioRustlsRuntime>>>,
    tauri_handle: Option<TauriHandle>,
) -> Result<(
    tokio::task::JoinHandle<()>,
    ::tokio::sync::watch::Receiver<Vec<SellerStatus>>,
)> {
    let (tx, rx) = ::tokio::sync::watch::channel(Vec::new());

    let rendezvous_nodes: Vec<_> = rendezvous_points
        .iter()
        .filter_map(|addr| addr.split_peer_id())
        .collect();

    let fetch_fn = list_sellers_init(
        rendezvous_nodes,
        namespace,
        tor_client,
        identity,
        db,
        tauri_handle,
        Some(tx.clone()),
        sellers,
    )
    .await?;

    let handle = tokio::task::spawn(async move {
        loop {
            let sellers = fetch_fn().await;
            let _ = tx.send(sellers);

            tokio::time::sleep(std::time::Duration::from_secs(90)).await;
        }
    });

    Ok((handle, rx))
}

// TODO: Let this take a refresh interval as an argument
pub async fn refresh_wallet_task<FMG, TMG, FB, TB, FS, TS>(
    max_giveable_fn: FMG,
    balance_fn: FB,
    sync_fn: FS,
) -> Result<(
    tokio::task::JoinHandle<()>,
    ::tokio::sync::watch::Receiver<(bitcoin::Amount, bitcoin::Amount)>,
)>
where
    TMG: Future<Output = Result<(bitcoin::Amount, bitcoin::Amount)>> + Send + 'static,
    FMG: Fn() -> TMG + Send + 'static,
    TB: Future<Output = Result<bitcoin::Amount>> + Send + 'static,
    FB: Fn() -> TB + Send + 'static,
    TS: Future<Output = Result<()>> + Send + 'static,
    FS: Fn() -> TS + Send + 'static,
{
    let (tx, rx) = ::tokio::sync::watch::channel((bitcoin::Amount::ZERO, bitcoin::Amount::ZERO));

    let handle = tokio::task::spawn(async move {
        loop {
            // Sync wallet before checking balance
            let _ = sync_fn().await;

            if let (Ok(balance), Ok((max_giveable, _fee))) =
                (balance_fn().await, max_giveable_fn().await)
            {
                let _ = tx.send((balance, max_giveable));
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });

    Ok((handle, rx))
}

#[allow(clippy::too_many_arguments)]
pub async fn determine_btc_to_swap<FB, TB, FMG, TMG, FS, TS, FQ>(
    quote_fetch_tasks: FQ,
    // TODO: Shouldn't this be a function?
    get_new_address: impl Future<Output = Result<bitcoin::Address>>,
    balance: FB,
    max_giveable_fn: FMG,
    sync: FS,
    event_emitter: Option<TauriHandle>,
    swap_id: Uuid,
    request_approval: impl Fn(QuoteWithAddress) -> Box<dyn Future<Output = Result<Option<SelectOfferApprovalRequest>>> + Send>,
) -> Result<(
    Multiaddr,
    PeerId,
    BidQuote,
    bitcoin::Amount,
    bitcoin::Amount,
    Option<bitcoin::Address<NetworkUnchecked>>,
    MoneroAddressPool,
)>
where
    TB: Future<Output = Result<bitcoin::Amount>> + Send + 'static,
    FB: Fn() -> TB + Send + 'static,
    TMG: Future<Output = Result<(bitcoin::Amount, bitcoin::Amount)>> + Send + 'static,
    FMG: Fn() -> TMG + Send + 'static,
    TS: Future<Output = Result<()>> + Send + 'static,
    FS: Fn() -> TS + Send + 'static,
    FQ: Fn() -> std::pin::Pin<
        Box<
            dyn Future<
                    Output = Result<(
                        tokio::task::JoinHandle<()>,
                        ::tokio::sync::watch::Receiver<Vec<SellerStatus>>,
                    )>,
                > + Send,
        >,
    >,
{
    // Start background tasks with watch channels
    let (quote_fetch_handle, mut quotes_rx): (
        _,
        ::tokio::sync::watch::Receiver<Vec<SellerStatus>>,
    ) = quote_fetch_tasks().await?;
    let (wallet_refresh_handle, mut balance_rx): (
        _,
        ::tokio::sync::watch::Receiver<(bitcoin::Amount, bitcoin::Amount)>,
    ) = refresh_wallet_task(max_giveable_fn, balance, sync).await?;

    // Get the abort handles to kill the background tasks when we exit the function
    let quote_fetch_abort_handle = AbortOnDropHandle::new(quote_fetch_handle);
    let wallet_refresh_abort_handle = AbortOnDropHandle::new(wallet_refresh_handle);

    let mut pending_approvals = FuturesUnordered::new();

    let deposit_address = get_new_address.await?;

    loop {
        // Get the latest quotes, balance and max_giveable
        let quotes = quotes_rx.borrow().clone();
        let (balance, max_giveable) = *balance_rx.borrow();

        let success_quotes = quotes
            .iter()
            .filter_map(|quote| match quote {
                SellerStatus::Online(quote_with_address) => Some(quote_with_address.clone()),
                SellerStatus::Unreachable(_) => None,
            })
            .collect::<Vec<_>>();

        // Emit a Tauri event
        event_emitter.emit_swap_progress_event(
            swap_id,
            TauriSwapProgressEvent::WaitingForBtcDeposit {
                deposit_address: deposit_address.clone(),
                max_giveable: max_giveable,
                min_bitcoin_lock_tx_fee: balance - max_giveable,
                known_quotes: success_quotes.clone(),
            },
        );

        // Iterate through quotes and find ones that match the balance and max_giveable
        let matching_quotes = success_quotes
            .iter()
            .filter_map(|quote_with_address| {
                let quote = quote_with_address.quote;

                if quote.min_quantity <= max_giveable && quote.max_quantity > bitcoin::Amount::ZERO
                {
                    let tx_lock_fee = balance - max_giveable;
                    let tx_lock_amount = std::cmp::min(max_giveable, quote.max_quantity);

                    Some((quote_with_address.clone(), tx_lock_amount, tx_lock_fee))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        // Put approval requests into FuturesUnordered
        for (quote, tx_lock_amount, tx_lock_fee) in matching_quotes {
            let future = request_approval(quote.clone());

            pending_approvals.push(async move {
                use std::pin::Pin;
                let pinned_future = Pin::from(future);
                let response = pinned_future.await;

                match response {
                    Ok(Some(response)) => {
                        Ok::<
                            Option<(
                                Multiaddr,
                                PeerId,
                                BidQuote,
                                bitcoin::Amount,
                                bitcoin::Amount,
                                Option<bitcoin::Address<NetworkUnchecked>>,
                                MoneroAddressPool,
                            )>,
                            anyhow::Error,
                        >(Some((
                            quote.multiaddr.clone(),
                            quote.peer_id.clone(),
                            quote.quote.clone(),
                            tx_lock_amount,
                            tx_lock_fee,
                            response.bitcoin_change_address,
                            response.monero_receive_pool,
                        )))
                    }
                    Ok(None) => Ok(None),
                    Err(_) => Ok(None),
                }
            });
        }

        tracing::info!(
            swap_id = ?swap_id,
            pending_approvals = ?pending_approvals.len(),
            balance = ?balance,
            max_giveable = ?max_giveable,
            quotes = ?quotes,
            "Waiting for user to select an offer"
        );

        // Listen for approvals, balance changes, or quote changes
        let result: Option<(
            Multiaddr,
            PeerId,
            BidQuote,
            bitcoin::Amount,
            bitcoin::Amount,
            Option<bitcoin::Address<NetworkUnchecked>>,
            MoneroAddressPool,
        )> = tokio::select! {
            // Any approval request completes
            approval_result = pending_approvals.next(), if !pending_approvals.is_empty() => {
                match approval_result {
                    Some(Ok(Some(result))) => Some(result),
                    Some(Ok(None)) => None, // User rejected
                    Some(Err(_)) => None,   // Error in approval
                    None => None,           // No more futures
                }
            }
            // Balance changed - drop all pending approval requests and and re-calculate
            _ = balance_rx.changed() => {
                pending_approvals.clear();
                None
            }
            // Quotes changed - drop all pending approval requests and re-calculate
            _ = quotes_rx.changed() => {
                pending_approvals.clear();
                None
            }
        };

        // If user accepted an offer, return it to start the swap
        if let Some((multiaddr, peer_id, quote, tx_lock_amount, tx_lock_fee, bitcoin_change_address, monero_receive_pool)) = result {
            quote_fetch_abort_handle.abort();
            wallet_refresh_abort_handle.abort();

            return Ok((multiaddr, peer_id, quote, tx_lock_amount, tx_lock_fee, bitcoin_change_address, monero_receive_pool));
        }

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
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
#[derive(Debug, Serialize, Deserialize)]
pub struct ResolveApprovalArgs {
    pub request_id: String,
    #[typeshare(serialized_as = "object")]
    pub accept: serde_json::Value,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct ResolveApprovalResponse {
    pub success: bool,
}

#[typeshare]
#[derive(Debug, Serialize, Deserialize)]
pub struct RejectApprovalArgs {
    pub request_id: String,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct RejectApprovalResponse {
    pub success: bool,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct CheckSeedArgs {
    pub seed: String,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct CheckSeedResponse {
    pub available: bool,
}

impl CheckSeedArgs {
    pub async fn request(self) -> Result<CheckSeedResponse> {
        let seed = MoneroSeed::from_string(Language::English, Zeroizing::new(self.seed));
        Ok(CheckSeedResponse {
            available: seed.is_ok(),
        })
    }
}

// New request type for Monero sync progress
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetMoneroSyncProgressArgs;

#[typeshare]
#[derive(Serialize, Clone, Deserialize, Debug)]
pub struct GetMoneroSyncProgressResponse {
    #[typeshare(serialized_as = "number")]
    pub current_block: u64,
    #[typeshare(serialized_as = "number")]
    pub target_block: u64,
    #[typeshare(serialized_as = "number")]
    pub progress_percentage: f32,
}

impl Request for GetMoneroSyncProgressArgs {
    type Response = GetMoneroSyncProgressResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_manager = ctx
            .monero_manager
            .as_ref()
            .context("Monero wallet manager not available")?;
        let wallet = wallet_manager.main_wallet().await;

        let sync_progress = wallet.call(|wallet| wallet.sync_progress()).await;

        Ok(GetMoneroSyncProgressResponse {
            current_block: sync_progress.current_block,
            target_block: sync_progress.target_block,
            progress_percentage: sync_progress.percentage(),
        })
    }
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetPendingApprovalsResponse {
    pub approvals: Vec<crate::cli::api::tauri_bindings::ApprovalRequest>,
}
