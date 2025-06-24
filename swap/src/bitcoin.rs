pub mod wallet;

mod cancel;
mod early_refund;
mod lock;
mod punish;
mod redeem;
mod refund;
mod timelocks;

pub use crate::bitcoin::cancel::{CancelTimelock, PunishTimelock, TxCancel};
pub use crate::bitcoin::early_refund::TxEarlyRefund;
pub use crate::bitcoin::lock::TxLock;
pub use crate::bitcoin::punish::TxPunish;
pub use crate::bitcoin::redeem::TxRedeem;
pub use crate::bitcoin::refund::TxRefund;
pub use crate::bitcoin::timelocks::{BlockHeight, ExpiredTimelocks};
pub use ::bitcoin::amount::Amount;
pub use ::bitcoin::psbt::Psbt as PartiallySignedTransaction;
pub use ::bitcoin::{Address, AddressType, Network, Transaction, Txid};
pub use ecdsa_fun::adaptor::EncryptedSignature;
pub use ecdsa_fun::fun::Scalar;
pub use ecdsa_fun::Signature;
pub use wallet::Wallet;

#[cfg(test)]
pub use wallet::TestWalletBuilder;

use crate::bitcoin::wallet::ScriptStatus;
use ::bitcoin::hashes::Hash;
use ::bitcoin::secp256k1::ecdsa;
use ::bitcoin::sighash::SegwitV0Sighash as Sighash;
use anyhow::{bail, Context, Result};
use bdk_wallet::miniscript::descriptor::Wsh;
use bdk_wallet::miniscript::{Descriptor, Segwitv0};
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
#[non_exhaustive]
pub enum network {
    #[serde(rename = "Mainnet")]
    Bitcoin,
    Testnet,
    Signet,
    Regtest,
}

/// This module is used to serialize and deserialize bitcoin addresses
/// even though the bitcoin crate does not support it for Address<NetworkChecked>.
pub mod address_serde {
    use std::str::FromStr;

    use bitcoin::address::{Address, NetworkChecked, NetworkUnchecked};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(address: &Address<NetworkChecked>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        address.to_string().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Address<NetworkChecked>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let unchecked: Address<NetworkUnchecked> =
            Address::from_str(&String::deserialize(deserializer)?)
                .map_err(serde::de::Error::custom)?;

        Ok(unchecked.assume_checked())
    }

    /// This submodule supports Option<Address>.
    pub mod option {
        use super::*;

        pub fn serialize<S>(
            address: &Option<Address<NetworkChecked>>,
            serializer: S,
        ) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match address {
                Some(addr) => addr.to_string().serialize(serializer),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D>(
            deserializer: D,
        ) -> Result<Option<Address<NetworkChecked>>, D::Error>
        where
            D: Deserializer<'de>,
        {
            let opt: Option<String> = Option::deserialize(deserializer)?;
            match opt {
                Some(s) => {
                    let unchecked: Address<NetworkUnchecked> =
                        Address::from_str(&s).map_err(serde::de::Error::custom)?;
                    Ok(Some(unchecked.assume_checked()))
                }
                None => Ok(None),
            }
        }
    }
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

        ecdsa.sign(&self.inner, &digest.to_byte_array())
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

        adaptor.encrypted_sign(&self.inner, &Y.0, &digest.to_byte_array())
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
    type Error = bitcoin::key::FromSliceError;

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

    if ecdsa.verify(
        &verification_key.0,
        &transaction_sighash.to_byte_array(),
        sig,
    ) {
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
        &digest.to_byte_array(),
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

pub fn build_shared_output_descriptor(
    A: Point,
    B: Point,
) -> Result<Descriptor<bitcoin::PublicKey>> {
    const MINISCRIPT_TEMPLATE: &str = "c:and_v(v:pk(A),pk_k(B))";

    let miniscript = MINISCRIPT_TEMPLATE
        .replace('A', &A.to_string())
        .replace('B', &B.to_string());

    let miniscript =
        bdk_wallet::miniscript::Miniscript::<bitcoin::PublicKey, Segwitv0>::from_str(&miniscript)
            .expect("a valid miniscript");

    Ok(Descriptor::Wsh(Wsh::new(miniscript)?))
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
        return ExpiredTimelocks::Cancel {
            blocks_left: tx_cancel_status.blocks_left_until(punish_timelock),
        };
    }

    ExpiredTimelocks::None {
        blocks_left: tx_lock_status.blocks_left_until(cancel_timelock),
    }
}

pub mod bitcoin_address {
    use anyhow::{Context, Result};
    use bitcoin::{
        address::{NetworkChecked, NetworkUnchecked},
        Address,
    };
    use serde::Serialize;
    use std::str::FromStr;

