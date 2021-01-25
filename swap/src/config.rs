pub mod seed;

use crate::bitcoin::Timelock;
use conquer_once::Lazy;
use std::time::Duration;

#[derive(Debug, Copy, Clone)]
pub struct Config {
    pub bob_time_to_act: Duration,
    pub bitcoin_finality_confirmations: u32,
    pub bitcoin_avg_block_time: Duration,
    pub monero_finality_confirmations: u32,
    pub bitcoin_cancel_timelock: Timelock,
    pub bitcoin_punish_timelock: Timelock,
    pub bitcoin_network: bitcoin::Network,
    pub monero_network: monero::Network,
}

impl Config {
    pub fn mainnet() -> Self {
        Self {
            bob_time_to_act: *mainnet::BOB_TIME_TO_ACT,
            bitcoin_finality_confirmations: mainnet::BITCOIN_FINALITY_CONFIRMATIONS,
            bitcoin_avg_block_time: *mainnet::BITCOIN_AVG_BLOCK_TIME,
            monero_finality_confirmations: mainnet::MONERO_FINALITY_CONFIRMATIONS,
            bitcoin_cancel_timelock: mainnet::BITCOIN_CANCEL_TIMELOCK,
            bitcoin_punish_timelock: mainnet::BITCOIN_PUNISH_TIMELOCK,
            bitcoin_network: bitcoin::Network::Bitcoin,
            monero_network: monero::Network::Mainnet,
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
            bitcoin_network: bitcoin::Network::Testnet,
            monero_network: monero::Network::Stagenet,
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
            bitcoin_network: bitcoin::Network::Regtest,
            monero_network: monero::Network::default(),
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

    pub static BITCOIN_CANCEL_TIMELOCK: Timelock = Timelock::new(100);

    pub static BITCOIN_PUNISH_TIMELOCK: Timelock = Timelock::new(50);
}
