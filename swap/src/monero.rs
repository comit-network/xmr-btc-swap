pub mod wallet;
pub mod wallet_rpc;

pub use ::monero::network::Network;
pub use ::monero::{Address, PrivateKey, PublicKey};
pub use curve25519_dalek::scalar::Scalar;
pub use wallet::{Daemon, Wallet, Wallets, WatchRequest};

use crate::bitcoin;
use anyhow::{bail, Result};
use rand::{CryptoRng, RngCore};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::fmt;
use std::ops::{Add, Mul, Sub};
use std::str::FromStr;
use typeshare::typeshare;

pub const PICONERO_OFFSET: u64 = 1_000_000_000_000;

#[derive(Serialize, Deserialize)]
#[serde(remote = "Network")]
#[allow(non_camel_case_types)]
pub enum network {
    Mainnet,
    Stagenet,
    Testnet,
}

/// A Monero block height.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockHeight {
    pub height: u64,
}

impl fmt::Display for BlockHeight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.height)
    }
}

pub fn private_key_from_secp256k1_scalar(scalar: bitcoin::Scalar) -> PrivateKey {
    let mut bytes = scalar.to_bytes();

    // we must reverse the bytes because a secp256k1 scalar is big endian, whereas a
    // ed25519 scalar is little endian
    bytes.reverse();

    PrivateKey::from_scalar(Scalar::from_bytes_mod_order(bytes))
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivateViewKey(#[serde(with = "monero_private_key")] PrivateKey);

impl fmt::Display for PrivateViewKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Delegate to the Display implementation of PrivateKey
        write!(f, "{}", self.0)
    }
}

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

/// Our own monero amount type, which we need because the monero crate
/// doesn't implement Serialize and Deserialize.
#[derive(Debug, Copy, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd)]
#[typeshare(serialized_as = "number")]
pub struct Amount(u64);

// TX Fees on Monero can be found here:
// - https://www.monero.how/monero-transaction-fees
// - https://bitinfocharts.com/comparison/monero-transactionfees.html#1y
//
// In the last year the highest avg fee on any given day was around 0.00075 XMR
// We use a multiplier of 4x to stay safe
// 0.00075 XMR * 4 = 0.003 XMR (around $1 as of Jun. 4th 2025)
// We DO NOT use this fee to construct any transactions. It is only to **estimate** how much
// we need to reserve for the fee when determining our max giveable amount
// We use a VERY conservative value here to stay on the safe side. We want to avoid not being able
// to lock as much as we previously estimated.
pub const CONSERVATIVE_MONERO_FEE: Amount = Amount::from_piconero(3_000_000_000);

impl Amount {
    pub const ZERO: Self = Self(0);
    pub const ONE_XMR: Self = Self(PICONERO_OFFSET);
    /// Create an [Amount] with piconero precision and the given number of
    /// piconeros.
    ///
    /// A piconero (a.k.a atomic unit) is equal to 1e-12 XMR.
    pub const fn from_piconero(amount: u64) -> Self {
        Amount(amount)
    }

    /// Return Monero Amount as Piconero.
    pub fn as_piconero(&self) -> u64 {
        self.0
    }

    /// Return Monero Amount as XMR.
    pub fn as_xmr(&self) -> f64 {
        let amount_decimal = Decimal::from(self.0);
        let offset_decimal = Decimal::from(PICONERO_OFFSET);
        let result = amount_decimal / offset_decimal;

        // Convert to f64 only at the end, after the division
        result
            .to_f64()
            .expect("Conversion from piconero to XMR should not overflow f64")
    }

    /// Calculate the conservative max giveable of Monero we can spent given [`self`] is the balance
    /// of a Monero wallet
    /// This is going to be LESS than we can really spent because we assume a high fee
    pub fn max_conservative_giveable(&self) -> Self {
        let pico_minus_fee = self
            .as_piconero()
            .saturating_sub(CONSERVATIVE_MONERO_FEE.as_piconero());

        Self::from_piconero(pico_minus_fee)
    }

