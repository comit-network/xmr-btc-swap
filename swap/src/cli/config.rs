use crate::fs::{default_data_dir, ensure_directory_exists};
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

const DEFAULT_ELECTRUM_HTTP_URL: &str = "https://blockstream.info/testnet/api/";
const DEFAULT_ELECTRUM_RPC_URL: &str = "ssl://electrum.blockstream.info:60002";
const DEFAULT_MONERO_WALLET_RPC_TESTNET_URL: &str = "http://127.0.0.1:38083/json_rpc";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Config {
    pub data: Data,
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
pub struct Data {
    pub dir: PathBuf,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bitcoin {
    pub electrum_http_url: Url,
    pub electrum_rpc_url: Url,
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
    let data_dir = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter data directory for the swap CLI or hit return to use default")
        .default(
            default_data_dir()
                .context("No default data dir value for this system")?
                .to_str()
                .context("Unsupported characters in default path")?
                .to_string(),
        )
        .interact_text()?;
    let data_dir = data_dir.as_str().parse()?;

    let electrum_http_url: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter Electrum HTTP URL or hit return to use default")
        .default(DEFAULT_ELECTRUM_HTTP_URL.to_owned())
        .interact_text()?;
    let electrum_http_url = Url::parse(electrum_http_url.as_str())?;

    let electrum_rpc_url: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter Electrum RPC URL or hit return to use default")
        .default(DEFAULT_ELECTRUM_RPC_URL.to_owned())
        .interact_text()?;
    let electrum_rpc_url = Url::parse(electrum_rpc_url.as_str())?;

    let monero_wallet_rpc_url = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter Monero Wallet RPC URL or hit enter to use default")
        .default(DEFAULT_MONERO_WALLET_RPC_TESTNET_URL.to_owned())
        .interact_text()?;
    let monero_wallet_rpc_url = monero_wallet_rpc_url.as_str().parse()?;
    println!();

    Ok(Config {
        data: Data { dir: data_dir },
        bitcoin: Bitcoin {
            electrum_http_url,
            electrum_rpc_url,
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
            data: Data {
                dir: Default::default(),
            },
            bitcoin: Bitcoin {
                electrum_http_url: Url::from_str(DEFAULT_ELECTRUM_HTTP_URL).unwrap(),
                electrum_rpc_url: Url::from_str(DEFAULT_ELECTRUM_RPC_URL).unwrap(),
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
