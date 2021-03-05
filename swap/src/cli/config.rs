use crate::fs::default_data_dir;
use anyhow::{Context, Result};
use config::ConfigError;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tracing::debug;
use url::Url;

pub const DEFAULT_ELECTRUM_HTTP_URL: &str = "https://blockstream.info/testnet/api/";
const DEFAULT_ELECTRUM_RPC_URL: &str = "ssl://electrum.blockstream.info:60002";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Config {
    pub data: Data,
    pub bitcoin: Bitcoin,
}

impl Config {
    pub fn read<D>(config_file: D) -> Result<Self, ConfigError>
    where
        D: AsRef<OsStr>,
    {
        let config_file = Path::new(&config_file);

        let mut config = config::Config::new();
        config.merge(config::File::from(config_file))?;
        config.try_into()
    }

    pub fn testnet() -> Self {
        Self {
            data: Data {
                dir: default_data_dir().expect("computed valid path for data dir"),
            },
            bitcoin: Bitcoin {
                electrum_http_url: DEFAULT_ELECTRUM_HTTP_URL
                    .parse()
                    .expect("default electrum http str is a valid url"),
                electrum_rpc_url: DEFAULT_ELECTRUM_RPC_URL
                    .parse()
                    .expect("default electrum rpc str is a valid url"),
            },
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Data {
    pub dir: PathBuf,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bitcoin {
    pub electrum_http_url: Url,
    pub electrum_rpc_url: Url,
}

#[derive(thiserror::Error, Debug, Clone, Copy)]
#[error("config not initialized")]
pub struct ConfigNotInitialized {}

pub fn read_config(config_path: PathBuf) -> Result<Result<Config, ConfigNotInitialized>> {
    if config_path.exists() {
        debug!(
            "Using config file at default path: {}",
            config_path.display()
        );
    } else {
        return Ok(Err(ConfigNotInitialized {}));
    }

    let file = Config::read(&config_path)
        .with_context(|| format!("Failed to read config file at {}", config_path.display()))?;

    Ok(Ok(file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::ensure_directory_exists;
    use std::fs;
    use std::str::FromStr;
    use tempfile::tempdir;

    pub fn initial_setup(config_path: PathBuf, config: Config) -> Result<()> {
        ensure_directory_exists(config_path.as_path())?;

        let toml = toml::to_string(&config)?;
        fs::write(&config_path, toml)?;

        Ok(())
    }

    #[test]
    fn config_roundtrip() {
        let temp_dir = tempdir().unwrap().path().to_path_buf();
        let config_path = Path::join(&temp_dir, "config.toml");

        let expected = Config {
            data: Data {
                dir: Default::default(),
            },
            bitcoin: Bitcoin {
                electrum_http_url: Url::from_str(DEFAULT_ELECTRUM_HTTP_URL).unwrap(),
                electrum_rpc_url: Url::from_str(DEFAULT_ELECTRUM_RPC_URL).unwrap(),
            },
        };

        initial_setup(config_path.clone(), expected.clone()).unwrap();
        let actual = read_config(config_path).unwrap().unwrap();

        assert_eq!(expected, actual);
    }
}
