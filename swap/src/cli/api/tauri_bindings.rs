use crate::bitcoin;
use crate::{bitcoin::ExpiredTimelocks, monero, network::quote::BidQuote};
use anyhow::{anyhow, Context, Result};
use bitcoin::Txid;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use strum::Display;
use tokio::sync::{oneshot, Mutex as TokioMutex};
use typeshare::typeshare;
use url::Url;
use uuid::Uuid;

use super::request::BalanceResponse;

const CLI_LOG_EMITTED_EVENT_NAME: &str = "cli-log-emitted";
const SWAP_PROGRESS_EVENT_NAME: &str = "swap-progress-update";
const SWAP_STATE_CHANGE_EVENT_NAME: &str = "swap-database-state-update";
const TIMELOCK_CHANGE_EVENT_NAME: &str = "timelock-change";
const CONTEXT_INIT_PROGRESS_EVENT_NAME: &str = "context-init-progress-update";
const BALANCE_CHANGE_EVENT_NAME: &str = "balance-change";
const BACKGROUND_REFUND_EVENT_NAME: &str = "background-refund";
const APPROVAL_EVENT_NAME: &str = "approval_event";

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LockBitcoinDetails {
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc_lock_amount: bitcoin::Amount,
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
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
    fn emit_approval(&self, event: ApprovalRequest) -> Result<()> {
        self.emit_tauri_event(APPROVAL_EVENT_NAME, event)
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
                .expect("it is later than the begin of the unix epoch")
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
            self.emit_approval(pending_event.clone())?;

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

                self.emit_approval(event)?;
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

    fn emit_swap_progress_event(&self, swap_id: Uuid, event: TauriSwapProgressEvent) {
        let _ = self.emit_tauri_event(
            SWAP_PROGRESS_EVENT_NAME,
            TauriSwapProgressEventWrapper { swap_id, event },
        );
    }

    fn emit_context_init_progress_event(&self, event: TauriContextStatusEvent) {
        let _ = self.emit_tauri_event(CONTEXT_INIT_PROGRESS_EVENT_NAME, event);
    }

    fn emit_cli_log_event(&self, event: TauriLogEvent) {
        let _ = self
            .emit_tauri_event(CLI_LOG_EMITTED_EVENT_NAME, event)
            .ok();
    }

    fn emit_swap_state_change_event(&self, swap_id: Uuid) {
        let _ = self.emit_tauri_event(
            SWAP_STATE_CHANGE_EVENT_NAME,
            TauriDatabaseStateEvent { swap_id },
        );
    }

    fn emit_timelock_change_event(&self, swap_id: Uuid, timelock: Option<ExpiredTimelocks>) {
        let _ = self.emit_tauri_event(
            TIMELOCK_CHANGE_EVENT_NAME,
            TauriTimelockChangeEvent { swap_id, timelock },
        );
    }

    fn emit_balance_update_event(&self, new_balance: bitcoin::Amount) {
        let _ = self.emit_tauri_event(
            BALANCE_CHANGE_EVENT_NAME,
            BalanceResponse {
                balance: new_balance,
            },
        );
    }

    fn emit_background_refund_event(&self, swap_id: Uuid, state: BackgroundRefundState) {
        let _ = self.emit_tauri_event(
            BACKGROUND_REFUND_EVENT_NAME,
            TauriBackgroundRefundEvent { swap_id, state },
        );
    }
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

#[typeshare]
#[derive(Display, Clone, Serialize)]
#[serde(tag = "componentName", content = "progress")]
pub enum TauriPartialInitProgress {
    OpeningBitcoinWallet(PendingCompleted<()>),
    DownloadingMoneroWalletRpc(PendingCompleted<DownloadProgress>),
    OpeningMoneroWallet(PendingCompleted<()>),
    OpeningDatabase(PendingCompleted<()>),
    EstablishingTorCircuits(PendingCompleted<()>),
}

#[typeshare]
#[derive(Display, Clone, Serialize)]
#[serde(tag = "type", content = "content")]
pub enum TauriContextStatusEvent {
    NotInitialized,
    Initializing(Vec<TauriPartialInitProgress>),
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
#[serde(tag = "type", content = "content")]
#[typeshare]
pub enum TauriSwapProgressEvent {
    RequestingQuote,
    Resuming,
    ReceivedQuote(BidQuote),
    WaitingForBtcDeposit {
        #[typeshare(serialized_as = "string")]
        deposit_address: bitcoin::Address,
        #[typeshare(serialized_as = "number")]
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        max_giveable: bitcoin::Amount,
        #[typeshare(serialized_as = "number")]
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        min_deposit_until_swap_will_start: bitcoin::Amount,
        #[typeshare(serialized_as = "number")]
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        max_deposit_until_maximum_amount_is_reached: bitcoin::Amount,
        #[typeshare(serialized_as = "number")]
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        min_bitcoin_lock_tx_fee: bitcoin::Amount,
        quote: BidQuote,
    },
    SwapSetupInflight {
        #[typeshare(serialized_as = "number")]
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        btc_lock_amount: bitcoin::Amount,
        #[typeshare(serialized_as = "number")]
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        btc_tx_lock_fee: bitcoin::Amount,
    },
    BtcLockTxInMempool {
        #[typeshare(serialized_as = "string")]
        btc_lock_txid: bitcoin::Txid,
        #[typeshare(serialized_as = "number")]
        btc_lock_confirmations: u64,
    },
    XmrLockTxInMempool {
        #[typeshare(serialized_as = "string")]
        xmr_lock_txid: monero::TxHash,
        #[typeshare(serialized_as = "number")]
        xmr_lock_tx_confirmations: u64,
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
#[typeshare]
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

#[derive(Serialize, Clone)]
#[typeshare]
pub struct TauriBackgroundRefundEvent {
    #[typeshare(serialized_as = "string")]
    swap_id: Uuid,
    state: BackgroundRefundState,
}

/// This struct contains the settings for the Context
#[typeshare]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TauriSettings {
    /// The URL of the Monero node e.g `http://xmr.node:18081`
    pub monero_node_url: Option<String>,
    /// The URL of the Electrum RPC server e.g `ssl://bitcoin.com:50001`
    #[typeshare(serialized_as = "string")]
    pub electrum_rpc_url: Option<Url>,
    /// Whether to initialize and use a tor client.
    pub use_tor: bool,
}
