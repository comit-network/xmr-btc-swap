use crate::fs::ensure_directory_exists;
use anyhow::{Context, Result};
use config::ConfigError;
use dialoguer::{theme::ColorfulTheme, Input};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};
use tracing::info;
use url::Url;

const DEFAULT_BITCOIND_TESTNET_URL: &str = "http://127.0.0.1:18332";
const DEFAULT_MONERO_WALLET_RPC_TESTNET_URL: &str = "http://127.0.0.1:38083/json_rpc";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Config {
    pub bitcoin: Bitcoin,
    pub monero: Monero,
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
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bitcoin {
    pub bitcoind_url: Url,
    pub wallet_name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Monero {
    pub wallet_rpc_url: Url,
}

#[derive(thiserror::Error, Debug, Clone, Copy)]
#[error("config not initialized")]
pub struct ConfigNotInitialized {}

pub fn read_config(config_path: PathBuf) -> Result<Result<Config, ConfigNotInitialized>> {
    if config_path.exists() {
        info!(
            "Using config file at default path: {}",
            config_path.display()
        );
    } else {
        return Ok(Err(ConfigNotInitialized {}));
    }

    let file = Config::read(&config_path)
        .with_context(|| format!("failed to read config file {}", config_path.display()))?;

    Ok(Ok(file))
}

pub fn initial_setup<F>(config_path: PathBuf, config_file: F) -> Result<()>
where
    F: Fn() -> Result<Config>,
{
    info!("Config file not found, running initial setup...");
    ensure_directory_exists(config_path.as_path())?;
    let initial_config = config_file()?;

    let toml = toml::to_string(&initial_config)?;
    fs::write(&config_path, toml)?;

    info!(
        "Initial setup complete, config file created at {} ",
        config_path.as_path().display()
    );
    Ok(())
}

pub fn query_user_for_initial_testnet_config() -> Result<Config> {
    println!();
    let bitcoind_url: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter Bitcoind URL (including username and password if applicable) or hit return to use default")
        .default(DEFAULT_BITCOIND_TESTNET_URL.to_owned())
        .interact_text()?;
    let bitcoind_url = Url::parse(bitcoind_url.as_str())?;

    let bitcoin_wallet_name: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter Bitcoind wallet name")
        .interact_text()?;

    let monero_wallet_rpc_url: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter Monero Wallet RPC URL or hit enter to use default")
        .default(DEFAULT_MONERO_WALLET_RPC_TESTNET_URL.to_owned())
        .interact_text()?;
    let monero_wallet_rpc_url = Url::parse(monero_wallet_rpc_url.as_str())?;
    println!();

    Ok(Config {
        bitcoin: Bitcoin {
            bitcoind_url,
            wallet_name: bitcoin_wallet_name,
        },
        monero: Monero {
            wallet_rpc_url: monero_wallet_rpc_url,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use tempfile::tempdir;

    #[test]
    fn config_roundtrip() {
        let temp_dir = tempdir().unwrap().path().to_path_buf();
        let config_path = Path::join(&temp_dir, "config.toml");

        let expected = Config {
            bitcoin: Bitcoin {
                bitcoind_url: Url::from_str("http://127.0.0.1:18332").unwrap(),
                wallet_name: "alice".to_string(),
            },
            monero: Monero {
                wallet_rpc_url: Url::from_str("http://127.0.0.1:38083/json_rpc").unwrap(),
            },
        };

        initial_setup(config_path.clone(), || Ok(expected.clone())).unwrap();
        let actual = read_config(config_path).unwrap().unwrap();

        assert_eq!(expected, actual);
    }
}
