use crate::{bitcoin, monero};
use anyhow::Result;
use ecdsa_fun::{adaptor::EncryptedSignature, Signature};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

#[derive(Clone, Debug)]
pub enum Message {
    Message0(Message0),
    Message1(Message1),
    Message2(Message2),
    Message3(Message3),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message0 {
    pub(crate) B: bitcoin::PublicKey,
    pub(crate) S_b_monero: monero::PublicKey,
    pub(crate) S_b_bitcoin: bitcoin::PublicKey,
    pub(crate) dleq_proof_s_b: cross_curve_dleq::Proof,
    pub(crate) v_b: monero::PrivateViewKey,
    pub(crate) refund_address: bitcoin::Address,
}

#[derive(Clone, Debug)]
pub struct Message1 {
    pub(crate) tx_lock: bitcoin::TxLock,
}

#[derive(Clone, Debug)]
pub struct Message2 {
    pub(crate) tx_punish_sig: Signature,
    pub(crate) tx_cancel_sig: Signature,
}

#[derive(Clone, Debug)]
pub struct Message3 {
    pub(crate) tx_redeem_encsig: EncryptedSignature,
}

impl_try_from_parent_enum!(Message0, Message);
impl_try_from_parent_enum!(Message1, Message);
impl_try_from_parent_enum!(Message2, Message);
impl_try_from_parent_enum!(Message3, Message);

impl_from_child_enum!(Message0, Message);
impl_from_child_enum!(Message1, Message);
impl_from_child_enum!(Message2, Message);
impl_from_child_enum!(Message3, Message);
