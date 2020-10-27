use anyhow::Result;
use libp2p::multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use url::Url;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub bitcoind_url: Url,
    pub listen_addr: Multiaddr,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let contents = fs::read_to_string(path).expect("Something went wrong reading the file");
        let file: Config = toml::from_str(contents.as_str())?;
        Ok(file)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bitcoind_url: Url::parse("https://127.0.0.1:8332")
                .expect("Failed to generate default config"),
            listen_addr: "/ip4/127.0.0.1/tcp/9876"
                .parse()
                .expect("Failed to generate default config"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Monero {
    pub ip: Option<String>,
    pub port: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_default_config() {
        let _ = Config::default();
    }
}
