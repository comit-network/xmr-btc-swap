use super::request::BalanceResponse;
use crate::bitcoin;
use crate::cli::api::request::{
    GetMoneroBalanceResponse, GetMoneroHistoryResponse, GetMoneroSyncProgressResponse,
};
use crate::cli::list_sellers::QuoteWithAddress;
use crate::monero::MoneroAddressPool;
use crate::{bitcoin::ExpiredTimelocks, monero, network::quote::BidQuote};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use bitcoin::Txid;
use ::bitcoin::address::NetworkUnchecked;
use monero_rpc_pool::pool::PoolStatus;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use strum::Display;
use tokio::sync::oneshot;
use typeshare::typeshare;
use uuid::Uuid;

#[typeshare]
#[derive(Clone, Serialize)]
#[serde(tag = "channelName", content = "event")]
pub enum TauriEvent {
    SwapProgress(TauriSwapProgressEventWrapper),
    ContextInitProgress(TauriContextStatusEvent),
    CliLog(TauriLogEvent),
    BalanceChange(BalanceResponse),
    SwapDatabaseStateUpdate(TauriDatabaseStateEvent),
    TimelockChange(TauriTimelockChangeEvent),
    Approval(ApprovalRequest),
    BackgroundProgress(TauriBackgroundProgressWrapper),
    PoolStatusUpdate(PoolStatus),
    MoneroWalletUpdate(MoneroWalletUpdate),
}

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "content")]
pub enum MoneroWalletUpdate {
    BalanceChange(GetMoneroBalanceResponse),
    SyncProgress(GetMoneroSyncProgressResponse),
    HistoryUpdate(GetMoneroHistoryResponse),
}

const TAURI_UNIFIED_EVENT_NAME: &str = "tauri-unified-event";

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LockBitcoinDetails {
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub btc_lock_amount: bitcoin::Amount,
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub btc_network_fee: bitcoin::Amount,
    #[typeshare(serialized_as = "number")]
    pub xmr_receive_amount: monero::Amount,
    pub monero_receive_pool: MoneroAddressPool,
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
}

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectMakerDetails {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub btc_amount_to_swap: bitcoin::Amount,
    pub maker: QuoteWithAddress,
}

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendMoneroDetails {
    /// Destination address for the Monero transfer
    #[typeshare(serialized_as = "string")]
    pub address: String,
    /// Amount to send
    #[typeshare(serialized_as = "number")]
    pub amount: monero::Amount,
    /// Transaction fee
    #[typeshare(serialized_as = "number")]
    pub fee: monero::Amount,
}

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PasswordRequestDetails {
    /// The wallet file path that requires a password
    pub wallet_path: String,
}

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "content")]
pub enum SeedChoice {
    RandomSeed,
    FromSeed { seed: String },
    FromWalletPath { wallet_path: String },
    Legacy,
}

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SeedSelectionDetails {
    /// List of recently used wallet paths
    pub recent_wallets: Vec<String>,
}

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectOfferApprovalRequest {
    #[typeshare(serialized_as = "Option<string>")]
    pub bitcoin_change_address: Option<bitcoin::Address<NetworkUnchecked>>,
    pub monero_receive_pool: MoneroAddressPool,
}

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApprovalRequest {
    request: ApprovalRequestType,
    request_status: RequestStatus,
    #[typeshare(serialized_as = "string")]
    request_id: Uuid,
}

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "content")]
pub enum ApprovalRequestType {
    /// Request approval before locking Bitcoin.
    /// Contains specific details for review.
    LockBitcoin(LockBitcoinDetails),
    /// Request approval for maker selection.
    /// Contains available makers and swap details.
    SelectMaker(SelectMakerDetails),
    /// Request seed selection from user.
    /// User can choose between random seed, provide their own, or select wallet file.
    SeedSelection(SeedSelectionDetails),
    /// Request approval for publishing a Monero transaction.
    SendMonero(SendMoneroDetails),
    /// Request password for wallet file.
    /// User must provide password to unlock the selected wallet.
    PasswordRequest(PasswordRequestDetails),
}

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", content = "content")]
pub enum RequestStatus {
    Pending {
        #[typeshare(serialized_as = "number")]
        expiration_ts: u64,
    },
    Resolved {
        #[typeshare(serialized_as = "object")]
        approve_input: serde_json::Value,
    },
    Rejected,
}

