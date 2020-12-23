use crate::{alice::swap::AliceState, bob::swap::BobState, SwapAmounts};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use xmr_btc::{
    alice,
    bitcoin::{EncryptedSignature, TxCancel, TxRefund},
    bob, monero,
    serde::monero_private_key,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Swap {
    Alice(Alice),
    Bob(Bob),
}

// Large enum variant is fine because this is only used for storage
// and is dropped once written in DB.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Alice {
    Started {
        amounts: SwapAmounts,
        state0: alice::State0,
    },
    Negotiated(alice::State3),
    BtcLocked(alice::State3),
    XmrLocked(alice::State3),
    EncSigLearned {
        encrypted_signature: EncryptedSignature,
        state3: alice::State3,
    },
    CancelTimelockExpired(alice::State3),
    BtcCancelled(alice::State3),
    BtcPunishable(alice::State3),
    BtcRefunded {
        state3: alice::State3,
        #[serde(with = "monero_private_key")]
        spend_key: monero::PrivateKey,
    },
    Done(AliceEndState),
}

#[derive(Copy, Clone, strum::Display, Debug, Deserialize, Serialize, PartialEq)]
pub enum AliceEndState {
    SafelyAborted,
    BtcRedeemed,
    XmrRefunded,
    BtcPunished,
}

impl From<&AliceState> for Alice {
    fn from(alice_state: &AliceState) -> Self {
        match alice_state {
            AliceState::Negotiated { state3, .. } => Alice::Negotiated(state3.as_ref().clone()),
            AliceState::BtcLocked { state3, .. } => Alice::BtcLocked(state3.as_ref().clone()),
            AliceState::XmrLocked { state3 } => Alice::XmrLocked(state3.as_ref().clone()),
            AliceState::EncSigLearned {
                state3,
                encrypted_signature,
            } => Alice::EncSigLearned {
                state3: state3.as_ref().clone(),
                encrypted_signature: encrypted_signature.clone(),
            },
            AliceState::BtcRedeemed => Alice::Done(AliceEndState::BtcRedeemed),
            AliceState::BtcCancelled { state3, .. } => Alice::BtcCancelled(state3.as_ref().clone()),
            AliceState::BtcRefunded { spend_key, state3 } => Alice::BtcRefunded {
                spend_key: *spend_key,
                state3: state3.as_ref().clone(),
            },
            AliceState::BtcPunishable { state3, .. } => {
                Alice::BtcPunishable(state3.as_ref().clone())
            }
            AliceState::XmrRefunded => Alice::Done(AliceEndState::XmrRefunded),
            AliceState::CancelTimelockExpired { state3 } => {
                Alice::CancelTimelockExpired(state3.as_ref().clone())
            }
            AliceState::BtcPunished => Alice::Done(AliceEndState::BtcPunished),
            AliceState::SafelyAborted => Alice::Done(AliceEndState::SafelyAborted),
            AliceState::Started { amounts, state0 } => Alice::Started {
                amounts: *amounts,
                state0: state0.clone(),
            },
        }
    }
}

impl From<Alice> for AliceState {
    fn from(db_state: Alice) -> Self {
        match db_state {
            Alice::Started { amounts, state0 } => AliceState::Started { amounts, state0 },
            Alice::Negotiated(state3) => AliceState::Negotiated {
                channel: None,
                amounts: SwapAmounts {
                    btc: state3.btc,
                    xmr: state3.xmr,
                },
                state3: Box::new(state3),
            },
            Alice::BtcLocked(state3) => AliceState::BtcLocked {
                channel: None,
                amounts: SwapAmounts {
                    btc: state3.btc,
                    xmr: state3.xmr,
                },
                state3: Box::new(state3),
            },
            Alice::XmrLocked(state3) => AliceState::XmrLocked {
                state3: Box::new(state3),
            },
            Alice::EncSigLearned {
                state3: state,
                encrypted_signature,
            } => AliceState::EncSigLearned {
                state3: Box::new(state),
                encrypted_signature,
            },
            Alice::CancelTimelockExpired(state3) => AliceState::CancelTimelockExpired {
                state3: Box::new(state3),
            },
            Alice::BtcCancelled(state) => {
                let tx_cancel = TxCancel::new(
                    &state.tx_lock,
                    state.cancel_timelock,
                    state.a.public(),
                    state.B,
                );

                AliceState::BtcCancelled {
                    state3: Box::new(state),
                    tx_cancel,
                }
            }
            Alice::BtcPunishable(state3) => {
                let tx_cancel = TxCancel::new(
                    &state3.tx_lock,
                    state3.cancel_timelock,
                    state3.a.public(),
                    state3.B,
                );
                let tx_refund = TxRefund::new(&tx_cancel, &state3.refund_address);
                AliceState::BtcPunishable {
                    tx_refund,
                    state3: Box::new(state3),
                }
            }
            Alice::BtcRefunded {
                state3, spend_key, ..
            } => AliceState::BtcRefunded {
                spend_key,
                state3: Box::new(state3),
            },
            Alice::Done(end_state) => match end_state {
                AliceEndState::SafelyAborted => AliceState::SafelyAborted,
                AliceEndState::BtcRedeemed => AliceState::BtcRedeemed,
                AliceEndState::XmrRefunded => AliceState::XmrRefunded,
                AliceEndState::BtcPunished => AliceState::BtcPunished,
            },
        }
    }
}

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

impl From<Alice> for Swap {
    fn from(from: Alice) -> Self {
        Swap::Alice(from)
    }
}

impl From<Bob> for Swap {
    fn from(from: Bob) -> Self {
        Swap::Bob(from)
    }
}

impl Display for Swap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Swap::Alice(alice) => Display::fmt(alice, f),
            Swap::Bob(bob) => Display::fmt(bob, f),
        }
    }
}

impl Display for Alice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Alice::Started { .. } => write!(f, "Started"),
            Alice::Negotiated(_) => f.write_str("Negotiated"),
            Alice::BtcLocked(_) => f.write_str("Bitcoin locked"),
            Alice::XmrLocked(_) => f.write_str("Monero locked"),
            Alice::CancelTimelockExpired(_) => f.write_str("Cancel timelock is expired"),
            Alice::BtcCancelled(_) => f.write_str("Bitcoin cancel transaction published"),
            Alice::BtcPunishable(_) => f.write_str("Bitcoin punishable"),
            Alice::BtcRefunded { .. } => f.write_str("Monero refundable"),
            Alice::Done(end_state) => write!(f, "Done: {}", end_state),
            Alice::EncSigLearned { .. } => f.write_str("Encrypted signature learned"),
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
