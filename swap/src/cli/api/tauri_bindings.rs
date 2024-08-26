/**
 * TOOD: Perhaps we should move this to the `src-tauri` package.
 */
use anyhow::Result;
use bitcoin::Txid;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use typeshare::typeshare;
use uuid::Uuid;

use crate::{monero, network::quote::BidQuote};

static SWAP_PROGRESS_EVENT_NAME: &str = "swap-progress-update";

#[derive(Debug, Clone)]
pub struct TauriHandle(Arc<AppHandle>);

impl TauriHandle {
    pub fn new(tauri_handle: AppHandle) -> Self {
        Self(Arc::new(tauri_handle))
    }

    pub fn emit_tauri_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()> {
        self.0.emit(event, payload).map_err(|e| e.into())
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
    BtcRedeemed,
    XmrRedeemInMempool {
        #[typeshare(serialized_as = "string")]
        xmr_redeem_txid: monero::TxHash,
        #[typeshare(serialized_as = "string")]
        xmr_redeem_address: monero::Address,
    },
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
