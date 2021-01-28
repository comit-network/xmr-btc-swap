use crate::fs::{default_config_path, ensure_directory_exists};
use anyhow::{Context, Result};
use config::{Config, ConfigError};
use dialoguer::{theme::ColorfulTheme, Input};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};
use tracing::info;
use url::Url;

pub mod seed;

const DEFAULT_BITCOIND_TESTNET_URL: &str = "http://127.0.0.1:18332";
const DEFAULT_MONERO_WALLET_RPC_TESTNET_URL: &str = "http://127.0.0.1:38083/json_rpc";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub bitcoin: Bitcoin,
    pub monero: Monero,
}

impl File {
    pub fn read<D>(config_file: D) -> Result<Self, ConfigError>
    where
        D: AsRef<OsStr>,
    {
        let config_file = Path::new(&config_file);

        let mut config = Config::new();
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

pub fn read_config() -> anyhow::Result<File> {
    let default_path = default_config_path()?;

    if default_path.exists() {
        info!(
            "Using config file at default path: {}",
            default_path.display()
        );
    } else {
        initial_setup(default_path.clone())?;
    }

    File::read(&default_path)
        .with_context(|| format!("failed to read config file {}", default_path.display()))
}

fn initial_setup(config_path: PathBuf) -> Result<()> {
    info!("Config file not found, running initial setup...");
    ensure_directory_exists(config_path.as_path())?;
    let initial_config = query_user_for_initial_testnet_config()?;

    let toml = toml::to_string(&initial_config)?;
    fs::write(config_path.clone(), toml)?;

    info!(
        "Initial setup complete, config file created at {} ",
        config_path.as_path().display()
    );
    Ok(())
}

fn query_user_for_initial_testnet_config() -> Result<File> {
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

    Ok(File {
        bitcoin: Bitcoin {
            bitcoind_url,
            wallet_name: bitcoin_wallet_name,
        },
        monero: Monero {
            wallet_rpc_url: monero_wallet_rpc_url,
        },
    })
}
