pub mod monero_private_key {
    use serde::{de, de::Visitor, Deserializer, Serializer};
    use std::fmt;

    struct BytesVisitor;

    impl<'de> Visitor<'de> for BytesVisitor {
        type Value = monero::PrivateKey;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "a string containing 32 bytes")
        }

        fn visit_bytes<E>(self, s: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if let Ok(key) = monero::PrivateKey::from_slice(s) {
                Ok(key)
            } else {
                Err(de::Error::invalid_length(s.len(), &self))
            }
        }
    }

    pub fn serialize<S>(x: &monero::PrivateKey, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_bytes(x.as_bytes())
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<monero::PrivateKey, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let key = deserializer.deserialize_bytes(BytesVisitor)?;
        Ok(key)
    }
}

pub mod bitcoin_amount {
    use bitcoin::Amount;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(x: &Amount, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_u64(x.as_sat())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Amount, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let sats = u64::deserialize(deserializer)?;
        let amount = Amount::from_sat(sats);

        Ok(amount)
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

#[cfg(test)]
mod tests {
    use super::*;
    use curve25519_dalek::scalar::Scalar;
    use rand::rngs::OsRng;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    pub struct MoneroPrivateKey(#[serde(with = "monero_private_key")] crate::monero::PrivateKey);

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    pub struct BitcoinAmount(#[serde(with = "bitcoin_amount")] ::bitcoin::Amount);

    #[test]
    fn serde_monero_private_key() {
        let key = MoneroPrivateKey(monero::PrivateKey::from_scalar(Scalar::random(&mut OsRng)));
        let encoded = serde_cbor::to_vec(&key).unwrap();
        let decoded: MoneroPrivateKey = serde_cbor::from_slice(&encoded).unwrap();
        assert_eq!(key, decoded);
    }
    #[test]
    fn serde_bitcoin_amount() {
        let amount = BitcoinAmount(::bitcoin::Amount::from_sat(100));
        let encoded = serde_cbor::to_vec(&amount).unwrap();
        let decoded: BitcoinAmount = serde_cbor::from_slice(&encoded).unwrap();
        assert_eq!(amount, decoded);
    }
}