struct PendingApproval {
    responder: Option<oneshot::Sender<serde_json::Value>>,
    #[allow(dead_code)]
    expiration_ts: u64,
    request: ApprovalRequest,
}

impl Drop for PendingApproval {
    fn drop(&mut self) {
        if let Some(responder) = self.responder.take() {
            tracing::debug!("Dropping pending approval because handle was dropped");
            let _ = responder.send(serde_json::Value::Bool(false));
        }
    }
}

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TorBootstrapStatus {
    pub frac: f32,
    pub ready_for_traffic: bool,
    pub blockage: Option<String>,
}

#[cfg(feature = "tauri")]
struct TauriHandleInner {
    app_handle: tauri::AppHandle,
    pending_approvals: Arc<Mutex<HashMap<Uuid, PendingApproval>>>,
}

#[derive(Clone)]
pub struct TauriHandle(
    #[cfg(feature = "tauri")]
    #[cfg_attr(feature = "tauri", allow(unused))]
    Arc<TauriHandleInner>,
);

impl TauriHandle {
    #[cfg(feature = "tauri")]
    pub fn new(tauri_handle: tauri::AppHandle) -> Self {
        use std::collections::HashMap;

        Self(
            #[cfg(feature = "tauri")]
            Arc::new(TauriHandleInner {
                app_handle: tauri_handle,
                pending_approvals: Arc::new(Mutex::new(HashMap::new())),
            }),
        )
    }

    #[allow(unused_variables)]
    pub fn emit_tauri_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()> {
        #[cfg(feature = "tauri")]
        {
            let inner = self.0.as_ref();
            tauri::Emitter::emit(&inner.app_handle, event, payload).map_err(anyhow::Error::from)?;
        }

        Ok(())
    }

    /// Helper to emit a approval event via the unified event name
    fn emit_approval(&self, event: ApprovalRequest) {
        tracing::debug!(?event, "Emitting approval event");
        self.emit_unified_event(TauriEvent::Approval(event))
    }

    pub async fn request_approval<Response>(
        &self,
        request_type: ApprovalRequestType,
        timeout_secs: Option<u64>,
    ) -> Result<Response>
    where
        Response: serde::de::DeserializeOwned + Clone + Serialize,
    {
        #[cfg(not(feature = "tauri"))]
        {
            bail!("Tauri feature not enabled");
        }

        #[cfg(feature = "tauri")]
        {
            // Create the approval request
            // Generate the UUID
            // Set the expiration timestamp
            let (responder, receiver) = oneshot::channel();
            let request_id = Uuid::new_v4();
            let timeout_secs = timeout_secs.unwrap_or(60 * 60 * 24 * 7);
            let timeout_duration = Duration::from_secs(timeout_secs);
            let expiration_ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| anyhow!("Failed to get current time: {}", e))?
                .as_secs()
                + timeout_duration.as_secs();
            let request = ApprovalRequest {
                request: request_type,
                request_status: RequestStatus::Pending { expiration_ts },
                request_id,
            };

            // Emit the "pending" event
            self.emit_approval(request.clone());

            tracing::debug!(%request, "Emitted approval request event");

            let pending = PendingApproval {
                responder: Some(responder),
                expiration_ts: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| anyhow!("Failed to get current time: {}", e))?
                    .as_secs()
                    + timeout_secs,
                request: request.clone(),
            };

            // Lock map and insert the pending approval
            {
                let mut pending_map = self
                    .0
                    .pending_approvals
                    .lock()
                    .map_err(|e| anyhow!("Failed to acquire approval lock: {}", e))?;
                pending_map.insert(request_id, pending);
            }

            // Create cleanup guard to handle cancellation
            let mut cleanup_guard = ApprovalCleanupGuard::new(
                request_id,
                self.clone(),
                self.0.pending_approvals.clone(),
            );

