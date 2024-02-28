use crate::env::{Mainnet, Testnet};
use crate::fs::{ensure_directory_exists, system_config_dir, system_data_dir};
use crate::tor::{DEFAULT_CONTROL_PORT, DEFAULT_SOCKS5_PORT};
use anyhow::{bail, Context, Result};
use config::ConfigError;
use dialoguer::theme::ColorfulTheme;
use dialoguer::Input;
use libp2p::core::Multiaddr;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use url::Url;

pub trait GetDefaults {
    fn getConfigFileDefaults() -> Result<Defaults>;
}

pub struct Defaults {
    pub config_path: PathBuf,
    data_dir: PathBuf,
    listen_address_tcp: Multiaddr,
    listen_address_ws: Multiaddr,
    electrum_rpc_url: Url,
    monero_wallet_rpc_url: Url,
    price_ticker_ws_url: Url,
    bitcoin_confirmation_target: usize,
}

impl GetDefaults for Testnet {
    fn getConfigFileDefaults() -> Result<Defaults> {
        let defaults = Defaults {
            config_path: default_asb_config_dir()?
                .join("testnet")
                .join("config.toml"),
            data_dir: default_asb_data_dir()?.join("testnet"),
            listen_address_tcp: Multiaddr::from_str("/ip4/0.0.0.0/tcp/9939")?,
            listen_address_ws: Multiaddr::from_str("/ip4/0.0.0.0/tcp/9940/ws")?,
            electrum_rpc_url: Url::parse("ssl://electrum.blockstream.info:60002")?,
            monero_wallet_rpc_url: Url::parse("http://127.0.0.1:38083/json_rpc")?,
            price_ticker_ws_url: Url::parse("wss://ws.kraken.com")?,
            bitcoin_confirmation_target: 1,
        };

        Ok(defaults)
    }
}

impl GetDefaults for Mainnet {
    fn getConfigFileDefaults() -> Result<Defaults> {
        let defaults = Defaults {
            config_path: default_asb_config_dir()?
                .join("mainnet")
                .join("config.toml"),
            data_dir: default_asb_data_dir()?.join("mainnet"),
            listen_address_tcp: Multiaddr::from_str("/ip4/0.0.0.0/tcp/9939")?,
            listen_address_ws: Multiaddr::from_str("/ip4/0.0.0.0/tcp/9940/ws")?,
            electrum_rpc_url: Url::parse("ssl://blockstream.info:700")?,
            monero_wallet_rpc_url: Url::parse("http://127.0.0.1:18083/json_rpc")?,
            price_ticker_ws_url: Url::parse("wss://ws.kraken.com")?,
            bitcoin_confirmation_target: 3,
        };

        Ok(defaults)
    }
}

fn default_asb_config_dir() -> Result<PathBuf> {
    system_config_dir()
        .map(|dir| Path::join(&dir, "asb"))
        .context("Could not generate default config file path")
}

fn default_asb_data_dir() -> Result<PathBuf> {
    system_data_dir()
        .map(|dir| Path::join(&dir, "asb"))
        .context("Could not generate default config file path")
}

const DEFAULT_MIN_BUY_AMOUNT: f64 = 0.002f64;
const DEFAULT_MAX_BUY_AMOUNT: f64 = 0.02f64;
const DEFAULT_SPREAD: f64 = 0.02f64;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub data: Data,
    pub network: Network,
    pub bitcoin: Bitcoin,
    pub monero: Monero,
    pub tor: TorConf,
    pub maker: Maker,
}

impl Config {
    pub fn read<D>(config_file: D) -> Result<Self, ConfigError>
    where
        D: AsRef<OsStr>,
    {
        let config_file = Path::new(&config_file);

        let config = config::Config::builder()
            .add_source(config::File::from(config_file))
            .add_source(
                config::Environment::with_prefix("ASB")
                    .separator("__")
                    .list_separator(","),
            )
            .build()?;

        config.try_into()
    }
}

impl TryFrom<config::Config> for Config {
    type Error = config::ConfigError;

