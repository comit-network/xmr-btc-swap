use crate::{
    bitcoin::{EncryptedSignature, TxCancel, TxRefund},
    monero,
    monero::monero_private_key,
    protocol::{alice, alice::AliceState, SwapAmounts},
};
use ::bitcoin::hashes::core::fmt::Display;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};

// Large enum variant is fine because this is only used for database
// and is dropped once written in DB.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Alice {
    Started {
        amounts: SwapAmounts,
        state0: alice::State0,
    },
    Negotiated {
        state3: alice::State3,
        #[serde(with = "crate::serde_peer_id")]
        bob_peer_id: PeerId,
    },
    BtcLocked {
        state3: alice::State3,
        #[serde(with = "crate::serde_peer_id")]
        bob_peer_id: PeerId,
    },
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
            AliceState::Negotiated {
                state3,
                bob_peer_id,
                ..
            } => Alice::Negotiated {
                state3: state3.as_ref().clone(),
                bob_peer_id: bob_peer_id.clone(),
            },
            AliceState::BtcLocked {
                state3,
                bob_peer_id,
                ..
            } => Alice::BtcLocked {
                state3: state3.as_ref().clone(),
                bob_peer_id: bob_peer_id.clone(),
            },
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
            Alice::Negotiated {
                state3,
                bob_peer_id,
            } => AliceState::Negotiated {
                bob_peer_id,
                amounts: SwapAmounts {
                    btc: state3.btc,
                    xmr: state3.xmr,
                },
                state3: Box::new(state3),
            },
            Alice::BtcLocked {
                state3,
                bob_peer_id,
            } => AliceState::BtcLocked {
                bob_peer_id,
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

impl Display for Alice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Alice::Started { .. } => write!(f, "Started"),
            Alice::Negotiated { .. } => f.write_str("Negotiated"),
            Alice::BtcLocked { .. } => f.write_str("Bitcoin locked"),
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