            // Determine if the request will be accepted or rejected
            // Either by being resolved by the user, or by timing out
            let unparsed_response = tokio::select! {
                res = receiver => Some(res.map_err(|_| anyhow!("Approval responder dropped"))?),
                _ = tokio::time::sleep(timeout_duration) => {
                    None
                },
            };

            let maybe_response: Option<Response> = match &unparsed_response {
                Some(value) => serde_json::from_value(value.clone())
                    .inspect_err(|e| {
                        tracing::error!("Failed to parse approval response to expected type: {}", e)
                    })
                    .ok(),
                None => None,
            };

            let mut map = self
                .0
                .pending_approvals
                .lock()
                .map_err(|e| anyhow!("Failed to acquire approval lock: {}", e))?;

            if let Some(_pending) = map.remove(&request_id) {
                let status = match &maybe_response {
                    Some(_) => RequestStatus::Resolved {
                        approve_input: unparsed_response.unwrap_or(serde_json::Value::Bool(false)),
                    },
                    None => RequestStatus::Rejected,
                };

                // Set the status and emit the event
                let mut approval = request.clone();
                approval.request_status = status;
                self.emit_approval(approval.clone());

                tracing::debug!(%approval, "Resolved approval request");
            }

            cleanup_guard.disarm();

            tracing::debug!("Returning approval response");

            maybe_response.context("Approval was rejected")
        }
    }

    pub async fn resolve_approval(
        &self,
        request_id: Uuid,
        response: serde_json::Value,
    ) -> Result<()> {
        #[cfg(not(feature = "tauri"))]
        {
            Err(anyhow!(
                "Cannot resolve approval: Tauri feature not enabled."
            ))
        }

        #[cfg(feature = "tauri")]
        {
            let mut pending_map = self
                .0
                .pending_approvals
                .lock()
                .map_err(|e| anyhow!("Failed to acquire approval lock: {}", e))?;
            if let Some(mut pending) = pending_map.remove(&request_id) {
                // Send response through oneshot channel
                if let Some(responder) = pending.responder.take() {
                    let _ = responder.send(response);
                    Ok(())
                } else {
                    Err(anyhow!("Approval responder was already consumed"))
                }
            } else {
                Err(anyhow!("Approval not found or already handled"))
            }
        }
    }

    pub async fn reject_approval(&self, request_id: Uuid) -> Result<()> {
        #[cfg(not(feature = "tauri"))]
        {
            Err(anyhow!(
                "Cannot reject approval: Tauri feature not enabled."
            ))
        }

        #[cfg(feature = "tauri")]
        {
            let mut pending_map = self
                .0
                .pending_approvals
                .lock()
                .map_err(|e| anyhow!("Failed to acquire approval lock: {}", e))?;
            if let Some(mut pending) = pending_map.remove(&request_id) {
                // Send rejection through oneshot channel
                if let Some(responder) = pending.responder.take() {
                    let _ = responder.send(serde_json::Value::Null);

                    // Emit the rejection event
                    let mut approval = pending.request.clone();
                    approval.request_status = RequestStatus::Rejected;
                    self.emit_approval(approval);

                    Ok(())
                } else {
                    Err(anyhow!("Approval responder was already consumed"))
                }
            } else {
                Err(anyhow!("Approval not found or already handled"))
            }
        }
    }
}

impl Display for ApprovalRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.request {
            ApprovalRequestType::LockBitcoin(..) => write!(f, "LockBitcoin()"),
            ApprovalRequestType::SelectMaker(..) => write!(f, "SelectMaker()"),
            ApprovalRequestType::SeedSelection(_) => write!(f, "SeedSelection()"),
            ApprovalRequestType::SendMonero(_) => write!(f, "SendMonero()"),
            ApprovalRequestType::PasswordRequest(_) => write!(f, "PasswordRequest()"),
        }
    }
}

#[async_trait]
pub trait TauriEmitter {
    async fn request_bitcoin_approval(
        &self,
        details: LockBitcoinDetails,
        timeout_secs: u64,
    ) -> Result<bool>;

