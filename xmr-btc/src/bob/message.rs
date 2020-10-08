use crate::{bitcoin, monero};
use anyhow::Result;
use ecdsa_fun::{adaptor::EncryptedSignature, Signature};
use std::convert::TryFrom;

#[derive(Debug)]
pub enum Message {
    Message0(Message0),
    Message1(Message1),
    Message2(Message2),
    Message3(Message3),
}

#[derive(Debug)]
pub struct Message0 {
    pub(crate) B: bitcoin::PublicKey,
    pub(crate) S_b_monero: monero::PublicKey,
    pub(crate) S_b_bitcoin: bitcoin::PublicKey,
    pub(crate) dleq_proof_s_b: cross_curve_dleq::Proof,
    pub(crate) v_b: monero::PrivateViewKey,
    pub(crate) refund_address: bitcoin::Address,
}

#[derive(Debug)]
pub struct Message1 {
    pub(crate) tx_lock: bitcoin::TxLock,
}

#[derive(Debug)]
pub struct Message2 {
    pub(crate) tx_punish_sig: Signature,
    pub(crate) tx_cancel_sig: Signature,
}

#[derive(Debug)]
pub struct Message3 {
    pub(crate) tx_redeem_encsig: EncryptedSignature,
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
                expected_type: "Create0".to_string(),
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
                expected_type: "Create0".to_string(),
                received: m,
            }),
        }
    }
}

impl From<Message3> for Message {
    fn from(m: Message3) -> Self {
        Message::Message3(m)
    }
}

impl TryFrom<Message> for Message3 {
    type Error = UnexpectedMessage;

    fn try_from(m: Message) -> Result<Self, Self::Error> {
        match m {
            Message::Message3(m) => Ok(m),
            _ => Err(UnexpectedMessage {
                expected_type: "Create0".to_string(),
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
