use crate::{bob::swap::BobState, SwapAmounts};
use bitcoin::hashes::core::fmt::Display;
use serde::{Deserialize, Serialize};
use xmr_btc::bob;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Bob {
    Started {
        state0: bob::State0,
        amounts: SwapAmounts,
    },
    Negotiated {
        state2: bob::State2,
    },
    BtcLocked {
        state3: bob::State3,
    },
    XmrLocked {
        state4: bob::State4,
    },
    EncSigSent {
        state4: bob::State4,
    },
    BtcRedeemed(bob::State5),
    CancelTimelockExpired(bob::State4),
    BtcCancelled(bob::State4),
    Done(BobEndState),
}

#[derive(Clone, strum::Display, Debug, Deserialize, Serialize, PartialEq)]
pub enum BobEndState {
    SafelyAborted,
    XmrRedeemed,
    BtcRefunded(Box<bob::State4>),
    BtcPunished,
}

impl From<BobState> for Bob {
    fn from(bob_state: BobState) -> Self {
        match bob_state {
            BobState::Started { state0, amounts } => Bob::Started { state0, amounts },
            BobState::Negotiated(state2) => Bob::Negotiated { state2 },
            BobState::BtcLocked(state3) => Bob::BtcLocked { state3 },
            BobState::XmrLocked(state4) => Bob::XmrLocked { state4 },
            BobState::EncSigSent(state4) => Bob::EncSigSent { state4 },
            BobState::BtcRedeemed(state5) => Bob::BtcRedeemed(state5),
            BobState::CancelTimelockExpired(state4) => Bob::CancelTimelockExpired(state4),
            BobState::BtcCancelled(state4) => Bob::BtcCancelled(state4),
            BobState::BtcRefunded(state4) => Bob::Done(BobEndState::BtcRefunded(Box::new(state4))),
            BobState::XmrRedeemed => Bob::Done(BobEndState::XmrRedeemed),
            BobState::BtcPunished => Bob::Done(BobEndState::BtcPunished),
            BobState::SafelyAborted => Bob::Done(BobEndState::SafelyAborted),
        }
    }
}

impl From<Bob> for BobState {
    fn from(db_state: Bob) -> Self {
        match db_state {
            Bob::Started { state0, amounts } => BobState::Started { state0, amounts },
            Bob::Negotiated { state2 } => BobState::Negotiated(state2),
            Bob::BtcLocked { state3 } => BobState::BtcLocked(state3),
            Bob::XmrLocked { state4 } => BobState::XmrLocked(state4),
            Bob::EncSigSent { state4 } => BobState::EncSigSent(state4),
            Bob::BtcRedeemed(state5) => BobState::BtcRedeemed(state5),
            Bob::CancelTimelockExpired(state4) => BobState::CancelTimelockExpired(state4),
            Bob::BtcCancelled(state4) => BobState::BtcCancelled(state4),
            Bob::Done(end_state) => match end_state {
                BobEndState::SafelyAborted => BobState::SafelyAborted,
                BobEndState::XmrRedeemed => BobState::XmrRedeemed,
                BobEndState::BtcRefunded(state4) => BobState::BtcRefunded(*state4),
                BobEndState::BtcPunished => BobState::BtcPunished,
            },
        }
    }
}

impl Display for Bob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Bob::Started { .. } => write!(f, "Started"),
            Bob::Negotiated { .. } => f.write_str("Negotiated"),
            Bob::BtcLocked { .. } => f.write_str("Bitcoin locked"),
            Bob::XmrLocked { .. } => f.write_str("Monero locked"),
            Bob::CancelTimelockExpired(_) => f.write_str("Cancel timelock is expired"),
            Bob::BtcCancelled(_) => f.write_str("Bitcoin refundable"),
            Bob::BtcRedeemed(_) => f.write_str("Monero redeemable"),
            Bob::Done(end_state) => write!(f, "Done: {}", end_state),
            Bob::EncSigSent { .. } => f.write_str("Encrypted signature sent"),
        }
    }
}
