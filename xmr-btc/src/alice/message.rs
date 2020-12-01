use anyhow::Result;
use ecdsa_fun::{adaptor::EncryptedSignature, Signature};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

use crate::{bitcoin, monero};

#[derive(Debug)]
pub enum Message {
    Message0(Message0),
    Message1(Message1),
    Message2(Message2),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message0 {
    pub(crate) A: bitcoin::PublicKey,
    pub(crate) S_a_monero: monero::PublicKey,
    pub(crate) S_a_bitcoin: bitcoin::PublicKey,
    pub(crate) dleq_proof_s_a: cross_curve_dleq::Proof,
    pub(crate) v_a: monero::PrivateViewKey,
    pub(crate) redeem_address: bitcoin::Address,
    pub(crate) punish_address: bitcoin::Address,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message1 {
    pub(crate) tx_cancel_sig: Signature,
    pub(crate) tx_refund_encsig: EncryptedSignature,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message2 {
    pub tx_lock_proof: monero::TransferProof,
}

impl_try_from_parent_enum!(Message0, Message);
impl_try_from_parent_enum!(Message1, Message);
impl_try_from_parent_enum!(Message2, Message);

impl_from_child_enum!(Message0, Message);
impl_from_child_enum!(Message1, Message);
impl_from_child_enum!(Message2, Message);
