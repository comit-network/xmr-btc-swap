use conquer_once::Lazy;
use std::time::Duration;

#[derive(Debug, Copy, Clone)]
pub struct Config {
    pub bob_time_to_act: Duration,
    pub bitcoin_finality_confirmations: u32,
    pub bitcoin_avg_block_time: Duration,
    pub monero_max_finality_time: Duration,
}

impl Config {
    pub fn mainnet() -> Self {
        Self {
            bob_time_to_act: *mainnet::BOB_TIME_TO_ACT,
            bitcoin_finality_confirmations: mainnet::BITCOIN_FINALITY_CONFIRMATIONS,
            bitcoin_avg_block_time: *mainnet::BITCOIN_AVG_BLOCK_TIME,
            monero_max_finality_time: *mainnet::MONERO_MAX_FINALITY_TIME,
        }
    }

    pub fn regtest() -> Self {
        Self {
            bob_time_to_act: *regtest::BOB_TIME_TO_ACT,
            bitcoin_finality_confirmations: regtest::BITCOIN_FINALITY_CONFIRMATIONS,
            bitcoin_avg_block_time: *regtest::BITCOIN_AVG_BLOCK_TIME,
            monero_max_finality_time: *regtest::MONERO_MAX_FINALITY_TIME,
        }
    }
}

mod mainnet {
    use super::*;

    // For each step, we are giving Bob 10 minutes to act.
    pub static BOB_TIME_TO_ACT: Lazy<Duration> = Lazy::new(|| Duration::from_secs(10 * 60));

    pub static BITCOIN_FINALITY_CONFIRMATIONS: u32 = 3;

    pub static BITCOIN_AVG_BLOCK_TIME: Lazy<Duration> = Lazy::new(|| Duration::from_secs(10 * 60));

    pub static MONERO_MAX_FINALITY_TIME: Lazy<Duration> =
        Lazy::new(|| Duration::from_secs_f64(15f64 * 1.5 * 2f64 * 60f64));
}

mod regtest {
    use super::*;

    // In test, set to 5 seconds to fail fast
    pub static BOB_TIME_TO_ACT: Lazy<Duration> = Lazy::new(|| Duration::from_secs(30));

    pub static BITCOIN_FINALITY_CONFIRMATIONS: u32 = 1;

    pub static BITCOIN_AVG_BLOCK_TIME: Lazy<Duration> = Lazy::new(|| Duration::from_secs(5));

    pub static MONERO_MAX_FINALITY_TIME: Lazy<Duration> =
        Lazy::new(|| Duration::from_secs_f64(60f64));
}