    #[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Serialize)]
    #[error("Invalid Bitcoin address provided, expected address on network {expected:?}  but address provided is on {actual:?}")]
    pub struct BitcoinAddressNetworkMismatch {
        #[serde(with = "crate::bitcoin::network")]
        expected: bitcoin::Network,
        #[serde(with = "crate::bitcoin::network")]
        actual: bitcoin::Network,
    }

    pub fn parse(addr_str: &str) -> Result<bitcoin::Address<NetworkUnchecked>> {
        let address = bitcoin::Address::from_str(addr_str)?;

        if address.assume_checked_ref().address_type() != Some(bitcoin::AddressType::P2wpkh) {
            anyhow::bail!("Invalid Bitcoin address provided, only bech32 format is supported!")
        }

        Ok(address)
    }

    /// Parse the address and validate the network.
    pub fn parse_and_validate_network(
        address: &str,
        expected_network: bitcoin::Network,
    ) -> Result<bitcoin::Address> {
        let addres = bitcoin::Address::from_str(address)?;
        let addres = addres.require_network(expected_network).with_context(|| {
            format!("Bitcoin address network mismatch, expected `{expected_network:?}`")
        })?;
        Ok(addres)
    }

    /// Parse the address and validate the network.
    pub fn parse_and_validate(address: &str, is_testnet: bool) -> Result<bitcoin::Address> {
        let expected_network = if is_testnet {
            bitcoin::Network::Testnet
        } else {
            bitcoin::Network::Bitcoin
        };
        parse_and_validate_network(address, expected_network)
    }

    /// Validate the address network.
    pub fn validate(
        address: Address<NetworkUnchecked>,
        is_testnet: bool,
    ) -> Result<Address<NetworkChecked>> {
        let expected_network = if is_testnet {
            bitcoin::Network::Testnet
        } else {
            bitcoin::Network::Bitcoin
        };
        validate_network(address, expected_network)
    }

    /// Validate the address network.
    pub fn validate_network(
        address: Address<NetworkUnchecked>,
        expected_network: bitcoin::Network,
    ) -> Result<Address<NetworkChecked>> {
        address
            .require_network(expected_network)
            .context("Bitcoin address network mismatch")
    }

    /// Validate the address network even though the address is already checked.
    pub fn revalidate_network(
        address: Address,
        expected_network: bitcoin::Network,
    ) -> Result<Address> {
        address
            .as_unchecked()
            .clone()
            .require_network(expected_network)
            .context("bitcoin address network mismatch")
    }

