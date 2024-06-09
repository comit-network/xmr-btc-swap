use crate::monero::TransferProof;
use crate::protocol::bob;
use crate::protocol::bob::BobState;
use monero_rpc::wallet::BlockHeight;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use std::fmt;

#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Bob {
    Started {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        btc_amount: bitcoin::Amount,
        #[serde_as(as = "DisplayFromStr")]
        change_address: bitcoin::Address,
    },
    ExecutionSetupDone {
        state2: bob::State2,
    },
    BtcLocked {
        state3: bob::State3,
        monero_wallet_restore_blockheight: BlockHeight,
    },
    XmrLockProofReceived {
        state: bob::State3,
        lock_transfer_proof: TransferProof,
        monero_wallet_restore_blockheight: BlockHeight,
    },
    XmrLocked {
        state4: bob::State4,
    },
    EncSigSent {
        state4: bob::State4,
    },
    BtcRedeemed(bob::State5),
    CancelTimelockExpired(bob::State6),
    BtcCancelled(bob::State6),
    Done(BobEndState),
}

#[derive(Clone, strum::Display, Debug, Deserialize, Serialize, PartialEq)]
pub enum BobEndState {
    SafelyAborted,
    XmrRedeemed { tx_lock_id: bitcoin::Txid },
    BtcRefunded(Box<bob::State6>),
    BtcPunished { tx_lock_id: bitcoin::Txid },
}

impl From<BobState> for Bob {
    fn from(bob_state: BobState) -> Self {
        match bob_state {
            BobState::Started {
                btc_amount,
                change_address,
            } => Bob::Started {
                btc_amount,
                change_address,
            },
            BobState::SwapSetupCompleted(state2) => Bob::ExecutionSetupDone { state2 },
            BobState::BtcLocked {
                state3,
                monero_wallet_restore_blockheight,
            } => Bob::BtcLocked {
                state3,
                monero_wallet_restore_blockheight,
            },
            BobState::XmrLockProofReceived {
                state,
                lock_transfer_proof,
                monero_wallet_restore_blockheight,
            } => Bob::XmrLockProofReceived {
                state,
                lock_transfer_proof,
                monero_wallet_restore_blockheight,
            },
            BobState::XmrLocked(state4) => Bob::XmrLocked { state4 },
            BobState::EncSigSent(state4) => Bob::EncSigSent { state4 },
            BobState::BtcRedeemed(state5) => Bob::BtcRedeemed(state5),
            BobState::CancelTimelockExpired(state6) => Bob::CancelTimelockExpired(state6),
            BobState::BtcCancelled(state6) => Bob::BtcCancelled(state6),
            BobState::BtcRefunded(state6) => Bob::Done(BobEndState::BtcRefunded(Box::new(state6))),
            BobState::XmrRedeemed { tx_lock_id } => {
                Bob::Done(BobEndState::XmrRedeemed { tx_lock_id })
            }
            BobState::BtcPunished { tx_lock_id } => {
                Bob::Done(BobEndState::BtcPunished { tx_lock_id })
            }
            BobState::SafelyAborted => Bob::Done(BobEndState::SafelyAborted),
        }
    }
}

impl From<Bob> for BobState {
    fn from(db_state: Bob) -> Self {
        match db_state {
            Bob::Started {
                btc_amount,
                change_address,
            } => BobState::Started {
                btc_amount,
                change_address,
            },
            Bob::ExecutionSetupDone { state2 } => BobState::SwapSetupCompleted(state2),
            Bob::BtcLocked {
                state3,
                monero_wallet_restore_blockheight,
            } => BobState::BtcLocked {
                state3,
                monero_wallet_restore_blockheight,
            },
            Bob::XmrLockProofReceived {
                state,
                lock_transfer_proof,
                monero_wallet_restore_blockheight,
            } => BobState::XmrLockProofReceived {
                state,
                lock_transfer_proof,
                monero_wallet_restore_blockheight,
            },
            Bob::XmrLocked { state4 } => BobState::XmrLocked(state4),
            Bob::EncSigSent { state4 } => BobState::EncSigSent(state4),
            Bob::BtcRedeemed(state5) => BobState::BtcRedeemed(state5),
            Bob::CancelTimelockExpired(state6) => BobState::CancelTimelockExpired(state6),
            Bob::BtcCancelled(state6) => BobState::BtcCancelled(state6),
            Bob::Done(end_state) => match end_state {
                BobEndState::SafelyAborted => BobState::SafelyAborted,
                BobEndState::XmrRedeemed { tx_lock_id } => BobState::XmrRedeemed { tx_lock_id },
                BobEndState::BtcRefunded(state6) => BobState::BtcRefunded(*state6),
                BobEndState::BtcPunished { tx_lock_id } => BobState::BtcPunished { tx_lock_id },
            },
        }
    }
}

impl fmt::Display for Bob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bob::Started { .. } => write!(f, "Started"),
            Bob::ExecutionSetupDone { .. } => f.write_str("Execution setup done"),
            Bob::BtcLocked { .. } => f.write_str("Bitcoin locked"),
            Bob::XmrLockProofReceived { .. } => {
                f.write_str("XMR lock transaction transfer proof received")
            }
            Bob::XmrLocked { .. } => f.write_str("Monero locked"),
            Bob::CancelTimelockExpired(_) => f.write_str("Cancel timelock is expired"),
            Bob::BtcCancelled(_) => f.write_str("Bitcoin refundable"),
            Bob::BtcRedeemed(_) => f.write_str("Monero redeemable"),
            Bob::Done(end_state) => write!(f, "Done: {}", end_state),
            Bob::EncSigSent { .. } => f.write_str("Encrypted signature sent"),
        }
    }
}
