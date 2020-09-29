use anyhow::Result;
use ecdsa_fun::adaptor::EncryptedSignature;

use crate::{bitcoin, monero};
use ecdsa_fun::Signature;

use std::convert::TryFrom;

#[derive(Debug)]
pub enum Message {
    Message0(Message0),
    Message1(Message1),
    Message2(Message2),
}

#[derive(Debug)]
pub struct Message0 {
    pub(crate) A: bitcoin::PublicKey,
    pub(crate) S_a_monero: monero::PublicKey,
    pub(crate) S_a_bitcoin: bitcoin::PublicKey,
    pub(crate) dleq_proof_s_a: cross_curve_dleq::Proof,
    pub(crate) v_a: monero::PrivateViewKey,
    pub(crate) redeem_address: bitcoin::Address,
    pub(crate) punish_address: bitcoin::Address,
}

#[derive(Debug)]
pub struct Message1 {
    pub(crate) tx_cancel_sig: Signature,
    pub(crate) tx_refund_encsig: EncryptedSignature,
}

#[derive(Debug)]
pub struct Message2 {
    pub(crate) tx_lock_proof: monero::TransferProof,
}

impl From<Message0> for Message {
    fn from(m: Message0) -> Self {
        Message::Message0(m)
    }
}

impl TryFrom<Message> for Message0 {
    type Error = UnexpectedMessage;

    fn try_from(m: Message) -> Result<Self, Self::Error> {
        match m {
            Message::Message0(m) => Ok(m),
            _ => Err(UnexpectedMessage {
                expected_type: "Create0".to_string(),
                received: m,
            }),
        }
    }
}

impl From<Message1> for Message {
    fn from(m: Message1) -> Self {
        Message::Message1(m)
    }
}

impl TryFrom<Message> for Message1 {
    type Error = UnexpectedMessage;

    fn try_from(m: Message) -> Result<Self, Self::Error> {
        match m {
            Message::Message1(m) => Ok(m),
            _ => Err(UnexpectedMessage {
                expected_type: "Create1".to_string(),
                received: m,
            }),
        }
    }
}

impl From<Message2> for Message {
    fn from(m: Message2) -> Self {
        Message::Message2(m)
    }
}

impl TryFrom<Message> for Message2 {
    type Error = UnexpectedMessage;

    fn try_from(m: Message) -> Result<Self, Self::Error> {
        match m {
            Message::Message2(m) => Ok(m),
            _ => Err(UnexpectedMessage {
                expected_type: "Create2".to_string(),
                received: m,
            }),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("expected message of type {expected_type}, got {received:?}")]
pub struct UnexpectedMessage {
    expected_type: String,
    received: Message,
}

impl UnexpectedMessage {
    pub fn new<T>(received: Message) -> Self {
        let expected_type = std::any::type_name::<T>();

        Self {
            expected_type: expected_type.to_string(),
            received,
        }
    }
}