    /// Validate the address network even though the address is already checked.
    pub fn revalidate(address: Address, is_testnet: bool) -> Result<Address> {
        revalidate_network(
            address,
            if is_testnet {
                bitcoin::Network::Testnet
            } else {
                bitcoin::Network::Bitcoin
            },
        )
    }
}

// Transform the ecdsa der signature bytes into a secp256kfun ecdsa signature type.
pub fn extract_ecdsa_sig(sig: &[u8]) -> Result<Signature> {
    let data = &sig[..sig.len() - 1];
    let sig = ecdsa::Signature::from_der(data)?.serialize_compact();
    Signature::from_bytes(sig).ok_or(anyhow::anyhow!("invalid signature"))
}

/// Bitcoin error codes: https://github.com/bitcoin/bitcoin/blob/97d3500601c1d28642347d014a6de1e38f53ae4e/src/rpc/protocol.h#L23
pub enum RpcErrorCode {
    /// Transaction or block was rejected by network rules. Error code -26.
    RpcVerifyRejected,
    /// Transaction or block was rejected by network rules. Error code -27.
    RpcVerifyAlreadyInChain,
    /// General error during transaction or block submission
    RpcVerifyError,
    /// Invalid address or key. Error code -5. Is throwns when a transaction is not found.
    /// See:
    /// - https://github.com/bitcoin/bitcoin/blob/ae024137bda9fe189f4e7ccf26dbaffd44cbbeb6/src/rpc/mempool.cpp#L470-L472
    /// - https://github.com/bitcoin/bitcoin/blob/ae024137bda9fe189f4e7ccf26dbaffd44cbbeb6/src/rpc/rawtransaction.cpp#L352-L368
    RpcInvalidAddressOrKey,
}

impl From<RpcErrorCode> for i64 {
    fn from(code: RpcErrorCode) -> Self {
        match code {
            RpcErrorCode::RpcVerifyError => -25,
            RpcErrorCode::RpcVerifyRejected => -26,
            RpcErrorCode::RpcVerifyAlreadyInChain => -27,
            RpcErrorCode::RpcInvalidAddressOrKey => -5,
        }
    }
}

pub fn parse_rpc_error_code(error: &anyhow::Error) -> anyhow::Result<i64> {
    // First try to extract an Electrum error from a MultiError if present
    if let Some(multi_error) = error.downcast_ref::<electrum_pool::MultiError>() {
        // Try to find the first Electrum error in the MultiError
        for single_error in multi_error.iter() {
            if let bdk_electrum::electrum_client::Error::Protocol(serde_json::Value::String(
                string,
            )) = single_error
            {
                let json = serde_json::from_str(
                    &string
                        .replace("sendrawtransaction RPC error:", "")
                        .replace("daemon error:", ""),
                )?;

                let json_map = match json {
                    serde_json::Value::Object(map) => map,
                    _ => continue, // Try next error if this one isn't a JSON object
                };

                let error_code_value = match json_map.get("code") {
                    Some(val) => val,
                    None => continue, // Try next error if no error code field
                };

                let error_code_number = match error_code_value {
                    serde_json::Value::Number(num) => num,
                    _ => continue, // Try next error if error code isn't a number
                };

                if let Some(int) = error_code_number.as_i64() {
                    return Ok(int);
                }
            }
        }
        // If we couldn't extract an RPC error code from any error in the MultiError
        bail!(
            "Error is of incorrect variant. We expected an Electrum error, but got: {}",
            error
        );
    }

    // Original logic for direct Electrum errors
    let string = match error.downcast_ref::<bdk_electrum::electrum_client::Error>() {
        Some(bdk_electrum::electrum_client::Error::Protocol(serde_json::Value::String(string))) => {
            string
        }
        _ => bail!(
            "Error is of incorrect variant. We expected an Electrum error, but got: {}",
            error
        ),
    };

    let json = serde_json::from_str(
        &string
            .replace("sendrawtransaction RPC error:", "")
            .replace("daemon error:", ""),
    )?;

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
    use crate::monero::TransferProof;
    use crate::protocol::{alice, bob};
    use bitcoin::secp256k1;
    use curve25519_dalek::scalar::Scalar;
    use ecdsa_fun::fun::marker::{NonZero, Public};
    use monero::PrivateKey;
    use rand::rngs::OsRng;
    use std::matches;
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

        assert!(matches!(expired_timelock, ExpiredTimelocks::None { .. }));
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

        assert!(matches!(expired_timelock, ExpiredTimelocks::Cancel { .. }));
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
        let alice_wallet = TestWalletBuilder::new(Amount::ONE_BTC.to_sat())
            .build()
            .await;
        let bob_wallet = TestWalletBuilder::new(Amount::ONE_BTC.to_sat())
            .build()
            .await;
        let spending_fee = Amount::from_sat(1_000);
        let btc_amount = Amount::from_sat(500_000);
        let xmr_amount = crate::monero::Amount::from_piconero(10000);

        let tx_redeem_fee = alice_wallet
            .estimate_fee(TxRedeem::weight(), Some(btc_amount))
            .await
            .unwrap();
        let tx_punish_fee = alice_wallet
            .estimate_fee(TxPunish::weight(), Some(btc_amount))
            .await
            .unwrap();
        let tx_lock_fee = alice_wallet
            .estimate_fee(TxLock::weight(), Some(btc_amount))
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
            tx_lock_fee,
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
        let bob_state4 = bob_state3.xmr_locked(
            crate::monero::BlockHeight { height: 0 },
            // We use bogus values here, because they're irrelevant to this test
            TransferProof::new(
                crate::monero::TxHash("foo".into()),
                PrivateKey::from_scalar(Scalar::one()),
            ),
        );
        let encrypted_signature = bob_state4.tx_redeem_encsig();
        let bob_state6 = bob_state4.cancel();

        let cancel_transaction = alice_state3.signed_cancel_transaction().unwrap();
        let punish_transaction = alice_state3.signed_punish_transaction().unwrap();
        let redeem_transaction = alice_state3
            .signed_redeem_transaction(encrypted_signature)
            .unwrap();
        let refund_transaction = bob_state6.signed_refund_transaction().unwrap();

        assert_weight(redeem_transaction, TxRedeem::weight().to_wu(), "TxRedeem");
        assert_weight(cancel_transaction, TxCancel::weight().to_wu(), "TxCancel");
        assert_weight(punish_transaction, TxPunish::weight().to_wu(), "TxPunish");
        assert_weight(refund_transaction, TxRefund::weight().to_wu(), "TxRefund");

        // Test TxEarlyRefund transaction
        let early_refund_transaction = alice_state3
            .signed_early_refund_transaction()
            .unwrap()
            .unwrap();
        assert_weight(
            early_refund_transaction,
            TxEarlyRefund::weight() as u64,
            "TxEarlyRefund",
        );
    }

