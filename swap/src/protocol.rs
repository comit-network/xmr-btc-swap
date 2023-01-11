use crate::protocol::alice::swap::is_complete as alice_is_complete;
use crate::protocol::alice::AliceState;
use crate::protocol::bob::swap::is_complete as bob_is_complete;
use crate::protocol::bob::BobState;
use crate::{bitcoin, monero};
use anyhow::Result;
use async_trait::async_trait;
use conquer_once::Lazy;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sigma_fun::ext::dl_secp256k1_ed25519_eq::{CrossCurveDLEQ, CrossCurveDLEQProof};
use sigma_fun::HashTranscript;
use std::convert::TryInto;
use uuid::Uuid;

pub mod alice;
pub mod bob;

pub static CROSS_CURVE_PROOF_SYSTEM: Lazy<
    CrossCurveDLEQ<HashTranscript<Sha256, rand_chacha::ChaCha20Rng>>,
> = Lazy::new(|| {
    CrossCurveDLEQ::<HashTranscript<Sha256, rand_chacha::ChaCha20Rng>>::new(
        (*ecdsa_fun::fun::G).normalize(),
        curve25519_dalek::constants::ED25519_BASEPOINT_POINT,
    )
});

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message0 {
    swap_id: Uuid,
    B: bitcoin::PublicKey,
    S_b_monero: monero::PublicKey,
    S_b_bitcoin: bitcoin::PublicKey,
    dleq_proof_s_b: CrossCurveDLEQProof,
    v_b: monero::PrivateViewKey,
    refund_address: bitcoin::Address,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    tx_refund_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    tx_cancel_fee: bitcoin::Amount,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message1 {
    A: bitcoin::PublicKey,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    dleq_proof_s_a: CrossCurveDLEQProof,
    v_a: monero::PrivateViewKey,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    tx_redeem_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    tx_punish_fee: bitcoin::Amount,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message2 {
    psbt: bitcoin::PartiallySignedTransaction,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message3 {
    tx_cancel_sig: bitcoin::Signature,
    tx_refund_encsig: bitcoin::EncryptedSignature,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message4 {
    tx_punish_sig: bitcoin::Signature,
    tx_cancel_sig: bitcoin::Signature,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Alice(AliceState),
    Bob(BobState),
}

impl State {
    pub fn swap_finished(&self) -> bool {
        match self {
            State::Alice(state) => alice_is_complete(state),
            State::Bob(state) => bob_is_complete(state),
        }
    }
}

impl From<AliceState> for State {
    fn from(alice: AliceState) -> Self {
        Self::Alice(alice)
    }
}

impl From<BobState> for State {
    fn from(bob: BobState) -> Self {
        Self::Bob(bob)
    }
}

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
#[error("Not in the role of Alice")]
pub struct NotAlice;

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
#[error("Not in the role of Bob")]
pub struct NotBob;

impl TryInto<BobState> for State {
    type Error = NotBob;

    fn try_into(self) -> std::result::Result<BobState, Self::Error> {
        match self {
            State::Alice(_) => Err(NotBob),
            State::Bob(state) => Ok(state),
        }
    }
}

impl TryInto<AliceState> for State {
    type Error = NotAlice;

    fn try_into(self) -> std::result::Result<AliceState, Self::Error> {
        match self {
            State::Alice(state) => Ok(state),
            State::Bob(_) => Err(NotAlice),
        }
    }
}

#[async_trait]
pub trait Database {
    async fn insert_peer_id(&self, swap_id: Uuid, peer_id: PeerId) -> Result<()>;
    async fn get_peer_id(&self, swap_id: Uuid) -> Result<PeerId>;
    async fn insert_monero_address(&self, swap_id: Uuid, address: monero::Address) -> Result<()>;
    async fn get_monero_address(&self, swap_id: Uuid) -> Result<monero::Address>;
    async fn insert_address(&self, peer_id: PeerId, address: Multiaddr) -> Result<()>;
    async fn get_addresses(&self, peer_id: PeerId) -> Result<Vec<Multiaddr>>;
    async fn insert_latest_state(&self, swap_id: Uuid, state: State) -> Result<()>;
    async fn get_state(&self, swap_id: Uuid) -> Result<State>;
    async fn all(&self) -> Result<Vec<(Uuid, State)>>;
}
