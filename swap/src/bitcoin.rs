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
pub use ::bitcoin::util::psbt::PartiallySignedTransaction;
pub use ::bitcoin::{Address, Network, Transaction, Txid};
pub use ecdsa_fun::adaptor::EncryptedSignature;
pub use ecdsa_fun::fun::Scalar;
pub use ecdsa_fun::Signature;
pub use wallet::Wallet;

#[cfg(test)]
pub use wallet::WalletBuilder;

use crate::bitcoin::wallet::ScriptStatus;
use ::bitcoin::hashes::hex::ToHex;
use ::bitcoin::hashes::Hash;
use ::bitcoin::{secp256k1, Sighash};
use anyhow::{bail, Context, Result};
use bdk::miniscript::descriptor::Wsh;
use bdk::miniscript::{Descriptor, Segwitv0};
use ecdsa_fun::adaptor::{Adaptor, HashTranscript};
use ecdsa_fun::fun::Point;
use ecdsa_fun::nonce::Deterministic;
use ecdsa_fun::ECDSA;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::str::FromStr;

#[derive(Serialize, Deserialize)]
#[serde(remote = "Network")]
#[allow(non_camel_case_types)]
pub enum network {
    #[serde(rename = "Mainnet")]
    Bitcoin,
    Testnet,
    Signet,
    Regtest,
}

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

    pub fn sign(&self, digest: Sighash) -> Signature {
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
    pub fn encsign(&self, Y: PublicKey, digest: Sighash) -> EncryptedSignature {
        let adaptor = Adaptor::<
            HashTranscript<Sha256, rand_chacha::ChaCha20Rng>,
            Deterministic<Sha256>,
        >::default();

        adaptor.encrypted_sign(&self.inner, &Y.0, &digest.into_inner())
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicKey(Point);

impl PublicKey {
    #[cfg(test)]
    pub fn random() -> Self {
        Self(Point::random(&mut rand::thread_rng()))
    }
}

impl From<PublicKey> for Point {
    fn from(from: PublicKey) -> Self {
        from.0
    }
}

impl TryFrom<PublicKey> for bitcoin::PublicKey {
    type Error = bitcoin::util::key::Error;

    fn try_from(pubkey: PublicKey) -> Result<Self, Self::Error> {
        let bytes = pubkey.0.to_bytes();
        bitcoin::PublicKey::from_slice(&bytes)
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
    transaction_sighash: &Sighash,
    sig: &Signature,
) -> Result<()> {
    let ecdsa = ECDSA::verify_only();

    if ecdsa.verify(&verification_key.0, &transaction_sighash.into_inner(), sig) {
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
    digest: &Sighash,
    encsig: &EncryptedSignature,
) -> Result<()> {
    let adaptor = Adaptor::<HashTranscript<Sha256>, Deterministic<Sha256>>::default();

    if adaptor.verify_encrypted_signature(
        &verification_key.0,
        &encryption_key.0,
        &digest.into_inner(),
        encsig,
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

    let miniscript = MINISCRIPT_TEMPLATE.replace('A', &A).replace('B', &B);

    let miniscript =
        bdk::miniscript::Miniscript::<bitcoin::PublicKey, Segwitv0>::from_str(&miniscript)
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

pub fn current_epoch(
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    tx_lock_status: ScriptStatus,
    tx_cancel_status: ScriptStatus,
) -> ExpiredTimelocks {
    if tx_cancel_status.is_confirmed_with(punish_timelock) {
        return ExpiredTimelocks::Punish;
    }

    if tx_lock_status.is_confirmed_with(cancel_timelock) {
        return ExpiredTimelocks::Cancel;
    }

    ExpiredTimelocks::None
}

/// Bitcoin error codes: https://github.com/bitcoin/bitcoin/blob/97d3500601c1d28642347d014a6de1e38f53ae4e/src/rpc/protocol.h#L23
pub enum RpcErrorCode {
    /// Transaction or block was rejected by network rules. Error code -26.
    RpcVerifyRejected,
    /// Transaction or block was rejected by network rules. Error code -27.
    RpcVerifyAlreadyInChain,
    /// General error during transaction or block submission
    RpcVerifyError,
}

impl From<RpcErrorCode> for i64 {
    fn from(code: RpcErrorCode) -> Self {
        match code {
            RpcErrorCode::RpcVerifyError => -25,
            RpcErrorCode::RpcVerifyRejected => -26,
            RpcErrorCode::RpcVerifyAlreadyInChain => -27,
        }
    }
}

pub fn parse_rpc_error_code(error: &anyhow::Error) -> anyhow::Result<i64> {
    let string = match error.downcast_ref::<bdk::Error>() {
        Some(bdk::Error::Electrum(bdk::electrum_client::Error::Protocol(
            serde_json::Value::String(string),
        ))) => string,
        _ => bail!("Error is of incorrect variant:{}", error),
    };

    let json = serde_json::from_str(&string.replace("sendrawtransaction RPC error:", ""))?;

    let json_map = match json {
        serde_json::Value::Object(map) => map,
        _ => bail!("Json error is not json object "),
    };

    let error_code_value = match json_map.get("code") {
        Some(val) => val,
        None => bail!("No error code field"),
    };

    let error_code_number = match error_code_value {
        serde_json::Value::Number(num) => num,
        _ => bail!("Error code is not a number"),
    };

    if let Some(int) = error_code_number.as_i64() {
        Ok(int)
    } else {
        bail!("Error code is not an unsigned integer")
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::{GetConfig, Regtest};
    use crate::protocol::{alice, bob};
    use rand::rngs::OsRng;
    use uuid::Uuid;

    #[test]
    fn lock_confirmations_le_to_cancel_timelock_no_timelock_expired() {
        let tx_lock_status = ScriptStatus::from_confirmations(4);
        let tx_cancel_status = ScriptStatus::Unseen;

        let expired_timelock = current_epoch(
            CancelTimelock::new(5),
            PunishTimelock::new(5),
            tx_lock_status,
            tx_cancel_status,
        );

        assert_eq!(expired_timelock, ExpiredTimelocks::None)
    }

    #[test]
    fn lock_confirmations_ge_to_cancel_timelock_cancel_timelock_expired() {
        let tx_lock_status = ScriptStatus::from_confirmations(5);
        let tx_cancel_status = ScriptStatus::Unseen;

        let expired_timelock = current_epoch(
            CancelTimelock::new(5),
            PunishTimelock::new(5),
            tx_lock_status,
            tx_cancel_status,
        );

        assert_eq!(expired_timelock, ExpiredTimelocks::Cancel)
    }

    #[test]
    fn cancel_confirmations_ge_to_punish_timelock_punish_timelock_expired() {
        let tx_lock_status = ScriptStatus::from_confirmations(10);
        let tx_cancel_status = ScriptStatus::from_confirmations(5);

        let expired_timelock = current_epoch(
            CancelTimelock::new(5),
            PunishTimelock::new(5),
            tx_lock_status,
            tx_cancel_status,
        );

        assert_eq!(expired_timelock, ExpiredTimelocks::Punish)
    }

    #[tokio::test]
    async fn calculate_transaction_weights() {
        let alice_wallet = WalletBuilder::new(Amount::ONE_BTC.to_sat()).build();
        let bob_wallet = WalletBuilder::new(Amount::ONE_BTC.to_sat()).build();
        let spending_fee = Amount::from_sat(1_000);
        let btc_amount = Amount::from_sat(500_000);
        let xmr_amount = crate::monero::Amount::from_piconero(10000);

        let tx_redeem_fee = alice_wallet
            .estimate_fee(TxRedeem::weight(), btc_amount)
            .await
            .unwrap();
        let tx_punish_fee = alice_wallet
            .estimate_fee(TxPunish::weight(), btc_amount)
            .await
            .unwrap();
        let redeem_address = alice_wallet.new_address().await.unwrap();
        let punish_address = alice_wallet.new_address().await.unwrap();

        let config = Regtest::get_config();
        let alice_state0 = alice::State0::new(
            btc_amount,
            xmr_amount,
            config,
            redeem_address,
            punish_address,
            tx_redeem_fee,
            tx_punish_fee,
            &mut OsRng,
        );

        let bob_state0 = bob::State0::new(
            Uuid::new_v4(),
            &mut OsRng,
            btc_amount,
            xmr_amount,
            config.bitcoin_cancel_timelock,
            config.bitcoin_punish_timelock,
            bob_wallet.new_address().await.unwrap(),
            config.monero_finality_confirmations,
            spending_fee,
            spending_fee,
        );

        let message0 = bob_state0.next_message();

        let (_, alice_state1) = alice_state0.receive(message0).unwrap();
        let alice_message1 = alice_state1.next_message();

        let bob_state1 = bob_state0
            .receive(&bob_wallet, alice_message1)
            .await
            .unwrap();
        let bob_message2 = bob_state1.next_message();

        let alice_state2 = alice_state1.receive(bob_message2).unwrap();
        let alice_message3 = alice_state2.next_message();

        let bob_state2 = bob_state1.receive(alice_message3).unwrap();
        let bob_message4 = bob_state2.next_message();

        let alice_state3 = alice_state2.receive(bob_message4).unwrap();

        let (bob_state3, _tx_lock) = bob_state2.lock_btc().await.unwrap();
        let bob_state4 = bob_state3.xmr_locked(monero_rpc::wallet::BlockHeight { height: 0 });
        let encrypted_signature = bob_state4.tx_redeem_encsig();
        let bob_state6 = bob_state4.cancel();

        let cancel_transaction = alice_state3.signed_cancel_transaction().unwrap();
        let punish_transaction = alice_state3.signed_punish_transaction().unwrap();
        let redeem_transaction = alice_state3
            .signed_redeem_transaction(encrypted_signature)
            .unwrap();
        let refund_transaction = bob_state6.signed_refund_transaction().unwrap();

        assert_weight(redeem_transaction, TxRedeem::weight(), "TxRedeem");
        assert_weight(cancel_transaction, TxCancel::weight(), "TxCancel");
        assert_weight(punish_transaction, TxPunish::weight(), "TxPunish");
        assert_weight(refund_transaction, TxRefund::weight(), "TxRefund");
    }

    // Weights fluctuate because of the length of the signatures. Valid ecdsa
    // signatures can have 68, 69, 70, 71, or 72 bytes. Since most of our
    // transactions have 2 signatures the weight can be up to 8 bytes less than
    // the static weight (4 bytes per signature).
    fn assert_weight(transaction: Transaction, expected_weight: usize, tx_name: &str) {
        let is_weight = transaction.weight();

        assert!(
            expected_weight - is_weight <= 8,
            "{} to have weight {}, but was {}. Transaction: {:#?}",
            tx_name,
            expected_weight,
            is_weight,
            transaction
        )
    }
}
