use crate::asb;
use crate::bitcoin::{CancelTimelock, PunishTimelock};
use serde::Serialize;
use std::cmp::max;
use std::time::Duration;
use time::ext::NumericalStdDuration;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub struct Config {
    pub bitcoin_lock_mempool_timeout: Duration,
    pub bitcoin_lock_confirmed_timeout: Duration,
    pub bitcoin_finality_confirmations: u32,
    pub bitcoin_avg_block_time: Duration,
    pub bitcoin_cancel_timelock: CancelTimelock,
    pub bitcoin_punish_timelock: PunishTimelock,
    pub bitcoin_network: bitcoin::Network,
    pub monero_avg_block_time: Duration,
    pub monero_finality_confirmations: u64,
    #[serde(with = "monero_network")]
    pub monero_network: monero::Network,
}

impl Config {
    pub fn bitcoin_sync_interval(&self) -> Duration {
        sync_interval(self.bitcoin_avg_block_time)
    }

    pub fn monero_sync_interval(&self) -> Duration {
        sync_interval(self.monero_avg_block_time)
    }
}

pub trait GetConfig {
    fn get_config() -> Config;
}

#[derive(Clone, Copy)]
pub struct Mainnet;

#[derive(Clone, Copy)]
pub struct Testnet;

#[derive(Clone, Copy)]
pub struct Regtest;

impl GetConfig for Mainnet {
    fn get_config() -> Config {
        Config {
            bitcoin_lock_mempool_timeout: 10.std_minutes(),
            bitcoin_lock_confirmed_timeout: 2.std_hours(),
            bitcoin_finality_confirmations: 1,
            bitcoin_avg_block_time: 10.std_minutes(),
            bitcoin_cancel_timelock: CancelTimelock::new(72),
            bitcoin_punish_timelock: PunishTimelock::new(72),
            bitcoin_network: bitcoin::Network::Bitcoin,
            monero_avg_block_time: 2.std_minutes(),
            monero_finality_confirmations: 10,
            monero_network: monero::Network::Mainnet,
        }
    }
}

impl GetConfig for Testnet {
    fn get_config() -> Config {
        Config {
            bitcoin_lock_mempool_timeout: 10.std_minutes(),
            bitcoin_lock_confirmed_timeout: 1.std_hours(),
            bitcoin_finality_confirmations: 1,
            bitcoin_avg_block_time: 10.std_minutes(),
            bitcoin_cancel_timelock: CancelTimelock::new(12),
            bitcoin_punish_timelock: PunishTimelock::new(6),
            bitcoin_network: bitcoin::Network::Testnet,
            monero_avg_block_time: 2.std_minutes(),
            monero_finality_confirmations: 10,
            monero_network: monero::Network::Stagenet,
        }
    }
}

impl GetConfig for Regtest {
    fn get_config() -> Config {
        Config {
            bitcoin_lock_mempool_timeout: 30.std_seconds(),
            bitcoin_lock_confirmed_timeout: 1.std_minutes(),
            bitcoin_finality_confirmations: 1,
            bitcoin_avg_block_time: 5.std_seconds(),
            bitcoin_cancel_timelock: CancelTimelock::new(100),
            bitcoin_punish_timelock: PunishTimelock::new(50),
            bitcoin_network: bitcoin::Network::Regtest,
            monero_avg_block_time: 1.std_seconds(),
            monero_finality_confirmations: 10,
            monero_network: monero::Network::Mainnet, // yes this is strange
        }
    }
}

fn sync_interval(avg_block_time: Duration) -> Duration {
    max(avg_block_time / 10, Duration::from_secs(1))
}

pub fn new(is_testnet: bool, asb_config: &asb::config::Config) -> Config {
    let env_config = if is_testnet {
        Testnet::get_config()
    } else {
        Mainnet::get_config()
    };

    let env_config =
        if let Some(bitcoin_finality_confirmations) = asb_config.bitcoin.finality_confirmations {
            Config {
                bitcoin_finality_confirmations,
                ..env_config
            }
        } else {
            env_config
        };

    if let Some(monero_finality_confirmations) = asb_config.monero.finality_confirmations {
        Config {
            monero_finality_confirmations,
            ..env_config
        }
    } else {
        env_config
    }
}

mod monero_network {
    use crate::monero::Network;
    use serde::Serializer;

    pub fn serialize<S>(x: &monero::Network, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let str = match x {
            Network::Mainnet => "mainnet",
            Network::Stagenet => "stagenet",
            Network::Testnet => "testnet",
        };
        s.serialize_str(str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_interval_is_one_second_if_avg_blocktime_is_one_second() {
        let interval = sync_interval(Duration::from_secs(1));

        assert_eq!(interval, Duration::from_secs(1))
    }

    #[test]
    fn check_interval_is_tenth_of_avg_blocktime() {
        let interval = sync_interval(Duration::from_secs(100));

        assert_eq!(interval, Duration::from_secs(10))
    }
}
