use crate::{bitcoin::ExpiredTimelocks, monero, network::quote::BidQuote};
use anyhow::Result;
use bitcoin::Txid;
use serde::{Deserialize, Serialize};
use strum::Display;
use typeshare::typeshare;
use url::Url;
use uuid::Uuid;

const CLI_LOG_EMITTED_EVENT_NAME: &str = "cli-log-emitted";
const SWAP_PROGRESS_EVENT_NAME: &str = "swap-progress-update";
const SWAP_STATE_CHANGE_EVENT_NAME: &str = "swap-database-state-update";
const TIMELOCK_CHANGE_EVENT_NAME: &str = "timelock-change";
const CONTEXT_INIT_PROGRESS_EVENT_NAME: &str = "context-init-progress-update";

#[derive(Debug, Clone)]
pub struct TauriHandle(
    #[cfg(feature = "tauri")]
    #[cfg_attr(feature = "tauri", allow(unused))]
    std::sync::Arc<tauri::AppHandle>,
);

impl TauriHandle {
    #[cfg(feature = "tauri")]
    pub fn new(tauri_handle: tauri::AppHandle) -> Self {
        Self(
            #[cfg(feature = "tauri")]
            std::sync::Arc::new(tauri_handle),
        )
    }

    #[allow(unused_variables)]
    pub fn emit_tauri_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()> {
        #[cfg(feature = "tauri")]
        tauri::Emitter::emit(self.0.as_ref(), event, payload).map_err(anyhow::Error::from)?;

        Ok(())
    }
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
pub enum TauriContextInitializationProgress {
    OpeningBitcoinWallet,
    OpeningMoneroWallet,
    OpeningDatabase,
}

#[typeshare]
#[derive(Display, Clone, Serialize)]
#[serde(tag = "type", content = "content")]
pub enum TauriContextStatusEvent {
    NotInitialized,
    Initializing(TauriContextInitializationProgress),
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
    Initiated,
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
    Started {
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

/// This struct contains the settings for the Context
#[typeshare]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TauriSettings {
    /// This is used for estimating the target block for Bitcoin (fee)
    pub bitcoin_confirmation_target: u16,
    /// The URL of the Monero node e.g `http://xmr.node:18081`
    pub monero_node_url: Option<String>,
    /// The URL of the Electrum RPC server e.g `ssl://bitcoin.com:50001`
    #[typeshare(serialized_as = "string")]
    pub electrum_rpc_url: Option<Url>,
}
