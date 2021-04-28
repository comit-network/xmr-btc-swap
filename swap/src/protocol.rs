use crate::{bitcoin, monero};
use conquer_once::Lazy;
use ecdsa_fun::fun::marker::Mark;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sigma_fun::ext::dl_secp256k1_ed25519_eq::{CrossCurveDLEQ, CrossCurveDLEQProof};
use sigma_fun::HashTranscript;
use uuid::Uuid;

pub mod alice;
pub mod bob;

pub static CROSS_CURVE_PROOF_SYSTEM: Lazy<
    CrossCurveDLEQ<HashTranscript<Sha256, rand_chacha::ChaCha20Rng>>,
> = Lazy::new(|| {
    CrossCurveDLEQ::<HashTranscript<Sha256, rand_chacha::ChaCha20Rng>>::new(
        (*ecdsa_fun::fun::G).mark::<ecdsa_fun::fun::marker::Normal>(),
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
