use super::request::BalanceResponse;
use crate::bitcoin;
use crate::{bitcoin::ExpiredTimelocks, monero, network::quote::BidQuote};
use anyhow::{anyhow, Context, Result};
use bitcoin::Txid;
use monero_rpc_pool::pool::PoolStatus;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use strum::Display;
use tokio::sync::{oneshot, Mutex as TokioMutex};
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
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
}

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "content")]
pub enum ApprovalRequestDetails {
    /// Request approval before locking Bitcoin.
    /// Contains specific details for review.
    LockBitcoin(LockBitcoinDetails),
}

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", content = "content")]
pub enum ApprovalRequest {
    Pending {
        request_id: String,
        #[typeshare(serialized_as = "number")]
        expiration_ts: u64,
        details: ApprovalRequestDetails,
    },
    Resolved {
        request_id: String,
        details: ApprovalRequestDetails,
    },
    Rejected {
        request_id: String,
        details: ApprovalRequestDetails,
    },
}

struct PendingApproval {
    responder: Option<oneshot::Sender<bool>>,
    details: ApprovalRequestDetails,
    #[allow(dead_code)]
    expiration_ts: u64,
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
    pending_approvals: TokioMutex<HashMap<Uuid, PendingApproval>>,
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
                pending_approvals: TokioMutex::new(HashMap::new()),
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
        self.emit_unified_event(TauriEvent::Approval(event))
    }

    pub async fn request_approval(
        &self,
        request_type: ApprovalRequestDetails,
        timeout_secs: u64,
    ) -> Result<bool> {
        #[cfg(not(feature = "tauri"))]
        {
            return Ok(true);
        }

        #[cfg(feature = "tauri")]
        {
            // Compute absolute expiration timestamp, and UUID for the request
            let request_id = Uuid::new_v4();
            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time to be after unix epoch (1970-01-01)")
                .as_secs();
            let expiration_ts = now_secs + timeout_secs;

            // Build the approval event
            let details = request_type.clone();
            let pending_event = ApprovalRequest::Pending {
                request_id: request_id.to_string(),
                expiration_ts,
                details: details.clone(),
            };

            // Emit the creation of the approval request to the frontend
            self.emit_approval(pending_event.clone());

            tracing::debug!(%request_id, request=?pending_event, "Emitted approval request event");

            // Construct the data structure we use to internally track the approval request
            let (responder, receiver) = oneshot::channel();
            let timeout_duration = Duration::from_secs(timeout_secs);

            let pending = PendingApproval {
                responder: Some(responder),
                details: request_type.clone(),
                expiration_ts,
            };

            // Lock map and insert the pending approval
            {
                let mut pending_map = self.0.pending_approvals.lock().await;
                pending_map.insert(request_id, pending);
            }

            // Determine if the request will be accepted or rejected
            // Either by being resolved by the user, or by timing out
            let accepted = tokio::select! {
                res = receiver => res.map_err(|_| anyhow!("Approval responder dropped"))?,
                _ = tokio::time::sleep(timeout_duration) => {
                    tracing::debug!(%request_id, "Approval request timed out and was therefore rejected");
                    false
                },
            };

            let mut map = self.0.pending_approvals.lock().await;
            if let Some(pending) = map.remove(&request_id) {
                let event = if accepted {
                    ApprovalRequest::Resolved {
                        request_id: request_id.to_string(),
                        details: pending.details,
                    }
                } else {
                    ApprovalRequest::Rejected {
                        request_id: request_id.to_string(),
                        details: pending.details,
                    }
                };

                self.emit_approval(event);
                tracing::debug!(%request_id, %accepted, "Resolved approval request");
            }

            Ok(accepted)
        }
    }

    pub async fn resolve_approval(&self, request_id: Uuid, accepted: bool) -> Result<()> {
        #[cfg(not(feature = "tauri"))]
        {
            return Err(anyhow!(
                "Cannot resolve approval: Tauri feature not enabled."
            ));
        }

        #[cfg(feature = "tauri")]
        {
            let mut pending_map = self.0.pending_approvals.lock().await;
            if let Some(pending) = pending_map.get_mut(&request_id) {
                let _ = pending
                    .responder
                    .take()
                    .context("Approval responder was already consumed")?
                    .send(accepted);

                Ok(())
            } else {
                Err(anyhow!("Approval not found or already handled"))
            }
        }
    }
}

pub trait TauriEmitter {
    fn request_approval<'life0, 'async_trait>(
        &'life0 self,
        request_type: ApprovalRequestDetails,
        timeout_secs: u64,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

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

impl TauriEmitter for TauriHandle {
    fn request_approval<'life0, 'async_trait>(
        &'life0 self,
        request_type: ApprovalRequestDetails,
        timeout_secs: u64,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(self.request_approval(request_type, timeout_secs))
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

impl TauriEmitter for Option<TauriHandle> {
    fn emit_tauri_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()> {
        match self {
            Some(tauri) => tauri.emit_tauri_event(event, payload),

            // If no TauriHandle is available, we just ignore the event and pretend as if it was emitted
            None => Ok(()),
        }
    }

    fn request_approval<'life0, 'async_trait>(
        &'life0 self,
        request_type: ApprovalRequestDetails,
        timeout_secs: u64,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            match self {
                Some(tauri) => tauri.request_approval(request_type, timeout_secs).await,
                None => Ok(true),
            }
        })
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

/// A handle for updating a specific background process's progress
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
        min_deposit_until_swap_will_start: bitcoin::Amount,
        #[typeshare(serialized_as = "number")]
        #[serde(with = "::bitcoin::amount::serde::as_sat")]
        max_deposit_until_maximum_amount_is_reached: bitcoin::Amount,
        #[typeshare(serialized_as = "number")]
        #[serde(with = "::bitcoin::amount::serde::as_sat")]
        min_bitcoin_lock_tx_fee: bitcoin::Amount,
        quote: BidQuote,
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
    },
    XmrLocked,
    EncryptedSignatureSent,
    BtcRedeemed,
    XmrRedeemInMempool {
        #[typeshare(serialized_as = "Vec<string>")]
        xmr_redeem_txids: Vec<monero::TxHash>,
        #[typeshare(serialized_as = "string")]
        xmr_redeem_address: monero::Address,
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
