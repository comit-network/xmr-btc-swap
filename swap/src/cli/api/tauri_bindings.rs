use crate::{bitcoin::ExpiredTimelocks, monero, network::quote::BidQuote};
use anyhow::{anyhow, Result};
use bitcoin::Txid;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
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

#[typeshare]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "content")]
pub enum ConfirmationRequestType {
    PreBtcLock { state2_json: String },
}

struct PendingConfirmation {
    responder: oneshot::Sender<bool>,
    expired: Arc<AtomicBool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[typeshare]
pub struct ConfirmationEventPayload {
    request_id: String,
    #[typeshare(serialized_as = "number")]
    timeout_secs: u64,
    details: ConfirmationRequestType,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[typeshare]
pub struct ConfirmationResolvedPayload {
    request_id: String,
}

#[cfg(feature = "tauri")]
struct TauriHandleInner {
    app_handle: tauri::AppHandle,
    pending_confirmations: TokioMutex<HashMap<Uuid, PendingConfirmation>>,
}

// Keep TauriHandle deriving Clone
#[derive(Clone)]
pub struct TauriHandle(
    #[cfg(feature = "tauri")]
    #[cfg_attr(feature = "tauri", allow(unused))]
    // Wrap the inner state management struct in Arc
    Arc<TauriHandleInner>,
);

impl TauriHandle {
    #[cfg(feature = "tauri")]
    pub fn new(tauri_handle: tauri::AppHandle) -> Self {
        Self(
            #[cfg(feature = "tauri")]
            Arc::new(TauriHandleInner {
                app_handle: tauri_handle,
                pending_confirmations: TokioMutex::new(HashMap::new()),
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

    // --- Confirmation Methods ---
    pub async fn request_confirmation(
        &self,
        request_type: ConfirmationRequestType,
        timeout_secs: u64,
    ) -> Result<bool> {
        #[cfg(not(feature = "tauri"))]
        {
            // If Tauri feature is not enabled, we cannot show UI.
            // Decide behavior: maybe auto-deny?
            tracing::warn!("Confirmation requested but Tauri feature not enabled. Auto-denying.");
            return Ok(false);
        }

        #[cfg(feature = "tauri")]
        {
            let request_id = Uuid::new_v4();
            let (tx, rx) = oneshot::channel();
            let expired = Arc::new(AtomicBool::new(false));
            let timeout_duration = Duration::from_secs(timeout_secs);

            let payload = ConfirmationEventPayload {
                request_id: request_id.to_string(),
                timeout_secs,
                details: request_type.clone(), // Clone for the event
            };

            // Emit event first
            self.emit_tauri_event("confirmation_request", payload)?;
            tracing::info!(%request_id, "Emitted confirmation request event");

            let pending_confirmation = PendingConfirmation {
                responder: tx,
                expired: expired.clone(),
            };

            // Lock map and insert
            {
                let mut pending_map = self.0.pending_confirmations.lock().await;
                pending_map.insert(request_id, pending_confirmation);
            }

            // Clone Arc for the timeout task
            let inner_clone = Arc::clone(&self.0);

            // Spawn timeout task
            tokio::spawn(async move {
                tokio::time::sleep(timeout_duration).await;
                if !expired.load(Ordering::SeqCst) {
                    let mut pending_map = inner_clone.pending_confirmations.lock().await;
                    if let Some(pending) = pending_map.remove(&request_id) {
                        tracing::warn!(%request_id, "Confirmation request timed out.");
                        let _ = pending.responder.send(false); // Send timeout signal (false = denied)

                        // Also emit resolved event on timeout
                        let _ = tauri::Emitter::emit(
                            &inner_clone.app_handle,
                            "confirmation_resolved",
                            ConfirmationResolvedPayload {
                                request_id: request_id.to_string(),
                            },
                        );
                    }
                }
            });

            // Wait for response from frontend (or timeout)
            rx.await
                .map_err(|_| anyhow!("Confirmation responder dropped"))
        }
    }

    pub async fn resolve_confirmation(&self, request_id: Uuid, accepted: bool) -> Result<()> {
        #[cfg(not(feature = "tauri"))]
        {
            // Should not be callable if tauri is not enabled, but handle defensively
            return Err(anyhow!(
                "Cannot resolve confirmation: Tauri feature not enabled."
            ));
        }

        #[cfg(feature = "tauri")]
        {
            let mut pending_map = self.0.pending_confirmations.lock().await;
            if let Some(pending) = pending_map.remove(&request_id) {
                if !pending.expired.swap(true, Ordering::SeqCst) {
                    // Send result only if not already expired
                    let _ = pending.responder.send(accepted);
                    tracing::info!(%request_id, %accepted, "Resolved confirmation request from frontend.");

                    // Emit resolution event
                    let payload = ConfirmationResolvedPayload {
                        request_id: request_id.to_string(),
                    };
                    self.emit_tauri_event("confirmation_resolved", payload)?;
                    Ok(())
                } else {
                    // Already expired and handled by timeout task
                    tracing::debug!(%request_id, "Confirmation already expired when frontend tried to resolve.");
                    // Return Ok because the resolution (timeout) happened, just not via this call.
                    // Or return Err? Let's return Ok for now.
                    Ok(())
                }
            } else {
                Err(anyhow!(
                    "Confirmation request ID not found (maybe already resolved or timed out)"
                ))
            }
        }
    }
    // --- End Confirmation Methods ---
}

pub trait TauriEmitter {
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
    fn emit_tauri_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()> {
        self.emit_tauri_event(event, payload)
    }
}

impl TauriEmitter for Option<TauriHandle> {
    fn emit_tauri_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()> {
        match self {
            Some(tauri) => tauri.emit_tauri_event(event, payload),
            None => Ok(()),
        }
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
}