    /// Calculate the Monero balance needed to send the [`self`] Amount to another address
    /// E.g: Amount(1 XMR).min_conservative_balance_to_spend() with a fee of 0.1 XMR would be 1.1 XMR
    /// This is going to be MORE than we really need because we assume a high fee
    pub fn min_conservative_balance_to_spend(&self) -> Self {
        let pico_minus_fee = self
            .as_piconero()
            .saturating_add(CONSERVATIVE_MONERO_FEE.as_piconero());

        Self::from_piconero(pico_minus_fee)
    }

    /// Calculate the maximum amount of Bitcoin that can be bought at a given
    /// asking price for this amount of Monero including the median fee.
    pub fn max_bitcoin_for_price(&self, ask_price: bitcoin::Amount) -> Option<bitcoin::Amount> {
        let pico_minus_fee = self.max_conservative_giveable();

        if pico_minus_fee.as_piconero() == 0 {
            return Some(bitcoin::Amount::ZERO);
        }

        // safely convert the BTC/XMR rate to sat/pico
        let ask_sats = Decimal::from(ask_price.to_sat());
        let pico_per_xmr = Decimal::from(PICONERO_OFFSET);
        let ask_sats_per_pico = ask_sats / pico_per_xmr;

        let pico = Decimal::from(pico_minus_fee.as_piconero());
        let max_sats = pico.checked_mul(ask_sats_per_pico)?;
        let satoshi = max_sats.to_u64()?;

        Some(bitcoin::Amount::from_sat(satoshi))
    }

    pub fn from_monero(amount: f64) -> Result<Self> {
        let decimal = Decimal::try_from(amount)?;
        Self::from_decimal(decimal)
    }

    pub fn parse_monero(amount: &str) -> Result<Self> {
        let decimal = Decimal::from_str(amount)?;
        Self::from_decimal(decimal)
    }

    pub fn as_piconero_decimal(&self) -> Decimal {
        Decimal::from(self.as_piconero())
    }

    fn from_decimal(amount: Decimal) -> Result<Self> {
        let piconeros_dec =
            amount.mul(Decimal::from_u64(PICONERO_OFFSET).expect("constant to fit into u64"));
        let piconeros = piconeros_dec
            .to_u64()
            .ok_or_else(|| OverflowError(amount.to_string()))?;
        Ok(Amount(piconeros))
    }

    /// Subtract but throw an error on underflow.
    pub fn checked_sub(self, rhs: Amount) -> Result<Self> {
        if self.0 < rhs.0 {
            bail!("checked sub would underflow");
        }

        Ok(Amount::from_piconero(self.0 - rhs.0))
    }
}

/// A Monero address with an associated percentage and human-readable label.
///
/// This structure represents a destination address for Monero transactions
/// along with the percentage of funds it should receive and a descriptive label.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[typeshare]
pub struct LabeledMoneroAddress {
    #[typeshare(serialized_as = "string")]
    address: monero::Address,
    #[typeshare(serialized_as = "number")]
    percentage: Decimal,
    label: String,
}

impl LabeledMoneroAddress {
    /// Creates a new labeled Monero address.
    ///
    /// # Arguments
    ///
    /// * `address` - The Monero address
    /// * `percentage` - The percentage of funds (between 0.0 and 1.0)
    /// * `label` - A human-readable label for this address
    ///
    /// # Errors
    ///
    /// Returns an error if the percentage is not between 0.0 and 1.0 inclusive.
    pub fn new(
        address: monero::Address,
        percentage: Decimal,
        label: String,
    ) -> Result<Self, String> {
        if percentage < Decimal::ZERO || percentage > Decimal::ONE {
            return Err(format!(
                "Percentage must be between 0 and 1 inclusive, got: {}",
                percentage
            ));
        }

        Ok(Self {
            address,
            percentage,
            label,
        })
    }

    /// Returns the Monero address.
    pub fn address(&self) -> monero::Address {
        self.address
    }

    /// Returns the percentage as a decimal.
    pub fn percentage(&self) -> Decimal {
        self.percentage
    }

