use crate::{bitcoin::Timelock, config::File};
use conquer_once::Lazy;
use std::time::Duration;
use url::Url;

pub struct Settings {
    pub wallets: Wallets,
    pub protocol: Protocol,
}

impl Settings {
    pub fn from_config_file_and_defaults(config: File) -> Self {
        Settings::testnet(
            config.bitcoin.bitcoind_url,
            config.bitcoin.wallet_name,
            config.monero.wallet_rpc_url,
        )
    }

    fn testnet(bitcoind_url: Url, bitcoin_wallet_name: String, monero_wallet_rpc_url: Url) -> Self {
        Self {
            wallets: Wallets::testnet(bitcoind_url, bitcoin_wallet_name, monero_wallet_rpc_url),
            protocol: Protocol::testnet(),
        }
    }
}

pub struct Wallets {
    pub bitcoin: Bitcoin,
    pub monero: Monero,
}

impl Wallets {
    pub fn mainnet(
        bitcoind_url: Url,
        bitcoin_wallet_name: String,
        monero_wallet_rpc_url: Url,
    ) -> Self {
        Self {
            bitcoin: Bitcoin {
                bitcoind_url,
                wallet_name: bitcoin_wallet_name,
                network: bitcoin::Network::Bitcoin,
            },
            monero: Monero {
                wallet_rpc_url: monero_wallet_rpc_url,
                network: monero::Network::Mainnet,
            },
        }
    }

    pub fn testnet(
        bitcoind_url: Url,
        bitcoin_wallet_name: String,
        monero_wallet_rpc_url: Url,
    ) -> Self {
        Self {
            bitcoin: Bitcoin {
                bitcoind_url,
                wallet_name: bitcoin_wallet_name,
                network: bitcoin::Network::Testnet,
            },
            monero: Monero {
                wallet_rpc_url: monero_wallet_rpc_url,
                network: monero::Network::Stagenet,
            },
        }
    }
}

pub struct Bitcoin {
    pub bitcoind_url: Url,
    pub wallet_name: String,
    pub network: bitcoin::Network,
}

pub struct Monero {
    pub wallet_rpc_url: Url,
    pub network: monero::Network,
}

#[derive(Debug, Copy, Clone)]
pub struct Protocol {
    pub bob_time_to_act: Duration,
    pub bitcoin_finality_confirmations: u32,
    pub bitcoin_avg_block_time: Duration,
    pub monero_finality_confirmations: u32,
    pub bitcoin_cancel_timelock: Timelock,
    pub bitcoin_punish_timelock: Timelock,
}

impl Protocol {
    pub fn mainnet() -> Self {
        Self {
            bob_time_to_act: *mainnet::BOB_TIME_TO_ACT,
            bitcoin_finality_confirmations: mainnet::BITCOIN_FINALITY_CONFIRMATIONS,
            bitcoin_avg_block_time: *mainnet::BITCOIN_AVG_BLOCK_TIME,
            monero_finality_confirmations: mainnet::MONERO_FINALITY_CONFIRMATIONS,
            bitcoin_cancel_timelock: mainnet::BITCOIN_CANCEL_TIMELOCK,
            bitcoin_punish_timelock: mainnet::BITCOIN_PUNISH_TIMELOCK,
        }
    }

    pub fn testnet() -> Self {
        Self {
            bob_time_to_act: *testnet::BOB_TIME_TO_ACT,
            bitcoin_finality_confirmations: testnet::BITCOIN_FINALITY_CONFIRMATIONS,
            bitcoin_avg_block_time: *testnet::BITCOIN_AVG_BLOCK_TIME,
            monero_finality_confirmations: testnet::MONERO_FINALITY_CONFIRMATIONS,
            bitcoin_cancel_timelock: testnet::BITCOIN_CANCEL_TIMELOCK,
            bitcoin_punish_timelock: testnet::BITCOIN_PUNISH_TIMELOCK,
        }
    }

    pub fn regtest() -> Self {
        Self {
            bob_time_to_act: *regtest::BOB_TIME_TO_ACT,
            bitcoin_finality_confirmations: regtest::BITCOIN_FINALITY_CONFIRMATIONS,
            bitcoin_avg_block_time: *regtest::BITCOIN_AVG_BLOCK_TIME,
            monero_finality_confirmations: regtest::MONERO_FINALITY_CONFIRMATIONS,
            bitcoin_cancel_timelock: regtest::BITCOIN_CANCEL_TIMELOCK,
            bitcoin_punish_timelock: regtest::BITCOIN_PUNISH_TIMELOCK,
        }
    }
}

mod mainnet {
    use super::*;

    // For each step, we are giving Bob 10 minutes to act.
    pub static BOB_TIME_TO_ACT: Lazy<Duration> = Lazy::new(|| Duration::from_secs(10 * 60));

    pub static BITCOIN_FINALITY_CONFIRMATIONS: u32 = 3;

    pub static BITCOIN_AVG_BLOCK_TIME: Lazy<Duration> = Lazy::new(|| Duration::from_secs(10 * 60));

    pub static MONERO_FINALITY_CONFIRMATIONS: u32 = 15;

    // Set to 12 hours, arbitrary value to be reviewed properly
    pub static BITCOIN_CANCEL_TIMELOCK: Timelock = Timelock::new(72);
    pub static BITCOIN_PUNISH_TIMELOCK: Timelock = Timelock::new(72);
}

mod testnet {
    use super::*;

    pub static BOB_TIME_TO_ACT: Lazy<Duration> = Lazy::new(|| Duration::from_secs(60 * 60));

    // This does not reflect recommended values for mainnet!
    pub static BITCOIN_FINALITY_CONFIRMATIONS: u32 = 1;

    pub static BITCOIN_AVG_BLOCK_TIME: Lazy<Duration> = Lazy::new(|| Duration::from_secs(5 * 60));

    // This does not reflect recommended values for mainnet!
    pub static MONERO_FINALITY_CONFIRMATIONS: u32 = 5;

    // This does not reflect recommended values for mainnet!
    pub static BITCOIN_CANCEL_TIMELOCK: Timelock = Timelock::new(12);
    pub static BITCOIN_PUNISH_TIMELOCK: Timelock = Timelock::new(6);
}

mod regtest {
    use super::*;

    // In test, we set a shorter time to fail fast
    pub static BOB_TIME_TO_ACT: Lazy<Duration> = Lazy::new(|| Duration::from_secs(30));

    pub static BITCOIN_FINALITY_CONFIRMATIONS: u32 = 1;

    pub static BITCOIN_AVG_BLOCK_TIME: Lazy<Duration> = Lazy::new(|| Duration::from_secs(5));

    pub static MONERO_FINALITY_CONFIRMATIONS: u32 = 1;

    pub static BITCOIN_CANCEL_TIMELOCK: Timelock = Timelock::new(50);

    pub static BITCOIN_PUNISH_TIMELOCK: Timelock = Timelock::new(50);
}
