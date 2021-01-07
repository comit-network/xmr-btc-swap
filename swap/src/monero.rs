pub mod wallet;

use ::bitcoin::hashes::core::fmt::Formatter;
use anyhow::Result;
use async_trait::async_trait;
use rand::{CryptoRng, RngCore};
use rust_decimal::{
    prelude::{FromPrimitive, ToPrimitive},
    Decimal,
};
use serde::{Deserialize, Serialize};
use std::{
    fmt::Display,
    ops::{Add, Mul, Sub},
    str::FromStr,
};

use crate::{bitcoin, serde::monero_private_key};

pub use ::monero::{Network, PrivateKey, PublicKey};
pub use curve25519_dalek::scalar::Scalar;
pub use wallet::Wallet;

pub const PICONERO_OFFSET: u64 = 1_000_000_000_000;

pub fn random_private_key<R: RngCore + CryptoRng>(rng: &mut R) -> PrivateKey {
    let scalar = Scalar::random(rng);

    PrivateKey::from_scalar(scalar)
}

pub fn private_key_from_secp256k1_scalar(scalar: bitcoin::Scalar) -> PrivateKey {
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
    pub const ZERO: Self = Self(0);
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

    pub fn parse_monero(amount: &str) -> Result<Self> {
        let decimal = Decimal::from_str(amount)?;
        let piconeros_dec =
            decimal.mul(Decimal::from_u64(PICONERO_OFFSET).expect("constant to fit into u64"));
        let piconeros = piconeros_dec
            .to_u64()
            .ok_or_else(|| OverflowError(amount.to_owned()))?;
        Ok(Amount(piconeros))
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
        let mut decimal = Decimal::from(self.0);
        decimal
            .set_scale(12)
            .expect("12 is smaller than max precision of 28");
        write!(f, "{} XMR", decimal)
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

#[derive(thiserror::Error, Debug, Clone, PartialEq)]
#[error("Overflow, cannot convert {0} to u64")]
pub struct OverflowError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_monero_min() {
        let min_pics = 1;
        let amount = Amount::from_piconero(min_pics);
        let monero = amount.to_string();
        assert_eq!("0.000000000001 XMR", monero);
    }

    #[test]
    fn display_monero_one() {
        let min_pics = 1000000000000;
        let amount = Amount::from_piconero(min_pics);
        let monero = amount.to_string();
        assert_eq!("1.000000000000 XMR", monero);
    }

    #[test]
    fn display_monero_max() {
        let max_pics = 18_446_744_073_709_551_615;
        let amount = Amount::from_piconero(max_pics);
        let monero = amount.to_string();
        assert_eq!("18446744.073709551615 XMR", monero);
    }

    #[test]
    fn parse_monero_min() {
        let monero_min = "0.000000000001";
        let amount = Amount::parse_monero(monero_min).unwrap();
        let pics = amount.0;
        assert_eq!(1, pics);
    }

    #[test]
    fn parse_monero() {
        let monero = "123";
        let amount = Amount::parse_monero(monero).unwrap();
        let pics = amount.0;
        assert_eq!(123000000000000, pics);
    }

    #[test]
    fn parse_monero_max() {
        let monero = "18446744.073709551615";
        let amount = Amount::parse_monero(monero).unwrap();
        let pics = amount.0;
        assert_eq!(18446744073709551615, pics);
    }

    #[test]
    fn parse_monero_overflows() {
        let overflow_pics = "18446744.073709551616";
        let error = Amount::parse_monero(overflow_pics).unwrap_err();
        assert_eq!(
            error.downcast_ref::<OverflowError>().unwrap(),
            &OverflowError(overflow_pics.to_owned())
        );
    }
}
