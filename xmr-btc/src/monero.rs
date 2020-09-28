#[cfg(test)]
pub mod wallet;

use std::ops::Add;

use anyhow::Result;
use async_trait::async_trait;
use rand::{CryptoRng, RngCore};

pub use curve25519_dalek::scalar::Scalar;
pub use monero::{Address, PrivateKey, PublicKey};

pub fn random_private_key<R: RngCore + CryptoRng>(rng: &mut R) -> PrivateKey {
    let scalar = Scalar::random(rng);

    PrivateKey::from_scalar(scalar)
}

#[cfg(test)]
pub use wallet::{AliceWallet, BobWallet};

#[derive(Clone, Copy, Debug)]
pub struct PrivateViewKey(PrivateKey);

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

#[derive(Debug, Copy, Clone)]
pub struct Amount(u64);

impl Amount {
    /// Create an [Amount] with piconero precision and the given number of
    /// piconeros.
    ///
    /// A piconero (a.k.a atomic unit) is equal to 1e-12 XMR.
    pub fn from_piconero(amount: u64) -> Self {
        Amount(amount)
    }
}

impl From<Amount> for u64 {
    fn from(from: Amount) -> u64 {
        from.0
    }
}

#[derive(Clone, Debug)]
pub struct TransferProof {
    tx_hash: TxHash,
    tx_key: PrivateKey,
}

#[derive(Clone, Debug)]
pub struct TxHash(String);

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
    ) -> Result<(TransferProof, Amount)>;
}

#[async_trait]
pub trait CheckTransfer {
    async fn check_transfer(
        &self,
        public_spend_key: PublicKey,
        public_view_key: PublicViewKey,
        transfer_proof: TransferProof,
        amount: Amount,
    ) -> Result<()>;
}

#[async_trait]
pub trait ImportOutput {
    async fn import_output(
        &self,
        private_spend_key: PrivateKey,
        private_view_key: PrivateViewKey,
    ) -> Result<()>;
}
