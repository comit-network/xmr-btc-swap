use crate::env::{Mainnet, Testnet};
use crate::fs::{ensure_directory_exists, system_config_dir, system_data_dir};
use anyhow::{bail, Context, Result};
use config::ConfigError;
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Input, Select};
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
    electrum_rpc_url: Url,
    monero_daemon_address: Url,
    price_ticker_ws_url: Url,
    bitcoin_confirmation_target: u16,
}

impl GetDefaults for Testnet {
    fn getConfigFileDefaults() -> Result<Defaults> {
        let defaults = Defaults {
            config_path: default_asb_config_dir()?
                .join("testnet")
                .join("config.toml"),
            data_dir: default_asb_data_dir()?.join("testnet"),
            listen_address_tcp: Multiaddr::from_str("/ip4/0.0.0.0/tcp/9939")?,
            electrum_rpc_url: Url::parse("ssl://electrum.blockstream.info:60002")?,
            monero_daemon_address: Url::parse("http://node.sethforprivacy.com:38089")?,
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
            electrum_rpc_url: Url::parse("ssl://blockstream.info:700")?,
            monero_daemon_address: Url::parse("nthpyro.dev:18089")?,
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

mod electrum_urls {
    use serde::de::Unexpected;
    use serde::{de, Deserialize, Deserializer};
    use serde_json::Value;
    use url::Url;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Url>, D::Error>
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
    #[serde(deserialize_with = "electrum_urls::deserialize")]
    pub electrum_rpc_urls: Vec<Url>,
    pub target_block: u16,
    pub finality_confirmations: Option<u32>,
    #[serde(with = "crate::bitcoin::network")]
    pub network: bitcoin::Network,
    #[serde(default = "default_use_mempool_space_fee_estimation")]
    pub use_mempool_space_fee_estimation: bool,
}

fn default_use_mempool_space_fee_estimation() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Monero {
    pub daemon_url: Url,
    pub finality_confirmations: Option<u64>,
    #[serde(with = "crate::monero::network")]
    pub network: monero::Network,
    #[serde(default = "default_monero_node_pool")]
    pub monero_node_pool: bool,
}

fn default_monero_node_pool() -> bool {
    false
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TorConf {
    pub register_hidden_service: bool,
    pub hidden_service_num_intro_points: u8,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Maker {
    #[serde(with = "::bitcoin::amount::serde::as_btc")]
    pub min_buy_btc: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_btc")]
    pub max_buy_btc: bitcoin::Amount,
    pub ask_spread: Decimal,
    pub price_ticker_ws_url: Url,
    #[serde(default, with = "crate::bitcoin::address_serde::option")]
    pub external_bitcoin_redeem_address: Option<bitcoin::Address>,
}

impl Default for TorConf {
    fn default() -> Self {
        Self {
            register_hidden_service: true,
            hidden_service_num_intro_points: 5,
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
        .default( format!("{}", defaults.listen_address_tcp))
        .interact_text()?;
    let listen_addresses = listen_addresses
        .split(',')
        .map(|str| str.parse())
        .collect::<Result<Vec<Multiaddr>, _>>()?;

    let mut electrum_rpc_urls = Vec::new();
    let mut electrum_number = 1;
    let mut electrum_done = false;

    println!(
        "You can configure multiple Electrum servers for redundancy. At least one is required."
    );

    // Ask for the first electrum URL with a default
    let electrum_rpc_url = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter first Electrum RPC URL or hit return to use default")
        .default(defaults.electrum_rpc_url)
        .interact_text()?;
    electrum_rpc_urls.push(electrum_rpc_url);
    electrum_number += 1;

    // Ask for additional electrum URLs
    while !electrum_done {
        let prompt = format!(
            "Enter additional Electrum RPC URL ({electrum_number}). Or just hit Enter to continue."
        );
        let electrum_url = Input::<Url>::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .allow_empty(true)
            .interact_text()?;
        if electrum_url.as_str().is_empty() {
            electrum_done = true;
        } else if electrum_rpc_urls.contains(&electrum_url) {
            println!("That Electrum URL is already in the list.");
        } else {
            electrum_rpc_urls.push(electrum_url);
            electrum_number += 1;
        }
    }

    let monero_daemon_url = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter Monero daemon url or hit enter to use default")
        .default(defaults.monero_daemon_address)
        .interact_text()?;

    let register_hidden_service = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Do you want a Tor hidden service to be created? This will allow you to run from behind a firewall without opening ports, and hide your IP address. You do not have to run a Tor daemon yourself. We recommend this for most users. (y/n)")
        .items(&["yes", "no"])
        .default(0)
        .interact()?
        == 0;

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
            electrum_rpc_urls,
            target_block,
            finality_confirmations: None,
            network: bitcoin_network,
            use_mempool_space_fee_estimation: true,
        },
        monero: Monero {
            daemon_url: monero_daemon_url,
            finality_confirmations: None,
            network: monero_network,
            monero_node_pool: false,
        },
        tor: TorConf {
            register_hidden_service,
            ..Default::default()
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
                electrum_rpc_urls: vec![defaults.electrum_rpc_url],
                target_block: defaults.bitcoin_confirmation_target,
                finality_confirmations: None,
                network: bitcoin::Network::Testnet,
                use_mempool_space_fee_estimation: true,
            },
            network: Network {
                listen: vec![defaults.listen_address_tcp],
                rendezvous_point: vec![],
                external_addresses: vec![],
            },
            monero: Monero {
                daemon_url: defaults.monero_daemon_address,
                finality_confirmations: None,
                network: monero::Network::Stagenet,
                monero_node_pool: false,
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
                electrum_rpc_urls: vec![defaults.electrum_rpc_url],
                target_block: defaults.bitcoin_confirmation_target,
                finality_confirmations: None,
                network: bitcoin::Network::Bitcoin,
                use_mempool_space_fee_estimation: true,
            },
            network: Network {
                listen: vec![defaults.listen_address_tcp],
                rendezvous_point: vec![],
                external_addresses: vec![],
            },
            monero: Monero {
                daemon_url: defaults.monero_daemon_address,
                finality_confirmations: None,
                network: monero::Network::Mainnet,
                monero_node_pool: false,
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
                electrum_rpc_urls: vec![defaults.electrum_rpc_url],
                target_block: defaults.bitcoin_confirmation_target,
                finality_confirmations: None,
                network: bitcoin::Network::Bitcoin,
                use_mempool_space_fee_estimation: true,
            },
            network: Network {
                listen,
                rendezvous_point: vec![],
                external_addresses,
            },
            monero: Monero {
                daemon_url: defaults.monero_daemon_address,
                finality_confirmations: None,
                network: monero::Network::Mainnet,
                monero_node_pool: false,
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
