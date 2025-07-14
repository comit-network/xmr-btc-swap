use serde::{Deserialize, Serialize};
use bitcoin::{Network};

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