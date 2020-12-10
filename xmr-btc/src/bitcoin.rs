pub mod transactions;

use crate::config::Config;
use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use bitcoin::hashes::{hex::ToHex, Hash};
use ecdsa_fun::{adaptor::Adaptor, fun::Point, nonce::Deterministic, ECDSA};
use miniscript::{Descriptor, Segwitv0};
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::str::FromStr;

pub use bitcoin::{util::psbt::PartiallySignedTransaction, *};
pub use ecdsa_fun::{adaptor::EncryptedSignature, fun::Scalar, Signature};
pub use transactions::{TxCancel, TxLock, TxPunish, TxRedeem, TxRefund};

// TODO: Configurable tx-fee (note: parties have to agree prior to swapping)
// Current reasoning:
// tx with largest weight (as determined by get_weight() upon broadcast in e2e
// test) = 609 assuming segwit and 60 sat/vB:
// (609 / 4) * 60 (sat/vB) = 9135 sats
// Recommended: Overpay a bit to ensure we don't have to wait too long for test
// runs.
pub const TX_FEE: u64 = 15_000;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SecretKey {
    inner: Scalar,
    public: Point,
}

impl SecretKey {
    pub fn new_random<R: RngCore + CryptoRng>(rng: &mut R) -> Self {
        let scalar = Scalar::random(rng);

        let ecdsa = ECDSA::<()>::default();
        let public = ecdsa.verification_key_for(&scalar);

        Self {
            inner: scalar,
            public,
        }
    }

    pub fn public(&self) -> PublicKey {
        PublicKey(self.public)
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.inner.to_bytes()
    }

    pub fn sign(&self, digest: SigHash) -> Signature {
        let ecdsa = ECDSA::<Deterministic<Sha256>>::default();

        ecdsa.sign(&self.inner, &digest.into_inner())
    }

    // TxRefund encsigning explanation:
    //
    // A and B, are the Bitcoin Public Keys which go on the joint output for
    // TxLock_Bitcoin. S_a and S_b, are the Monero Public Keys which go on the
    // joint output for TxLock_Monero

    // tx_refund: multisig(A, B), published by bob
    // bob can produce sig on B for tx_refund using b
    // alice sends over an encrypted signature on A for tx_refund using a encrypted
    // with S_b we want to leak s_b

    // produced (by Alice) encsig - published (by Bob) sig = s_b (it's not really
    // subtraction, it's recover)

    // self = a, Y = S_b, digest = tx_refund
    pub fn encsign(&self, Y: PublicKey, digest: SigHash) -> EncryptedSignature {
        let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

        adaptor.encrypted_sign(&self.inner, &Y.0, &digest.into_inner())
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublicKey(Point);

impl From<PublicKey> for Point {
    fn from(from: PublicKey) -> Self {
        from.0
    }
}

impl From<Scalar> for SecretKey {
    fn from(scalar: Scalar) -> Self {
        let ecdsa = ECDSA::<()>::default();
        let public = ecdsa.verification_key_for(&scalar);

        Self {
            inner: scalar,
            public,
        }
    }
}

impl From<SecretKey> for Scalar {
    fn from(sk: SecretKey) -> Self {
        sk.inner
    }
}

impl From<Scalar> for PublicKey {
    fn from(scalar: Scalar) -> Self {
        let ecdsa = ECDSA::<()>::default();
        PublicKey(ecdsa.verification_key_for(&scalar))
    }
}

pub fn verify_sig(
    verification_key: &PublicKey,
    transaction_sighash: &SigHash,
    sig: &Signature,
) -> Result<()> {
    let ecdsa = ECDSA::verify_only();

    if ecdsa.verify(&verification_key.0, &transaction_sighash.into_inner(), &sig) {
        Ok(())
    } else {
        bail!(InvalidSignature)
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("signature is invalid")]
pub struct InvalidSignature;

pub fn verify_encsig(
    verification_key: PublicKey,
    encryption_key: PublicKey,
    digest: &SigHash,
    encsig: &EncryptedSignature,
) -> Result<()> {
    let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

    if adaptor.verify_encrypted_signature(
        &verification_key.0,
        &encryption_key.0,
        &digest.into_inner(),
        &encsig,
    ) {
        Ok(())
    } else {
        bail!(InvalidEncryptedSignature)
    }
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("encrypted signature is invalid")]
pub struct InvalidEncryptedSignature;

pub fn build_shared_output_descriptor(A: Point, B: Point) -> Descriptor<bitcoin::PublicKey> {
    const MINISCRIPT_TEMPLATE: &str = "c:and_v(v:pk(A),pk_k(B))";

    // NOTE: This shouldn't be a source of error, but maybe it is
    let A = ToHex::to_hex(&secp256k1::PublicKey::from(A));
    let B = ToHex::to_hex(&secp256k1::PublicKey::from(B));

    let miniscript = MINISCRIPT_TEMPLATE.replace("A", &A).replace("B", &B);

    let miniscript = miniscript::Miniscript::<bitcoin::PublicKey, Segwitv0>::from_str(&miniscript)
        .expect("a valid miniscript");

    Descriptor::Wsh(miniscript)
}

#[async_trait]
pub trait BuildTxLockPsbt {
    async fn build_tx_lock_psbt(
        &self,
        output_address: Address,
        output_amount: Amount,
    ) -> Result<PartiallySignedTransaction>;
}

#[async_trait]
pub trait SignTxLock {
    async fn sign_tx_lock(&self, tx_lock: TxLock) -> Result<Transaction>;
}

#[async_trait]
pub trait BroadcastSignedTransaction {
    async fn broadcast_signed_transaction(&self, transaction: Transaction) -> Result<Txid>;
}

#[async_trait]
pub trait WatchForRawTransaction {
    async fn watch_for_raw_transaction(&self, txid: Txid) -> Transaction;
}

#[async_trait]
pub trait WaitForTransactionFinality {
    async fn wait_for_transaction_finality(&self, txid: Txid, config: Config) -> Result<()>;
}

#[async_trait]
pub trait BlockHeight {
    async fn block_height(&self) -> u32;
}

#[async_trait]
pub trait TransactionBlockHeight {
    async fn transaction_block_height(&self, txid: Txid) -> u32;
}

#[async_trait]
pub trait WaitForBlockHeight {
    async fn wait_for_block_height(&self, height: u32);
}

#[async_trait]
pub trait GetRawTransaction {
    async fn get_raw_transaction(&self, txid: Txid) -> Result<Transaction>;
}

#[async_trait]
pub trait Network {
    fn get_network(&self) -> bitcoin::Network;
}

pub fn recover(S: PublicKey, sig: Signature, encsig: EncryptedSignature) -> Result<SecretKey> {
    let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

    let s = adaptor
        .recover_decryption_key(&S.0, &sig, &encsig)
        .map(SecretKey::from)
        .ok_or_else(|| anyhow!("secret recovery failure"))?;

    Ok(s)
}

pub async fn poll_until_block_height_is_gte<B>(client: &B, target: u32)
where
    B: BlockHeight,
{
    while client.block_height().await < target {
        tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
    }
}
