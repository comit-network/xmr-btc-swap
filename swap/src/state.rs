use serde::{Deserialize, Serialize};
use std::fmt::Display;
use xmr_btc::{alice, bitcoin::EncryptedSignature, bob, monero, serde::monero_private_key};

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Swap {
    Alice(Alice),
    Bob(Bob),
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Alice {
    Negotiated(alice::State3),
    BtcLocked(alice::State3),
    XmrLocked(alice::State3),
    // TODO(Franck): Delete this state as it is not used in alice::swap
    BtcRedeemable {
        state: alice::State3,
        redeem_tx: bitcoin::Transaction,
    },
    EncSignLearned {
        state: alice::State3,
        encrypted_signature: EncryptedSignature,
    },
    BtcCancelled(alice::State3),
    BtcPunishable(alice::State3),
    BtcRefunded {
        state: alice::State3,
        #[serde(with = "monero_private_key")]
        spend_key: monero::PrivateKey,
        view_key: monero::PrivateViewKey,
    },
    SwapComplete,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Bob {
    Handshaken(bob::State2),
    BtcLocked(bob::State2),
    XmrLocked(bob::State2),
    BtcRedeemed(bob::State2),
    BtcRefundable(bob::State2),
    SwapComplete,
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
            Alice::Negotiated(_) => f.write_str("Handshake complete"),
            Alice::BtcLocked(_) => f.write_str("Bitcoin locked"),
            Alice::XmrLocked(_) => f.write_str("Monero locked"),
            Alice::BtcRedeemable { .. } => f.write_str("Bitcoin redeemable"),
            Alice::BtcCancelled(_) => f.write_str("Bitcoin cancel transaction published"),
            Alice::BtcPunishable(_) => f.write_str("Bitcoin punishable"),
            Alice::BtcRefunded { .. } => f.write_str("Monero refundable"),
            Alice::SwapComplete => f.write_str("Swap complete"),
            Alice::EncSignLearned { .. } => f.write_str("Encrypted signature learned"),
        }
    }
}

impl Display for Bob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Bob::Handshaken(_) => f.write_str("Handshake complete"),
            Bob::BtcLocked(_) | Bob::XmrLocked(_) | Bob::BtcRefundable(_) => {
                f.write_str("Bitcoin refundable")
            }
            Bob::BtcRedeemed(_) => f.write_str("Monero redeemable"),
            Bob::SwapComplete => f.write_str("Swap complete"),
        }
    }
}
