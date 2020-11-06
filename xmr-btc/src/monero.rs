use crate::serde::monero_private_key;
use anyhow::Result;
use async_trait::async_trait;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use std::ops::{Add, Sub};

pub use curve25519_dalek::scalar::Scalar;
pub use monero::*;

pub const MIN_CONFIRMATIONS: u32 = 10;

pub fn random_private_key<R: RngCore + CryptoRng>(rng: &mut R) -> PrivateKey {
    let scalar = Scalar::random(rng);

    PrivateKey::from_scalar(scalar)
}

pub fn private_key_from_secp256k1_scalar(scalar: crate::bitcoin::Scalar) -> PrivateKey {
    let mut bytes = scalar.to_bytes();

    // we must reverse the bytes because a secp256k1 scalar is big endian, whereas a
    // ed25519 scalar is little endian
    bytes.reverse();

    PrivateKey::from_scalar(Scalar::from_bytes_mod_order(bytes))
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct PrivateViewKey(#[serde(with = "monero_private_key")] PrivateKey);

impl PrivateViewKey {
    pub fn new_random<R: RngCore + CryptoRng>(rng: &mut R) -> Self {
        let scalar = Scalar::random(rng);
        let private_key = PrivateKey::from_scalar(scalar);

        Self(private_key)
    }

    pub fn public(&self) -> PublicViewKey {
        PublicViewKey(PublicKey::from_private_key(&self.0))
    }
}

impl Add for PrivateViewKey {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl From<PrivateViewKey> for PrivateKey {
    fn from(from: PrivateViewKey) -> Self {
        from.0
    }
}

impl From<PublicViewKey> for PublicKey {
    fn from(from: PublicViewKey) -> Self {
        from.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PublicViewKey(PublicKey);

#[derive(Debug, Copy, Clone, Deserialize, Serialize, PartialEq, PartialOrd)]
pub struct Amount(u64);

impl Amount {
    /// Create an [Amount] with piconero precision and the given number of
    /// piconeros.
    ///
    /// A piconero (a.k.a atomic unit) is equal to 1e-12 XMR.
    pub fn from_piconero(amount: u64) -> Self {
        Amount(amount)
    }
    pub fn as_piconero(&self) -> u64 {
        self.0
    }
}

impl Add for Amount {
    type Output = Amount;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Amount {
    type Output = Amount;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl From<Amount> for u64 {
    fn from(from: Amount) -> u64 {
        from.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferProof {
    tx_hash: TxHash,
    #[serde(with = "monero_private_key")]
    tx_key: PrivateKey,
}

impl TransferProof {
    pub fn new(tx_hash: TxHash, tx_key: PrivateKey) -> Self {
        Self { tx_hash, tx_key }
    }
    pub fn tx_hash(&self) -> TxHash {
        self.tx_hash.clone()
    }
    pub fn tx_key(&self) -> PrivateKey {
        self.tx_key
    }
}

// TODO: add constructor/ change String to fixed length byte array
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxHash(pub String);

impl From<TxHash> for String {
    fn from(from: TxHash) -> Self {
        from.0
    }
}

#[async_trait]
pub trait Transfer {
    async fn transfer(
        &self,
        public_spend_key: PublicKey,
        public_view_key: PublicViewKey,
        amount: Amount,
    ) -> anyhow::Result<(TransferProof, Amount)>;
}

#[async_trait]
pub trait WatchForTransfer {
    async fn watch_for_transfer(
        &self,
        public_spend_key: PublicKey,
        public_view_key: PublicViewKey,
        transfer_proof: TransferProof,
        amount: Amount,
        expected_confirmations: u32,
    ) -> Result<(), InsufficientFunds>;
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("transaction does not pay enough: expected {expected:?}, got {actual:?}")]
pub struct InsufficientFunds {
    pub expected: Amount,
    pub actual: Amount,
}

#[async_trait]
pub trait CreateWalletForOutput {
    async fn create_and_load_wallet_for_output(
        &self,
        private_spend_key: PrivateKey,
        private_view_key: PrivateViewKey,
    ) -> anyhow::Result<()>;
}