    /// Returns the human-readable label.
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// A collection of labeled Monero addresses that can receive funds in a transaction.
///
/// This structure manages multiple destination addresses with their associated
/// percentages and labels. It's used for splitting Monero transactions across
/// multiple recipients, such as for donations or multi-destination swaps.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[typeshare]
pub struct MoneroAddressPool(Vec<LabeledMoneroAddress>);

use rust_decimal::prelude::ToPrimitive;

impl MoneroAddressPool {
    /// Creates a new address pool from a vector of labeled addresses.
    ///
    /// # Arguments
    ///
    /// * `addresses` - Vector of labeled Monero addresses
    pub fn new(addresses: Vec<LabeledMoneroAddress>) -> Self {
        Self(addresses)
    }

    /// Returns a vector of all Monero addresses in the pool.
    pub fn addresses(&self) -> Vec<monero::Address> {
        self.0.iter().map(|address| address.address()).collect()
    }

    /// Returns a vector of all percentages as f64 values (0-1 range).
    pub fn percentages(&self) -> Vec<f64> {
        self.0
            .iter()
            .map(|address| {
                address
                    .percentage()
                    .to_f64()
                    .expect("Decimal should convert to f64")
            })
            .collect()
    }

    /// Returns an iterator over the labeled addresses.
    pub fn iter(&self) -> impl Iterator<Item = &LabeledMoneroAddress> {
        self.0.iter()
    }

    /// Validates that all addresses in the pool are on the expected network.
    ///
    /// # Arguments
    ///
    /// * `network` - The expected Monero network
    ///
    /// # Errors
    ///
    /// Returns an error if any address is on a different network than expected.
    pub fn assert_network(&self, network: Network) -> Result<()> {
        for address in self.0.iter() {
            if address.address().network != network {
                bail!("Address pool contains addresses on the wrong network (address {} is on {:?}, expected {:?})", address.address(), address.address().network, network);
            }
        }

        Ok(())
    }

    /// Assert that the sum of the percentages in the address pool is 1 (allowing for a small tolerance)
    pub fn assert_sum_to_one(&self) -> Result<()> {
        let sum = self
            .0
            .iter()
            .map(|address| address.percentage())
            .sum::<Decimal>();

        const TOLERANCE: f64 = 1e-6;

        if (sum - Decimal::ONE).abs() > Decimal::from_f64(TOLERANCE).unwrap() {
            bail!("Address pool percentages do not sum to 1");
        }

        Ok(())
    }
}

impl From<::monero::Address> for MoneroAddressPool {
    fn from(address: ::monero::Address) -> Self {
        Self(vec![LabeledMoneroAddress::new(
            address,
            Decimal::from(1),
            "user address".to_string(),
        )
        .expect("Percentage 1 is always valid")])
    }
}

impl Add for Amount {
    type Output = Amount;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub<Amount> for Amount {
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

impl From<::monero::Amount> for Amount {
    fn from(from: ::monero::Amount) -> Self {
        Amount::from_piconero(from.as_pico())
    }
}

impl From<Amount> for ::monero::Amount {
    fn from(from: Amount) -> Self {
        ::monero::Amount::from_pico(from.as_piconero())
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut decimal = Decimal::from(self.0);
        decimal
            .set_scale(12)
            .expect("12 is smaller than max precision of 28");
        write!(f, "{} XMR", decimal)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxHash(pub String);

impl From<TxHash> for String {
    fn from(from: TxHash) -> Self {
        from.0
    }
}

impl fmt::Debug for TxHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for TxHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("expected {expected}, got {actual}")]
pub struct InsufficientFunds {
    pub expected: Amount,
    pub actual: Amount,
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[error("Overflow, cannot convert {0} to u64")]
pub struct OverflowError(pub String);

pub mod monero_private_key {
    use monero::consensus::{Decodable, Encodable};
    use monero::PrivateKey;
    use serde::de::Visitor;
    use serde::ser::Error;
    use serde::{de, Deserializer, Serializer};
    use std::fmt;
    use std::io::Cursor;

    struct BytesVisitor;

    impl<'de> Visitor<'de> for BytesVisitor {
        type Value = PrivateKey;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "a byte array representing a Monero private key")
        }

        fn visit_bytes<E>(self, s: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let mut s = s;
            PrivateKey::consensus_decode(&mut s).map_err(|err| E::custom(format!("{:?}", err)))
        }

        fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let bytes = hex::decode(s).map_err(|err| E::custom(format!("{:?}", err)))?;
            PrivateKey::consensus_decode(&mut bytes.as_slice())
                .map_err(|err| E::custom(format!("{:?}", err)))
        }
    }

    pub fn serialize<S>(x: &PrivateKey, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut bytes = Cursor::new(vec![]);
        x.consensus_encode(&mut bytes)
            .map_err(|err| S::Error::custom(format!("{:?}", err)))?;
        if s.is_human_readable() {
            s.serialize_str(&hex::encode(bytes.into_inner()))
        } else {
            s.serialize_bytes(bytes.into_inner().as_ref())
        }
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<PrivateKey, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let key = {
            if deserializer.is_human_readable() {
                deserializer.deserialize_string(BytesVisitor)?
            } else {
                deserializer.deserialize_bytes(BytesVisitor)?
            }
        };
        Ok(key)
    }
}

pub mod monero_amount {
    use crate::monero::Amount;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(x: &Amount, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_u64(x.as_piconero())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Amount, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let picos = u64::deserialize(deserializer)?;
        let amount = Amount::from_piconero(picos);

        Ok(amount)
    }
}

pub mod monero_address {
    use anyhow::{bail, Context, Result};
    use std::str::FromStr;