    async fn request_maker_selection(
        &self,
        details: SelectMakerDetails,
        timeout_secs: u64,
    ) -> Result<Option<SelectOfferApprovalRequest>>;

    async fn request_seed_selection(&self) -> Result<SeedChoice>;

    async fn request_seed_selection_with_recent_wallets(
        &self,
        recent_wallets: Vec<String>,
    ) -> Result<SeedChoice>;

    async fn request_password(&self, wallet_path: String) -> Result<String>;

    fn emit_tauri_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()>;

    fn emit_unified_event(&self, event: TauriEvent) {
        let _ = self.emit_tauri_event(TAURI_UNIFIED_EVENT_NAME, event);
    }

    // Restore default implementations below
    fn emit_swap_progress_event(&self, swap_id: Uuid, event: TauriSwapProgressEvent) {
        self.emit_unified_event(TauriEvent::SwapProgress(TauriSwapProgressEventWrapper {
            swap_id,
            event,
        }));
    }

    fn emit_context_init_progress_event(&self, event: TauriContextStatusEvent) {
        self.emit_unified_event(TauriEvent::ContextInitProgress(event));
    }

    fn emit_cli_log_event(&self, event: TauriLogEvent) {
        self.emit_unified_event(TauriEvent::CliLog(event));
    }

    fn emit_swap_state_change_event(&self, swap_id: Uuid) {
        self.emit_unified_event(TauriEvent::SwapDatabaseStateUpdate(
            TauriDatabaseStateEvent { swap_id },
        ));
    }

    fn emit_timelock_change_event(&self, swap_id: Uuid, timelock: Option<ExpiredTimelocks>) {
        self.emit_unified_event(TauriEvent::TimelockChange(TauriTimelockChangeEvent {
            swap_id,
            timelock,
        }));
    }

    fn emit_balance_update_event(&self, new_balance: bitcoin::Amount) {
        self.emit_unified_event(TauriEvent::BalanceChange(BalanceResponse {
            balance: new_balance,
        }));
    }

    fn emit_background_progress(&self, id: Uuid, event: TauriBackgroundProgress) {
        self.emit_unified_event(TauriEvent::BackgroundProgress(
            TauriBackgroundProgressWrapper { id, event },
        ));
    }

    fn emit_pool_status_update(&self, status: PoolStatus) {
        self.emit_unified_event(TauriEvent::PoolStatusUpdate(status));
    }

    /// Create a new background progress handle for tracking a specific type of progress
    fn new_background_process<T: Clone>(
        &self,
        component: fn(PendingCompleted<T>) -> TauriBackgroundProgress,
    ) -> TauriBackgroundProgressHandle<T>;

    fn new_background_process_with_initial_progress<T: Clone>(
        &self,
        component: fn(PendingCompleted<T>) -> TauriBackgroundProgress,
        initial_progress: T,
    ) -> TauriBackgroundProgressHandle<T>;
}

#[async_trait]
impl TauriEmitter for TauriHandle {
    async fn request_bitcoin_approval(
        &self,
        details: LockBitcoinDetails,
        timeout_secs: u64,
    ) -> Result<bool> {
        Ok(self
            .request_approval(
                ApprovalRequestType::LockBitcoin(details),
                Some(timeout_secs),
            )
            .await
            .unwrap_or(false))
    }

    async fn request_maker_selection(
        &self,
        details: SelectMakerDetails,
        timeout_secs: u64,
    ) -> Result<Option<SelectOfferApprovalRequest>> {
        Ok(self
            .request_approval(
                ApprovalRequestType::SelectMaker(details),
                Some(timeout_secs),
            )
            .await
            .unwrap_or(None))
    }

    async fn request_seed_selection(&self) -> Result<SeedChoice> {
        self.request_seed_selection_with_recent_wallets(vec![])
            .await
    }

    async fn request_seed_selection_with_recent_wallets(
        &self,
        recent_wallets: Vec<String>,
    ) -> Result<SeedChoice> {
        let details = SeedSelectionDetails { recent_wallets };
        self.request_approval(ApprovalRequestType::SeedSelection(details), None)
            .await
    }

