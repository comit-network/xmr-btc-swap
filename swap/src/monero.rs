use serde::{de::Error, Deserialize, Deserializer, Serializer};

use xmr_btc::monero::Amount;

pub mod amount_serde {
    use super::*;
    use std::str::FromStr;

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