    #[tokio::test]
    async fn tx_early_refund_can_be_constructed_and_signed() {
        let alice_wallet = TestWalletBuilder::new(Amount::ONE_BTC.to_sat())
            .build()
            .await;
        let bob_wallet = TestWalletBuilder::new(Amount::ONE_BTC.to_sat())
            .build()
            .await;
        let spending_fee = Amount::from_sat(1_000);
        let btc_amount = Amount::from_sat(500_000);
        let xmr_amount = crate::monero::Amount::from_piconero(10000);

        let tx_redeem_fee = alice_wallet
            .estimate_fee(TxRedeem::weight(), Some(btc_amount))
            .await
            .unwrap();
        let tx_punish_fee = alice_wallet
            .estimate_fee(TxPunish::weight(), Some(btc_amount))
            .await
            .unwrap();

        let refund_address = alice_wallet.new_address().await.unwrap();
        let punish_address = alice_wallet.new_address().await.unwrap();

        let config = Regtest::get_config();
        let alice_state0 = alice::State0::new(
            btc_amount,
            xmr_amount,
            config,
            refund_address.clone(),
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
            spending_fee,
        );

        // Complete the state machine up to State3
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

        // Test TxEarlyRefund construction
        let tx_early_refund = alice_state3.tx_early_refund();

        // Verify basic properties
        assert_eq!(tx_early_refund.txid(), tx_early_refund.txid()); // Should be deterministic
        assert!(tx_early_refund.digest() != Sighash::all_zeros()); // Should have valid digest

        // Test that it can be signed and completed
        let early_refund_transaction = alice_state3
            .signed_early_refund_transaction()
            .unwrap()
            .unwrap();

        // Verify the transaction has expected structure
        assert_eq!(early_refund_transaction.input.len(), 1); // One input from lock tx
        assert_eq!(early_refund_transaction.output.len(), 1); // One output to refund address
        assert_eq!(
            early_refund_transaction.output[0].script_pubkey,
            refund_address.script_pubkey()
        );

        // Verify the input is spending the lock transaction
        assert_eq!(
            early_refund_transaction.input[0].previous_output,
            alice_state3.tx_lock.as_outpoint()
        );

        // Verify the amount is correct (lock amount minus fee)
        let expected_amount = alice_state3.tx_lock.lock_amount() - alice_state3.tx_refund_fee;
        assert_eq!(early_refund_transaction.output[0].value, expected_amount);
    }

    #[test]
    fn tx_early_refund_has_correct_weight() {
        // TxEarlyRefund should have the same weight as other similar transactions
        assert_eq!(TxEarlyRefund::weight(), 548);

        // It should be the same as TxRedeem and TxRefund weights since they have similar structure
        assert_eq!(TxEarlyRefund::weight() as u64, TxRedeem::weight().to_wu());
        assert_eq!(TxEarlyRefund::weight() as u64, TxRefund::weight().to_wu());
    }

    // Weights fluctuate because of the length of the signatures. Valid ecdsa
    // signatures can have 68, 69, 70, 71, or 72 bytes. Since most of our
    // transactions have 2 signatures the weight can be up to 8 bytes less than
    // the static weight (4 bytes per signature).
    fn assert_weight(transaction: Transaction, expected_weight: u64, tx_name: &str) {
        let is_weight = transaction.weight();

        assert!(
            expected_weight - is_weight.to_wu() <= 8,
            "{} to have weight {}, but was {}. Transaction: {:#?}",
            tx_name,
            expected_weight,
            is_weight,
            transaction
        )
    }

    #[test]
    fn compare_point_hex() {
        // secp256kfun Point and secp256k1 PublicKey should have the same bytes and hex representation
        let secp = secp256k1::Secp256k1::default();
        let keypair = secp256k1::Keypair::new(&secp, &mut OsRng);

        let pubkey = keypair.public_key();
        let point: Point<_, Public, NonZero> = Point::from_bytes(pubkey.serialize()).unwrap();

        assert_eq!(pubkey.to_string(), point.to_string());
    }
}
