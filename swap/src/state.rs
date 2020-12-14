use crate::SwapAmounts;
use libp2p::{core::Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use xmr_btc::{
    alice, bitcoin::EncryptedSignature, bob, cross_curve_dleq, monero, serde::monero_private_key,
};

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Swap {
    Alice(Alice),
    Bob(Bob),
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Alice {
    Started {
        amounts: SwapAmounts,
        // TODO: This should not be saved, instead always derive it from a seed (and that seed file
        // is the only thing that has to be kept secure)
        a: crate::bitcoin::SecretKey,
        s_a: cross_curve_dleq::Scalar,
        v_a: monero::PrivateViewKey,
        refund_timelock: u32,
        punish_timelock: u32,
        redeem_address: ::bitcoin::Address,
        punish_address: ::bitcoin::Address,
    },
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
    Cancelling(alice::State3),
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
    Started {
        state0: bob::State0,
        amounts: SwapAmounts,
        addr: Multiaddr,
    },
    Negotiated {
        state2: bob::State2,
        #[serde(with = "crate::serde::peer_id")]
        peer_id: PeerId,
    },
    BtcLocked {
        state3: bob::State3,
        #[serde(with = "crate::serde::peer_id")]
        peer_id: PeerId,
    },
    XmrLocked {
        state4: bob::State4,
        #[serde(with = "crate::serde::peer_id")]
        peer_id: PeerId,
    },
    EncSigSent {
        state4: bob::State4,
        #[serde(with = "crate::serde::peer_id")]
        peer_id: PeerId,
    },
    BtcRedeemed(bob::State5),
    BtcCancelled(bob::State4),
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
            Alice::Started { .. } => f.write_str("Swap started"),
            Alice::Negotiated(_) => f.write_str("Handshake complete"),
            Alice::BtcLocked(_) => f.write_str("Bitcoin locked"),
            Alice::XmrLocked(_) => f.write_str("Monero locked"),
            Alice::BtcRedeemable { .. } => f.write_str("Bitcoin redeemable"),
            Alice::Cancelling(_) => f.write_str("Submitting TxCancel"),
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
            Bob::Negotiated { .. } => f.write_str("Handshake complete"),
            Bob::BtcLocked { .. } | Bob::XmrLocked { .. } | Bob::BtcCancelled(_) => {
                f.write_str("Bitcoin refundable")
            }
            Bob::BtcRedeemed(_) => f.write_str("Monero redeemable"),
            Bob::SwapComplete => f.write_str("Swap complete"),
            Bob::Started { .. } => f.write_str("Swap started"),
            Bob::EncSigSent { .. } => f.write_str("Encrypted signature sent"),
        }
    }
}
