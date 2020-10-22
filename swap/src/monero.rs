//! Monero stuff, for now just serde.

// This has to be in a sub-module to use with serde derive.
pub mod amount_serde {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};
    use std::str::FromStr;
    use xmr_btc::monero::Amount;

    pub fn serialize<S>(value: &Amount, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.as_piconero().to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Amount, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let value =
            u64::from_str(value.as_str()).map_err(<D as Deserializer<'de>>::Error::custom)?;
        let amount = Amount::from_piconero(value);

        Ok(amount)
    }
}
