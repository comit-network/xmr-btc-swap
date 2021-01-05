pub mod monero_private_key {
    use monero::{
        consensus::{Decodable, Encodable},
        PrivateKey,
    };
    use serde::{de, de::Visitor, ser::Error, Deserializer, Serializer};
    use std::{fmt, io::Cursor};

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
    }

    pub fn serialize<S>(x: &PrivateKey, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut bytes = Cursor::new(vec![]);
        x.consensus_encode(&mut bytes)
            .map_err(|err| S::Error::custom(format!("{:?}", err)))?;
        s.serialize_bytes(bytes.into_inner().as_ref())
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<PrivateKey, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let key = deserializer.deserialize_bytes(BytesVisitor)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    pub struct MoneroPrivateKey(#[serde(with = "monero_private_key")] crate::monero::PrivateKey);

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    pub struct MoneroAmount(#[serde(with = "monero_amount")] crate::monero::Amount);

    #[test]
    fn serde_monero_private_key() {
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
}