    #[derive(thiserror::Error, Debug, Clone, Copy, PartialEq)]
    #[error("Invalid monero address provided, expected address on network {expected:?} but address provided is on {actual:?}")]
    pub struct MoneroAddressNetworkMismatch {
        pub expected: monero::Network,
        pub actual: monero::Network,
    }

    pub fn parse(s: &str) -> Result<monero::Address> {
        monero::Address::from_str(s).with_context(|| {
            format!(
                "Failed to parse {} as a monero address, please make sure it is a valid address",
                s
            )
        })
    }

    pub fn validate(
        address: monero::Address,
        expected_network: monero::Network,
    ) -> Result<monero::Address> {
        if address.network != expected_network {
            bail!(MoneroAddressNetworkMismatch {
                expected: expected_network,
                actual: address.network,
            });
        }
        Ok(address)
    }

    pub fn validate_is_testnet(
        address: monero::Address,
        is_testnet: bool,
    ) -> Result<monero::Address> {
        let expected_network = if is_testnet {
            monero::Network::Stagenet
        } else {
            monero::Network::Mainnet
        };
        validate(address, expected_network)
    }
}

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

    #[test]
    fn max_bitcoin_to_trade() {
        // sanity check: if the asking price is 1 BTC / 1 XMR
        // and we have μ XMR + fee
        // then max BTC we can buy is μ
        let ask = bitcoin::Amount::from_btc(1.0).unwrap();

        let xmr = Amount::parse_monero("1.0").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(1.0).unwrap());

        let xmr = Amount::parse_monero("0.5").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(0.5).unwrap());

