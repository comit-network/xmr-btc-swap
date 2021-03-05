pub mod wallet;

mod cancel;
mod lock;
mod punish;
mod redeem;
mod refund;
mod timelocks;

pub use crate::bitcoin::cancel::{CancelTimelock, PunishTimelock, TxCancel};
pub use crate::bitcoin::lock::TxLock;
pub use crate::bitcoin::punish::TxPunish;
pub use crate::bitcoin::redeem::TxRedeem;
pub use crate::bitcoin::refund::TxRefund;
pub use crate::bitcoin::timelocks::{BlockHeight, ExpiredTimelocks};
pub use ::bitcoin::util::amount::Amount;
pub use ::bitcoin::{Address, Network, Transaction, Txid};
pub use ecdsa_fun::adaptor::EncryptedSignature;
pub use ecdsa_fun::fun::Scalar;
pub use ecdsa_fun::Signature;
pub use wallet::Wallet;

use ::bitcoin::hashes::hex::ToHex;
use ::bitcoin::hashes::Hash;
use ::bitcoin::{secp256k1, SigHash};
use anyhow::{bail, Context, Result};
use ecdsa_fun::adaptor::{Adaptor, HashTranscript};
use ecdsa_fun::fun::Point;
use ecdsa_fun::nonce::Deterministic;
use ecdsa_fun::ECDSA;
use miniscript::descriptor::Wsh;
use miniscript::{Descriptor, Segwitv0};
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::str::FromStr;

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
    // bob can produce sig on B using b
    // alice sends over an encrypted signature on A encrypted with S_b
    // s_b is leaked to alice when bob publishes signed tx_refund allowing her to
    // recover s_b: recover(encsig, S_b, sig_tx_refund) = s_b
    // alice now has s_a and s_b and can refund monero

    // self = a, Y = S_b, digest = tx_refund
    pub fn encsign(&self, Y: PublicKey, digest: SigHash) -> EncryptedSignature {
        let adaptor = Adaptor::<
            HashTranscript<Sha256, rand_chacha::ChaCha20Rng>,
            Deterministic<Sha256>,
        >::default();

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

impl From<Point> for PublicKey {
    fn from(p: Point) -> Self {
        Self(p)
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
    let adaptor = Adaptor::<HashTranscript<Sha256>, Deterministic<Sha256>>::default();

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

    Descriptor::Wsh(Wsh::new(miniscript).expect("a valid descriptor"))
}

pub fn recover(S: PublicKey, sig: Signature, encsig: EncryptedSignature) -> Result<SecretKey> {
    let adaptor = Adaptor::<HashTranscript<Sha256>, Deterministic<Sha256>>::default();

    let s = adaptor
        .recover_decryption_key(&S.0, &sig, &encsig)
        .map(SecretKey::from)
        .context("Failed to recover secret from adaptor signature")?;

    Ok(s)
}

pub async fn poll_until_block_height_is_gte(
    client: &crate::bitcoin::Wallet,
    target: BlockHeight,
) -> Result<()> {
    while client.get_block_height().await? < target {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Ok(())
}

pub async fn current_epoch(
    bitcoin_wallet: &crate::bitcoin::Wallet,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    lock_tx_id: ::bitcoin::Txid,
) -> Result<ExpiredTimelocks> {
    let current_block_height = bitcoin_wallet.get_block_height().await?;
    let lock_tx_height = bitcoin_wallet.transaction_block_height(lock_tx_id).await?;
    let cancel_timelock_height = lock_tx_height + cancel_timelock;
    let punish_timelock_height = cancel_timelock_height + punish_timelock;

    match (
        current_block_height < cancel_timelock_height,
        current_block_height < punish_timelock_height,
    ) {
        (true, _) => Ok(ExpiredTimelocks::None),
        (false, true) => Ok(ExpiredTimelocks::Cancel),
        (false, false) => Ok(ExpiredTimelocks::Punish),
    }
}

pub async fn wait_for_cancel_timelock_to_expire(
    bitcoin_wallet: &crate::bitcoin::Wallet,
    cancel_timelock: CancelTimelock,
    lock_tx_id: ::bitcoin::Txid,
) -> Result<()> {
    let tx_lock_height = bitcoin_wallet.transaction_block_height(lock_tx_id).await?;

    poll_until_block_height_is_gte(bitcoin_wallet, tx_lock_height + cancel_timelock).await?;
    Ok(())
}

#[derive(Clone, Copy, thiserror::Error, Debug)]
#[error("transaction does not spend anything")]
pub struct NoInputs;

#[derive(Clone, Copy, thiserror::Error, Debug)]
#[error("transaction has {0} inputs, expected 1")]
pub struct TooManyInputs(usize);

#[derive(Clone, Copy, thiserror::Error, Debug)]
#[error("empty witness stack")]
pub struct EmptyWitnessStack;

#[derive(Clone, Copy, thiserror::Error, Debug)]
#[error("input has {0} witnesses, expected 3")]
pub struct NotThreeWitnesses(usize);