    fn try_from(value: config::Config) -> Result<Self, Self::Error> {
        value.try_deserialize()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Data {
    pub dir: PathBuf,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    #[serde(deserialize_with = "addr_list::deserialize")]
    pub listen: Vec<Multiaddr>,
    #[serde(default, deserialize_with = "addr_list::deserialize")]
    pub rendezvous_point: Vec<Multiaddr>,
    #[serde(default, deserialize_with = "addr_list::deserialize")]
    pub external_addresses: Vec<Multiaddr>,
}

mod addr_list {
    use libp2p::Multiaddr;
    use serde::de::Unexpected;
    use serde::{de, Deserialize, Deserializer};
    use serde_json::Value;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Multiaddr>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = Value::deserialize(deserializer)?;
        return match s {
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
        };
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bitcoin {
    pub electrum_rpc_url: Url,
    pub target_block: usize,
    pub finality_confirmations: Option<u32>,
    #[serde(with = "crate::bitcoin::network")]
    pub network: bitcoin::Network,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Monero {
    pub wallet_rpc_url: Url,
    pub finality_confirmations: Option<u64>,
    #[serde(with = "crate::monero::network")]
    pub network: monero::Network,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TorConf {
    pub control_port: u16,
    pub socks5_port: u16,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Maker {
    #[serde(with = "::bitcoin::util::amount::serde::as_btc")]
    pub min_buy_btc: bitcoin::Amount,
    #[serde(with = "::bitcoin::util::amount::serde::as_btc")]
    pub max_buy_btc: bitcoin::Amount,
    pub ask_spread: Decimal,
    pub price_ticker_ws_url: Url,
    pub external_bitcoin_redeem_address: Option<bitcoin::Address>,
}

impl Default for TorConf {
    fn default() -> Self {
        Self {
            control_port: DEFAULT_CONTROL_PORT,
            socks5_port: DEFAULT_SOCKS5_PORT,
        }
    }
}

#[derive(thiserror::Error, Debug, Clone, Copy)]
#[error("config not initialized")]
pub struct ConfigNotInitialized;

pub fn read_config(config_path: PathBuf) -> Result<Result<Config, ConfigNotInitialized>> {
    if config_path.exists() {
        tracing::info!(
            path = %config_path.display(),
            "Reading config file",
        );
    } else {
        return Ok(Err(ConfigNotInitialized {}));
    }

    let file = Config::read(&config_path)
        .with_context(|| format!("Failed to read config file at {}", config_path.display()))?;

    Ok(Ok(file))
}

pub fn initial_setup(config_path: PathBuf, config: Config) -> Result<()> {
    let toml = toml::to_string(&config)?;

    ensure_directory_exists(config_path.as_path())?;
    fs::write(&config_path, toml)?;

    tracing::info!(
        path = %config_path.as_path().display(),
        "Initial setup complete, config file created",
    );
    Ok(())
}

pub fn query_user_for_initial_config(testnet: bool) -> Result<Config> {
    let (bitcoin_network, monero_network, defaults) = if testnet {
        tracing::info!("Running initial setup for testnet");

        let bitcoin_network = bitcoin::Network::Testnet;
        let monero_network = monero::Network::Stagenet;
        let defaults = Testnet::getConfigFileDefaults()?;

        (bitcoin_network, monero_network, defaults)
    } else {
        tracing::info!("Running initial setup for mainnet");
        let bitcoin_network = bitcoin::Network::Bitcoin;
        let monero_network = monero::Network::Mainnet;
        let defaults = Mainnet::getConfigFileDefaults()?;

        (bitcoin_network, monero_network, defaults)
    };

    println!();
    let data_dir = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter data directory for asb or hit return to use default")
        .default(
            defaults
                .data_dir
                .to_str()
                .context("Unsupported characters in default path")?
                .to_string(),
        )
        .interact_text()?;
    let data_dir = data_dir.as_str().parse()?;

    let target_block = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("How fast should your Bitcoin transactions be confirmed? Your transaction fee will be calculated based on this target. Hit return to use default")
        .default(defaults.bitcoin_confirmation_target)
        .interact_text()?;

    let listen_addresses = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter multiaddresses (comma separated) on which asb should list for peer-to-peer communications or hit return to use default")
        .default( format!("{},{}", defaults.listen_address_tcp, defaults.listen_address_ws))
        .interact_text()?;
    let listen_addresses = listen_addresses
        .split(',')
        .map(|str| str.parse())
        .collect::<Result<Vec<Multiaddr>, _>>()?;

    let electrum_rpc_url = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter Electrum RPC URL or hit return to use default")
        .default(defaults.electrum_rpc_url)
        .interact_text()?;

    let monero_wallet_rpc_url = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter Monero Wallet RPC URL or hit enter to use default")
        .default(defaults.monero_wallet_rpc_url)
        .interact_text()?;

    let tor_control_port = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter Tor control port or hit enter to use default. If Tor is not running on your machine, no hidden service will be created.")
        .default(DEFAULT_CONTROL_PORT.to_owned())
        .interact_text()?;

    let tor_socks5_port = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter Tor socks5 port or hit enter to use default")
        .default(DEFAULT_SOCKS5_PORT.to_owned())
        .interact_text()?;

    let min_buy = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter minimum Bitcoin amount you are willing to accept per swap or hit enter to use default.")
        .default(DEFAULT_MIN_BUY_AMOUNT)
        .interact_text()?;
    let min_buy = bitcoin::Amount::from_btc(min_buy)?;

    let max_buy = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter maximum Bitcoin amount you are willing to accept per swap or hit enter to use default.")
        .default(DEFAULT_MAX_BUY_AMOUNT)
        .interact_text()?;
    let max_buy = bitcoin::Amount::from_btc(max_buy)?;

    let ask_spread = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter spread (in percent; value between 0.x and 1.0) to be used on top of the market rate or hit enter to use default.")
        .default(DEFAULT_SPREAD)
        .interact_text()?;
    if !(0.0..=1.0).contains(&ask_spread) {
        bail!(format!("Invalid spread {}. For the spread value floating point number in interval [0..1] are allowed.", ask_spread))
    }
    let ask_spread = Decimal::from_f64(ask_spread).context("Unable to parse spread")?;

    let mut number = 1;
    let mut done = false;
    let mut rendezvous_points = Vec::new();
    println!("ASB can register with multiple rendezvous nodes for discoverability. This can also be edited in the config file later.");
    while !done {
        let prompt = format!(
            "Enter the address for rendezvous node ({number}). Or just hit Enter to continue."
        );
        let rendezvous_addr = Input::<Multiaddr>::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .allow_empty(true)
            .interact_text()?;
        if rendezvous_addr.is_empty() {
            done = true;
        } else if rendezvous_points.contains(&rendezvous_addr) {
            println!("That rendezvous address is already in the list.");
        } else {
            rendezvous_points.push(rendezvous_addr);
            number += 1;
        }
    }

    println!();

    Ok(Config {
        data: Data { dir: data_dir },
        network: Network {
            listen: listen_addresses,
            rendezvous_point: rendezvous_points, // keeping the singular key name for backcompat
            external_addresses: vec![],
        },
        bitcoin: Bitcoin {
            electrum_rpc_url,
            target_block,
            finality_confirmations: None,
            network: bitcoin_network,
        },
        monero: Monero {
            wallet_rpc_url: monero_wallet_rpc_url,
            finality_confirmations: None,
            network: monero_network,
        },
        tor: TorConf {
            control_port: tor_control_port,
            socks5_port: tor_socks5_port,
        },
        maker: Maker {
            min_buy_btc: min_buy,
            max_buy_btc: max_buy,
            ask_spread,
            price_ticker_ws_url: defaults.price_ticker_ws_url,
            external_bitcoin_redeem_address: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    // these tests are run serially since env vars affect the whole process
    #[test]
    #[serial]
    fn config_roundtrip_testnet() {
        let temp_dir = tempdir().unwrap().path().to_path_buf();
        let config_path = Path::join(&temp_dir, "config.toml");

        let defaults = Testnet::getConfigFileDefaults().unwrap();

        let expected = Config {
            data: Data {
                dir: Default::default(),
            },
            bitcoin: Bitcoin {
                electrum_rpc_url: defaults.electrum_rpc_url,
                target_block: defaults.bitcoin_confirmation_target,
                finality_confirmations: None,
                network: bitcoin::Network::Testnet,
            },
            network: Network {
                listen: vec![defaults.listen_address_tcp, defaults.listen_address_ws],
                rendezvous_point: vec![],
                external_addresses: vec![],
            },
            monero: Monero {
                wallet_rpc_url: defaults.monero_wallet_rpc_url,
                finality_confirmations: None,
                network: monero::Network::Stagenet,
            },
            tor: Default::default(),
            maker: Maker {
                min_buy_btc: bitcoin::Amount::from_btc(DEFAULT_MIN_BUY_AMOUNT).unwrap(),
                max_buy_btc: bitcoin::Amount::from_btc(DEFAULT_MAX_BUY_AMOUNT).unwrap(),
                ask_spread: Decimal::from_f64(DEFAULT_SPREAD).unwrap(),
                price_ticker_ws_url: defaults.price_ticker_ws_url,
                external_bitcoin_redeem_address: None,
            },
        };

        initial_setup(config_path.clone(), expected.clone()).unwrap();
        let actual = read_config(config_path).unwrap().unwrap();

        assert_eq!(expected, actual);
    }

    #[test]
    #[serial]
    fn config_roundtrip_mainnet() {
        let temp_dir = tempdir().unwrap().path().to_path_buf();
        let config_path = Path::join(&temp_dir, "config.toml");

        let defaults = Mainnet::getConfigFileDefaults().unwrap();

        let expected = Config {
            data: Data {
                dir: Default::default(),
            },
            bitcoin: Bitcoin {
                electrum_rpc_url: defaults.electrum_rpc_url,
                target_block: defaults.bitcoin_confirmation_target,
                finality_confirmations: None,
                network: bitcoin::Network::Bitcoin,
            },
            network: Network {
                listen: vec![defaults.listen_address_tcp, defaults.listen_address_ws],
                rendezvous_point: vec![],
                external_addresses: vec![],
            },
            monero: Monero {
                wallet_rpc_url: defaults.monero_wallet_rpc_url,
                finality_confirmations: None,
                network: monero::Network::Mainnet,
            },
            tor: Default::default(),
            maker: Maker {
                min_buy_btc: bitcoin::Amount::from_btc(DEFAULT_MIN_BUY_AMOUNT).unwrap(),
                max_buy_btc: bitcoin::Amount::from_btc(DEFAULT_MAX_BUY_AMOUNT).unwrap(),
                ask_spread: Decimal::from_f64(DEFAULT_SPREAD).unwrap(),
                price_ticker_ws_url: defaults.price_ticker_ws_url,
                external_bitcoin_redeem_address: None,
            },
        };

        initial_setup(config_path.clone(), expected.clone()).unwrap();
        let actual = read_config(config_path).unwrap().unwrap();

        assert_eq!(expected, actual);
    }

    #[test]
    #[serial]
    fn env_override() {
        let temp_dir = tempfile::tempdir().unwrap().path().to_path_buf();
        let config_path = Path::join(&temp_dir, "config.toml");

        let defaults = Mainnet::getConfigFileDefaults().unwrap();

        let dir = PathBuf::from("/tmp/dir");
        std::env::set_var("ASB__DATA__DIR", dir.clone());
        let addr1 = "/dns4/example.com/tcp/9939";
        let addr2 = "/ip4/1.2.3.4/tcp/9940";
        let external_addresses = vec![addr1.parse().unwrap(), addr2.parse().unwrap()];
        let listen = external_addresses.clone();
        std::env::set_var(
            "ASB__NETWORK__EXTERNAL_ADDRESSES",
            format!("{},{}", addr1, addr2),
        );
        std::env::set_var("ASB__NETWORK__LISTEN", format!("{},{}", addr1, addr2));

        let expected = Config {
            data: Data { dir },
            bitcoin: Bitcoin {
                electrum_rpc_url: defaults.electrum_rpc_url,
                target_block: defaults.bitcoin_confirmation_target,
                finality_confirmations: None,
                network: bitcoin::Network::Bitcoin,
            },
            network: Network {
                listen,
                rendezvous_point: vec![],
                external_addresses,
            },
            monero: Monero {
                wallet_rpc_url: defaults.monero_wallet_rpc_url,
                finality_confirmations: None,
                network: monero::Network::Mainnet,
            },
            tor: Default::default(),
            maker: Maker {
                min_buy_btc: bitcoin::Amount::from_btc(DEFAULT_MIN_BUY_AMOUNT).unwrap(),
                max_buy_btc: bitcoin::Amount::from_btc(DEFAULT_MAX_BUY_AMOUNT).unwrap(),
                ask_spread: Decimal::from_f64(DEFAULT_SPREAD).unwrap(),
                price_ticker_ws_url: defaults.price_ticker_ws_url,
                external_bitcoin_redeem_address: None,
            },
        };

        initial_setup(config_path.clone(), expected.clone()).unwrap();
        let actual = read_config(config_path).unwrap().unwrap();

        assert_eq!(expected, actual);
        std::env::remove_var("ASB__DATA__DIR");
        std::env::remove_var("ASB__NETWORK__EXTERNAL_ADDRESSES");
        std::env::remove_var("ASB__NETWORK__LISTEN");
    }
}
