use monero::{Amount, Network};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Serialize, Deserialize)]
#[serde(remote = "Network")]
#[allow(non_camel_case_types)]
pub enum network {
    Mainnet,
    Stagenet,
    Testnet,
}

pub mod private_key {
    use hex;
    use monero::consensus::{Decodable, Encodable};
    use monero::PrivateKey;
    use serde::de::Visitor;
    use serde::ser::Error;
    use serde::{de, Deserializer, Serializer};
    use std::fmt;
    use std::io::Cursor;

    struct BytesVisitor;

    impl Visitor<'_> for BytesVisitor {
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

pub mod amount {
    use super::*;

    pub fn serialize<S>(x: &Amount, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_u64(x.as_pico())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Amount, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let picos = u64::deserialize(deserializer)?;
        let amount = Amount::from_pico(picos);

        Ok(amount)
    }
}

pub mod address {
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