    async fn request_password(&self, wallet_path: String) -> Result<String> {
        let details = PasswordRequestDetails { wallet_path };
        self.request_approval(ApprovalRequestType::PasswordRequest(details), None)
            .await
    }

    fn emit_tauri_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()> {
        self.emit_tauri_event(event, payload)
    }

    fn new_background_process<T: Clone>(
        &self,
        component: fn(PendingCompleted<T>) -> TauriBackgroundProgress,
    ) -> TauriBackgroundProgressHandle<T> {
        let id = Uuid::new_v4();

        TauriBackgroundProgressHandle {
            id,
            component,
            emitter: Some(self.clone()),
            is_finished: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn new_background_process_with_initial_progress<T: Clone>(
        &self,
        component: fn(PendingCompleted<T>) -> TauriBackgroundProgress,
        initial_progress: T,
    ) -> TauriBackgroundProgressHandle<T> {
        let background_process_handle = self.new_background_process(component);
        background_process_handle.update(initial_progress);
        background_process_handle
    }
}

#[async_trait]
impl TauriEmitter for Option<TauriHandle> {
    fn emit_tauri_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()> {
        match self {
            Some(tauri) => tauri.emit_tauri_event(event, payload),

            // If no TauriHandle is available, we just ignore the event and pretend as if it was emitted
            None => Ok(()),
        }
    }

    async fn request_bitcoin_approval(
        &self,
        details: LockBitcoinDetails,
        timeout_secs: u64,
    ) -> Result<bool> {
        match self {
            Some(tauri) => tauri.request_bitcoin_approval(details, timeout_secs).await,
            None => bail!("No Tauri handle available"),
        }
    }

    async fn request_maker_selection(
        &self,
        details: SelectMakerDetails,
        timeout_secs: u64,
    ) -> Result<Option<SelectOfferApprovalRequest>> {
        match self {
            Some(tauri) => tauri.request_maker_selection(details, timeout_secs).await,
            None => bail!("No Tauri handle available"),
        }
    }

    async fn request_seed_selection(&self) -> Result<SeedChoice> {
        match self {
            Some(tauri) => tauri.request_seed_selection().await,
            None => bail!("No Tauri handle available"),
        }
    }

    async fn request_seed_selection_with_recent_wallets(
        &self,
        recent_wallets: Vec<String>,
    ) -> Result<SeedChoice> {
        match self {
            Some(tauri) => {
                tauri
                    .request_seed_selection_with_recent_wallets(recent_wallets)
                    .await
            }
            None => bail!("No Tauri handle available"),
        }
    }

    async fn request_password(&self, wallet_path: String) -> Result<String> {
        match self {
            Some(tauri) => tauri.request_password(wallet_path).await,
            None => bail!("No Tauri handle available"),
        }
    }

    fn new_background_process<T: Clone>(
        &self,
        component: fn(PendingCompleted<T>) -> TauriBackgroundProgress,
    ) -> TauriBackgroundProgressHandle<T> {
        let id = Uuid::new_v4();

        TauriBackgroundProgressHandle {
            id,
            component,
            emitter: self.clone(),
            is_finished: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn new_background_process_with_initial_progress<T: Clone>(
        &self,
        component: fn(PendingCompleted<T>) -> TauriBackgroundProgress,
        initial_progress: T,
    ) -> TauriBackgroundProgressHandle<T> {
        let background_process_handle = self.new_background_process(component);
        background_process_handle.update(initial_progress);
        background_process_handle
    }
}

impl TauriHandle {
    #[cfg(feature = "tauri")]
    pub async fn get_pending_approvals(&self) -> Result<Vec<ApprovalRequest>> {
        let pending_map = self
            .0
            .pending_approvals
            .lock()
            .map_err(|e| anyhow!("Failed to acquire approval lock: {}", e))?;

        let approvals: Vec<ApprovalRequest> = pending_map
            .values()
            .map(|pending| pending.request.clone())
            .collect();

        Ok(approvals)
    }

    #[cfg(not(feature = "tauri"))]
    pub async fn get_pending_approvals(&self) -> Result<Vec<ApprovalRequest>> {
        Ok(Vec::new())
    }
}

/// A handle for updating a specific background progress's progress
///
/// # Examples
///
/// ```
/// // For Tor bootstrap progress
/// use self::{TauriHandle, TauriBackgroundProgress, TorBootstrapStatus};
///
/// // In a real scenario, tauri_handle would be properly initialized.
/// // For this example, we'll use Option<TauriHandle>::None,
/// // which allows calling new_background_process.
/// let tauri_handle: Option<TauriHandle> = None;
///
/// let tor_progress = tauri_handle.new_background_process(
///     |status| TauriBackgroundProgress::EstablishingTorCircuits(status)
/// );
///
/// // Define a sample TorBootstrapStatus
/// let tor_status = TorBootstrapStatus {
///     frac: 0.5,
///     ready_for_traffic: false,
///     blockage: None,
/// };
///
/// tor_progress.update(tor_status);
/// tor_progress.finish();
/// ```
#[derive(Clone)]
pub struct TauriBackgroundProgressHandle<T: Clone> {
    id: Uuid,
    component: fn(PendingCompleted<T>) -> TauriBackgroundProgress,
    emitter: Option<TauriHandle>,
    is_finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl<T: Clone> TauriBackgroundProgressHandle<T> {
    /// Update the progress of this background process
    /// Updates after finish() has been called will be ignored
    #[cfg(feature = "tauri")]
    pub fn update(&self, progress: T) {
        // Silently fail if the background process has already been finished
        if self.is_finished.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }

        if let Some(emitter) = &self.emitter {
            emitter.emit_background_progress(
                self.id,
                (self.component)(PendingCompleted::Pending(progress)),
            );
        }
    }

    #[cfg(not(feature = "tauri"))]
    pub fn update(&self, _progress: T) {
        // Do nothing when tauri is not enabled
    }

    /// Mark this background process as completed
    /// All subsequent update() calls will be ignored
    pub fn finish(&self) {
        self.is_finished
            .store(true, std::sync::atomic::Ordering::Relaxed);

        if let Some(emitter) = &self.emitter {
            emitter
                .emit_background_progress(self.id, (self.component)(PendingCompleted::Completed));
        }
    }
}

impl<T: Clone> Drop for TauriBackgroundProgressHandle<T> {
    fn drop(&mut self) {
        (*self).finish();
    }
}

#[typeshare]
#[derive(Display, Clone, Serialize)]
#[serde(tag = "type", content = "content")]
pub enum PendingCompleted<P> {
    Pending(P),
    Completed,
}

#[derive(Serialize, Clone)]
#[typeshare]
pub struct DownloadProgress {
    // Progress of the download in percent (0-100)
    #[typeshare(serialized_as = "number")]
    pub progress: u64,
    // Size of the download file in bytes
    #[typeshare(serialized_as = "number")]
    pub size: u64,
}

#[derive(Clone, Serialize)]
#[typeshare]
#[serde(tag = "type", content = "content")]
pub enum TauriBitcoinSyncProgress {
    Known {
        // Number of addresses processed
        #[typeshare(serialized_as = "number")]
        consumed: u64,
        // Total number of addresses to process
        #[typeshare(serialized_as = "number")]
        total: u64,
    },
    Unknown,
}

#[derive(Clone, Serialize)]
#[typeshare]
#[serde(tag = "type", content = "content")]
pub enum TauriBitcoinFullScanProgress {
    Known {
        #[typeshare(serialized_as = "number")]
        current_index: u64,
        #[typeshare(serialized_as = "number")]
        assumed_total: u64,
    },
    Unknown,
}

#[derive(Serialize, Clone)]
#[typeshare]
pub struct BackgroundRefundProgress {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
}

#[typeshare]
#[derive(Display, Clone, Serialize)]
#[serde(tag = "componentName", content = "progress")]
pub enum TauriBackgroundProgress {
    OpeningBitcoinWallet(PendingCompleted<()>),
    OpeningMoneroWallet(PendingCompleted<()>),
    OpeningDatabase(PendingCompleted<()>),
    EstablishingTorCircuits(PendingCompleted<TorBootstrapStatus>),
    SyncingBitcoinWallet(PendingCompleted<TauriBitcoinSyncProgress>),
    FullScanningBitcoinWallet(PendingCompleted<TauriBitcoinFullScanProgress>),
    BackgroundRefund(PendingCompleted<BackgroundRefundProgress>),
    ListSellers(PendingCompleted<ListSellersProgress>),
}

#[typeshare]
#[derive(Clone, Serialize)]
pub struct TauriBackgroundProgressWrapper {
    #[typeshare(serialized_as = "string")]
    id: Uuid,
    event: TauriBackgroundProgress,
}

#[typeshare]
#[derive(Display, Clone, Serialize)]
pub enum TauriContextStatusEvent {
    NotInitialized,
    Initializing,
    Available,
    Failed,
}

#[derive(Serialize, Clone)]
#[typeshare]
pub struct TauriSwapProgressEventWrapper {
    #[typeshare(serialized_as = "string")]
    swap_id: Uuid,
    event: TauriSwapProgressEvent,
}

#[derive(Serialize, Clone)]
#[typeshare]
#[serde(tag = "type", content = "content")]
pub enum TauriSwapProgressEvent {
    RequestingQuote,
    Resuming,
    ReceivedQuote(BidQuote),
    WaitingForBtcDeposit {
        #[typeshare(serialized_as = "string")]
        deposit_address: bitcoin::Address,
        #[typeshare(serialized_as = "number")]
        #[serde(with = "::bitcoin::amount::serde::as_sat")]
        max_giveable: bitcoin::Amount,
        #[typeshare(serialized_as = "number")]
        #[serde(with = "::bitcoin::amount::serde::as_sat")]
        min_bitcoin_lock_tx_fee: bitcoin::Amount,
        known_quotes: Vec<QuoteWithAddress>,
    },
    SwapSetupInflight {
        #[typeshare(serialized_as = "number")]
        #[serde(with = "::bitcoin::amount::serde::as_sat")]
        btc_lock_amount: bitcoin::Amount,
        #[typeshare(serialized_as = "number")]
        #[serde(with = "::bitcoin::amount::serde::as_sat")]
        btc_tx_lock_fee: bitcoin::Amount,
    },
    BtcLockTxInMempool {
        #[typeshare(serialized_as = "string")]
        btc_lock_txid: bitcoin::Txid,
        #[typeshare(serialized_as = "Option<number>")]
        btc_lock_confirmations: Option<u64>,
    },
    XmrLockTxInMempool {
        #[typeshare(serialized_as = "string")]
        xmr_lock_txid: monero::TxHash,
        #[typeshare(serialized_as = "Option<number>")]
        xmr_lock_tx_confirmations: Option<u64>,
        #[typeshare(serialized_as = "number")]
        xmr_lock_tx_target_confirmations: u64,
    },
    XmrLocked,
    EncryptedSignatureSent,
    RedeemingMonero,
    WaitingForXmrConfirmationsBeforeRedeem {
        #[typeshare(serialized_as = "string")]
        xmr_lock_txid: monero::TxHash,
        #[typeshare(serialized_as = "number")]
        xmr_lock_tx_confirmations: u64,
        #[typeshare(serialized_as = "number")]
        xmr_lock_tx_target_confirmations: u64,
    },
    XmrRedeemInMempool {
        #[typeshare(serialized_as = "Vec<string>")]
        xmr_redeem_txids: Vec<monero::TxHash>,
        xmr_receive_pool: MoneroAddressPool,
    },
    CancelTimelockExpired,
    BtcCancelled {
        #[typeshare(serialized_as = "string")]
        btc_cancel_txid: Txid,
    },
    // tx_early_refund has been published but has not been confirmed yet
    // we can still transition into BtcRefunded from here
    BtcEarlyRefundPublished {
        #[typeshare(serialized_as = "string")]
        btc_early_refund_txid: Txid,
    },
    // tx_refund has been published but has not been confirmed yet
    // we can still transition into BtcEarlyRefunded from here
    BtcRefundPublished {
        #[typeshare(serialized_as = "string")]
        btc_refund_txid: Txid,
    },
    // tx_early_refund has been confirmed
    BtcEarlyRefunded {
        #[typeshare(serialized_as = "string")]
        btc_early_refund_txid: Txid,
    },
    // tx_refund has been confirmed
    BtcRefunded {
        #[typeshare(serialized_as = "string")]
        btc_refund_txid: Txid,
    },
    BtcPunished,
    AttemptingCooperativeRedeem,
    CooperativeRedeemAccepted,
    CooperativeRedeemRejected {
        reason: String,
    },
    Released,
}

/// This event is emitted whenever there is a log message issued in the CLI.
///
/// It contains a json serialized object containing the log message and metadata.
#[typeshare]
#[derive(Debug, Serialize, Clone)]
pub struct TauriLogEvent {
    /// The serialized object containing the log message and metadata.
    pub buffer: String,
}

#[derive(Serialize, Clone)]
#[typeshare]
pub struct TauriDatabaseStateEvent {
    #[typeshare(serialized_as = "string")]
    swap_id: Uuid,
}

#[derive(Serialize, Clone)]
#[typeshare]
pub struct TauriTimelockChangeEvent {
    #[typeshare(serialized_as = "string")]
    swap_id: Uuid,
    timelock: Option<ExpiredTimelocks>,
}

#[derive(Serialize, Clone)]
#[typeshare]
#[serde(tag = "type", content = "content")]
pub enum BackgroundRefundState {
    Started,
    Failed { error: String },
    Completed,
}

#[typeshare]
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "content")]
pub enum MoneroNodeConfig {
    Pool,
    SingleNode { url: String },
}

/// This struct contains the settings for the Context
#[typeshare]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TauriSettings {
    /// Configuration for Monero node connection
    pub monero_node_config: MoneroNodeConfig,
    /// The URLs of the Electrum RPC servers e.g `["ssl://bitcoin.com:50001", "ssl://backup.com:50001"]`
    pub electrum_rpc_urls: Vec<String>,
    /// Whether to initialize and use a tor client.
    pub use_tor: bool,
}

#[typeshare]
#[derive(Debug, Serialize, Clone)]
pub struct ListSellersProgress {
    pub rendezvous_points_connected: u32,
    pub rendezvous_points_total: u32,
    pub rendezvous_points_failed: u32,
    pub peers_discovered: u32,
    pub quotes_received: u32,
    pub quotes_failed: u32,
}

// Add this struct before the TauriHandle implementation
struct ApprovalCleanupGuard {
    request_id: Option<Uuid>,
    approval_store: Arc<Mutex<HashMap<Uuid, PendingApproval>>>,
    handle: TauriHandle,
}

impl ApprovalCleanupGuard {
    fn new(
        request_id: Uuid,
        handle: TauriHandle,
        approval_store: Arc<Mutex<HashMap<Uuid, PendingApproval>>>,
    ) -> Self {
        Self {
            request_id: Some(request_id),
            handle,
            approval_store,
        }
    }

    /// Disarm the guard so it won't cleanup on drop (call when normally resolved)
    fn disarm(&mut self) {
        self.request_id = None;
    }
}

impl Drop for ApprovalCleanupGuard {
    fn drop(&mut self) {
        if let Some(request_id) = self.request_id.take() {
            let approval_store = self.approval_store.clone();
            let handle = self.handle.clone();

            tokio::task::spawn_blocking(move || {
                tracing::debug!(%request_id, "Approval handle dropped, we should cleanup now");

                // Lock the Mutex
                if let Ok(mut approval_store) = approval_store.lock() {
                    // Check if the request id still present in the map
                    if let Some(mut pending_approval) = approval_store.remove(&request_id) {
                        // If there is still someone listening, send a rejection
                        if let Some(responder) = pending_approval.responder.take() {
                            let _ = responder.send(serde_json::Value::Bool(false));
                        }

                        handle.emit_approval(ApprovalRequest {
                            request: pending_approval.request.clone().request,
                            request_status: RequestStatus::Rejected,
                            request_id,
                        });
                    }
                }
            });
        }
    }
}
