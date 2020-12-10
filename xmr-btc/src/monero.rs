use crate::serde::monero_private_key;
use anyhow::Result;
use async_trait::async_trait;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use std::ops::{Add, Mul, Sub};

use bitcoin::hashes::core::fmt::Formatter;
pub use curve25519_dalek::scalar::Scalar;
pub use monero::*;
use rust_decimal::{prelude::FromPrimitive, Decimal};
use std::fmt::Display;

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

    /// Maximum piconero value that can be converted is currently i64 maximum
    /// value. This should only be used for Display purposes and not for
    /// calculations!
    pub fn as_monero(&self) -> Result<String> {
        let piconero_as_i64 = i64::from_u64(self.0).ok_or_else(|| OverflowError(self.0))?;
        let dec = Decimal::new(piconero_as_i64, 12);
        Ok(dec.to_string())
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

impl Mul<u64> for Amount {
    type Output = Amount;

    fn mul(self, rhs: u64) -> Self::Output {
        Self(self.0 * rhs)
    }
}

impl From<Amount> for u64 {
    fn from(from: Amount) -> u64 {
        from.0
    }
}

impl Display for Amount {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let monero = self.as_monero();

        match monero {
            Ok(monero) => write!(f, "{} XMR", monero),
            Err(_) => write!(f, "{} piconero", self.as_piconero()),
        }
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

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq)]
#[error("Overflow, cannot convert {0} to i64")]
pub struct OverflowError(pub u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_piconero_as_monero() {
        let one_piconero = 1;
        let amount = Amount::from_piconero(one_piconero);
        let monero = amount.as_monero().unwrap();
        assert_eq!("0.000000000001", monero);
    }

    #[test]
    fn one_monero_as_monero() {
        let one_monero_in_pics = 1000000000000;
        let amount = Amount::from_piconero(one_monero_in_pics);
        let monero = amount.as_monero().unwrap();
        assert_eq!("1.000000000000", monero);
    }

    #[test]
    fn monero_with_i64_max_returns_monero() {
        let max_pics = 9_223_372_036_854_775_807;
        let amount = Amount::from_piconero(max_pics);
        let monero = amount.as_monero().unwrap();
        assert_eq!("9223372.036854775807", monero);
    }

    #[test]
    fn monero_with_i64_overflow_returns_error() {
        let max_pics = 9_223_372_036_854_775_808;
        let amount = Amount::from_piconero(max_pics);

        let error = amount.as_monero().unwrap_err();
        assert_eq!(
            error.downcast_ref::<OverflowError>().unwrap(),
            &OverflowError(9_223_372_036_854_775_808)
        );
    }

    #[test]
    fn display_one_monero_as_xmr() {
        let one_monero_in_pics = 1000000000000;
        let amount = Amount::from_piconero(one_monero_in_pics);
        let monero = amount.to_string();
        assert_eq!("1.000000000000 XMR", monero);
    }

    #[test]
    fn display_i64_max_as_xmr() {
        let max_pics = 9_223_372_036_854_775_807;
        let amount = Amount::from_piconero(max_pics);
        let monero = amount.to_string();
        assert_eq!("9223372.036854775807 XMR", monero);
    }

    #[test]
    fn display_i64_overflow_as_piconero() {
        let max_pics = 9_223_372_036_854_775_808;
        let amount = Amount::from_piconero(max_pics);
        let monero = amount.to_string();
        assert_eq!("9223372036854775808 piconero", monero);
    }

    #[test]
    fn display_u64_max_as_piconero() {
        let max_pics = 18_446_744_073_709_551_615;
        let amount = Amount::from_piconero(max_pics);
        let monero = amount.to_string();
        assert_eq!("18446744073709551615 piconero", monero);
    }
}
