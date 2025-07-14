pub mod multiaddresses {
    use libp2p::Multiaddr;
    use serde::de::Unexpected;
    use serde::{de, Deserialize, Deserializer};
    use serde_json::Value;
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Multiaddr>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = Value::deserialize(deserializer)?;
        match s {
            Value::String(s) => {
                let list: Result<Vec<_>, _> = s
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.trim().parse().map_err(de::Error::custom))
                    .collect();
                Ok(list?)
            }
            Value::Array(a) => {
                let list: Result<Vec<_>, _> = a
                    .iter()
                    .map(|v| {
                        if let Value::String(s) = v {
                            s.trim().parse().map_err(de::Error::custom)
                            } else {
                                Err(de::Error::custom("expected a string"))
                            }
                        })
                        .collect();
                    Ok(list?)
                }
                value => Err(de::Error::invalid_type(
                    Unexpected::Other(&value.to_string()),
                    &"a string or array",
                )),
            }
    }
}