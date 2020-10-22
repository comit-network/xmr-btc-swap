pub mod ecdsa_fun_signature {
    use serde::{de, de::Visitor, Deserializer, Serializer};
    use std::{convert::TryFrom, fmt};

    struct Bytes64Visitor;

    impl<'de> Visitor<'de> for Bytes64Visitor {
        type Value = ecdsa_fun::Signature;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "a string containing 64 bytes")
        }

        fn visit_bytes<E>(self, s: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if let Ok(value) = <[u8; 64]>::try_from(s) {
                let sig = ecdsa_fun::Signature::from_bytes(value)
                    .expect("bytes represent an integer greater than or equal to the curve order");
                Ok(sig)
            } else {
                Err(de::Error::invalid_length(s.len(), &self))
            }
        }
    }

    pub fn serialize<S>(x: &ecdsa_fun::Signature, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_bytes(&x.to_bytes())
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<ecdsa_fun::Signature, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let sig = deserializer.deserialize_bytes(Bytes64Visitor)?;
        Ok(sig)
    }
}

pub mod cross_curve_dleq_scalar {
    use serde::{de, de::Visitor, Deserializer, Serializer};
    use std::{convert::TryFrom, fmt};

    struct Bytes32Visitor;

    impl<'de> Visitor<'de> for Bytes32Visitor {
        type Value = cross_curve_dleq::Scalar;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "a string containing 32 bytes")
        }

        fn visit_bytes<E>(self, s: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if let Ok(value) = <[u8; 32]>::try_from(s) {
                Ok(cross_curve_dleq::Scalar::from(value))
            } else {
                Err(de::Error::invalid_length(s.len(), &self))
            }
        }
    }

    pub fn serialize<S>(x: &cross_curve_dleq::Scalar, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialise as ed25519 because the inner bytes are private
        // TODO: Open PR in cross_curve_dleq to allow accessing the inner bytes
        s.serialize_bytes(&x.into_ed25519().to_bytes())
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<cross_curve_dleq::Scalar, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let dleq = deserializer.deserialize_bytes(Bytes32Visitor)?;
        Ok(dleq)
    }
}

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
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &bitcoin::Amount, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(value.as_sat())
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<bitcoin::Amount, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        let amount = bitcoin::Amount::from_sat(value);

        Ok(amount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::bitcoin::SigHash;
    use curve25519_dalek::scalar::Scalar;
    use rand::rngs::OsRng;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    pub struct CrossCurveDleqScalar(
        #[serde(with = "cross_curve_dleq_scalar")] cross_curve_dleq::Scalar,
    );

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    pub struct ECDSAFunSignature(#[serde(with = "ecdsa_fun_signature")] ecdsa_fun::Signature);

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    pub struct MoneroPrivateKey(#[serde(with = "monero_private_key")] crate::monero::PrivateKey);

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    pub struct BitcoinAmount(#[serde(with = "bitcoin_amount")] ::bitcoin::Amount);

    #[test]
    fn serde_cross_curv_dleq_scalar() {
        let scalar = CrossCurveDleqScalar(cross_curve_dleq::Scalar::random(&mut OsRng));
        let encoded = serde_cbor::to_vec(&scalar).unwrap();
        let decoded: CrossCurveDleqScalar = serde_cbor::from_slice(&encoded).unwrap();
        assert_eq!(scalar, decoded);
    }

    #[test]
    fn serde_ecdsa_fun_sig() {
        let secret_key = crate::bitcoin::SecretKey::new_random(&mut OsRng);
        let sig = ECDSAFunSignature(secret_key.sign(SigHash::default()));
        let encoded = serde_cbor::to_vec(&sig).unwrap();
        let decoded: ECDSAFunSignature = serde_cbor::from_slice(&encoded).unwrap();
        assert_eq!(sig, decoded);
    }

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