        let xmr = Amount::parse_monero("2.5").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(2.5).unwrap());

        let xmr = Amount::parse_monero("420").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(420.0).unwrap());

        let xmr = Amount::parse_monero("0.00001").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(0.00001).unwrap());

        // other ask prices

        let ask = bitcoin::Amount::from_btc(0.5).unwrap();
        let xmr = Amount::parse_monero("2").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(1.0).unwrap());

        let ask = bitcoin::Amount::from_btc(2.0).unwrap();
        let xmr = Amount::parse_monero("1").unwrap() + CONSERVATIVE_MONERO_FEE;
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_btc(2.0).unwrap());

        let ask = bitcoin::Amount::from_sat(382_900);
        let xmr = Amount::parse_monero("10").unwrap();
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_sat(3_827_851));

        // example from https://github.com/comit-network/xmr-btc-swap/issues/1084
        // with rate from kraken at that time
        let ask = bitcoin::Amount::from_sat(685_800);
        let xmr = Amount::parse_monero("0.826286435921").unwrap();
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(btc, bitcoin::Amount::from_sat(564_609));
    }

    #[test]
    fn max_bitcoin_to_trade_overflow() {
        let xmr = Amount::from_monero(30.0).unwrap();
        let ask = bitcoin::Amount::from_sat(728_688);
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(bitcoin::Amount::from_sat(21_858_453), btc);

        let xmr = Amount::from_piconero(u64::MAX);
        let ask = bitcoin::Amount::from_sat(u64::MAX);
        let btc = xmr.max_bitcoin_for_price(ask);

        assert!(btc.is_none());
    }

    #[test]
    fn geting_max_bitcoin_to_trade_with_balance_smaller_than_locking_fee() {
        let ask = bitcoin::Amount::from_sat(382_900);
        let xmr = Amount::parse_monero("0.00001").unwrap();
        let btc = xmr.max_bitcoin_for_price(ask).unwrap();

        assert_eq!(bitcoin::Amount::ZERO, btc);
    }

    use rand::rngs::OsRng;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct MoneroPrivateKey(#[serde(with = "monero_private_key")] crate::monero::PrivateKey);

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct MoneroAmount(#[serde(with = "monero_amount")] crate::monero::Amount);

    #[test]
    fn serde_monero_private_key_json() {
        let key = MoneroPrivateKey(monero::PrivateKey::from_scalar(
            crate::monero::Scalar::random(&mut OsRng),
        ));
        let encoded = serde_json::to_vec(&key).unwrap();
        let decoded: MoneroPrivateKey = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(key, decoded);
    }

    #[test]
    fn serde_monero_private_key_cbor() {
        let key = MoneroPrivateKey(monero::PrivateKey::from_scalar(
            crate::monero::Scalar::random(&mut OsRng),
        ));
        let encoded = serde_cbor::to_vec(&key).unwrap();
        let decoded: MoneroPrivateKey = serde_cbor::from_slice(&encoded).unwrap();
        assert_eq!(key, decoded);
    }

    #[test]
    fn serde_monero_amount() {
        let amount = MoneroAmount(crate::monero::Amount::from_piconero(1000));
        let encoded = serde_cbor::to_vec(&amount).unwrap();
        let decoded: MoneroAmount = serde_cbor::from_slice(&encoded).unwrap();
        assert_eq!(amount, decoded);
    }

    #[test]
    fn max_conservative_giveable_basic() {
        // Test with balance larger than fee
        let balance = Amount::parse_monero("1.0").unwrap();
        let giveable = balance.max_conservative_giveable();
        let expected = balance.as_piconero() - CONSERVATIVE_MONERO_FEE.as_piconero();
        assert_eq!(giveable.as_piconero(), expected);
    }

    #[test]
    fn max_conservative_giveable_exact_fee() {
        // Test with balance exactly equal to fee
        let balance = CONSERVATIVE_MONERO_FEE;
        let giveable = balance.max_conservative_giveable();
        assert_eq!(giveable, Amount::ZERO);
    }

    #[test]
    fn max_conservative_giveable_less_than_fee() {
        // Test with balance less than fee (should saturate to 0)
        let balance = Amount::from_piconero(CONSERVATIVE_MONERO_FEE.as_piconero() / 2);
        let giveable = balance.max_conservative_giveable();
        assert_eq!(giveable, Amount::ZERO);
    }

    #[test]
    fn max_conservative_giveable_zero_balance() {
        // Test with zero balance
        let balance = Amount::ZERO;
        let giveable = balance.max_conservative_giveable();
        assert_eq!(giveable, Amount::ZERO);
    }

    #[test]
    fn max_conservative_giveable_large_balance() {
        // Test with large balance
        let balance = Amount::parse_monero("100.0").unwrap();
        let giveable = balance.max_conservative_giveable();
        let expected = balance.as_piconero() - CONSERVATIVE_MONERO_FEE.as_piconero();
        assert_eq!(giveable.as_piconero(), expected);

        // Ensure the result makes sense
        assert!(giveable.as_piconero() > 0);
        assert!(giveable < balance);
    }

    #[test]
    fn min_conservative_balance_to_spend_basic() {
        // Test with 1 XMR amount to send
        let amount_to_send = Amount::parse_monero("1.0").unwrap();
        let min_balance = amount_to_send.min_conservative_balance_to_spend();
        let expected = amount_to_send.as_piconero() + CONSERVATIVE_MONERO_FEE.as_piconero();
        assert_eq!(min_balance.as_piconero(), expected);
    }

    #[test]
    fn min_conservative_balance_to_spend_zero() {
        // Test with zero amount to send
        let amount_to_send = Amount::ZERO;
        let min_balance = amount_to_send.min_conservative_balance_to_spend();
        assert_eq!(min_balance, CONSERVATIVE_MONERO_FEE);
    }

    #[test]
    fn min_conservative_balance_to_spend_small_amount() {
        // Test with small amount
        let amount_to_send = Amount::from_piconero(1000);
        let min_balance = amount_to_send.min_conservative_balance_to_spend();
        let expected = 1000 + CONSERVATIVE_MONERO_FEE.as_piconero();
        assert_eq!(min_balance.as_piconero(), expected);
    }

    #[test]
    fn min_conservative_balance_to_spend_large_amount() {
        // Test with large amount
        let amount_to_send = Amount::parse_monero("50.0").unwrap();
        let min_balance = amount_to_send.min_conservative_balance_to_spend();
        let expected = amount_to_send.as_piconero() + CONSERVATIVE_MONERO_FEE.as_piconero();
        assert_eq!(min_balance.as_piconero(), expected);

        // Ensure the result makes sense
        assert!(min_balance > amount_to_send);
        assert!(min_balance > CONSERVATIVE_MONERO_FEE);
    }

    #[test]
    fn conservative_fee_functions_are_inverse() {
        // Test that the functions are somewhat inverse of each other
        let original_balance = Amount::parse_monero("5.0").unwrap();

        // Get max giveable amount
        let max_giveable = original_balance.max_conservative_giveable();

        // Calculate min balance needed to send that amount
        let min_balance_needed = max_giveable.min_conservative_balance_to_spend();

        // The min balance needed should be equal to or slightly more than the original balance
        // (due to the conservative nature of the fee estimation)
        assert!(min_balance_needed >= original_balance);

        // The difference should be at most the conservative fee
        let difference = min_balance_needed.as_piconero() - original_balance.as_piconero();
        assert!(difference <= CONSERVATIVE_MONERO_FEE.as_piconero());
    }

    #[test]
    fn conservative_fee_edge_cases() {
        // Test with maximum possible amount
        let max_amount = Amount::from_piconero(u64::MAX - CONSERVATIVE_MONERO_FEE.as_piconero());
        let giveable = max_amount.max_conservative_giveable();
        assert!(giveable.as_piconero() > 0);

        // Test min balance calculation doesn't overflow
        let large_amount = Amount::from_piconero(u64::MAX / 2);
        let min_balance = large_amount.min_conservative_balance_to_spend();
        assert!(min_balance > large_amount);
    }

    #[test]
    fn labeled_monero_address_percentage_validation() {
        use rust_decimal::Decimal;

        let address = "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a".parse().unwrap();

        // Valid percentages should work (0-1 range)
        assert!(LabeledMoneroAddress::new(address, Decimal::ZERO, "test".to_string()).is_ok());
        assert!(LabeledMoneroAddress::new(address, Decimal::ONE, "test".to_string()).is_ok());
        assert!(LabeledMoneroAddress::new(address, Decimal::new(5, 1), "test".to_string()).is_ok()); // 0.5
        assert!(
            LabeledMoneroAddress::new(address, Decimal::new(9925, 4), "test".to_string()).is_ok()
        ); // 0.9925

        // Invalid percentages should fail
        assert!(
            LabeledMoneroAddress::new(address, Decimal::new(-1, 0), "test".to_string()).is_err()
        );
        assert!(
            LabeledMoneroAddress::new(address, Decimal::new(11, 1), "test".to_string()).is_err()
        ); // 1.1
        assert!(
            LabeledMoneroAddress::new(address, Decimal::new(2, 0), "test".to_string()).is_err()
        ); // 2.0
    }
}
