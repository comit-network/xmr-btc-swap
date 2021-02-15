use crate::bitcoin::{CancelTimelock, PunishTimelock};
use conquer_once::Lazy;
use std::time::Duration;

#[derive(Debug, Copy, Clone)]
pub struct ExecutionParams {
    pub bob_time_to_act: Duration,
    pub bitcoin_finality_confirmations: u32,
    pub bitcoin_avg_block_time: Duration,
    pub monero_finality_confirmations: u32,
    pub bitcoin_cancel_timelock: CancelTimelock,
    pub bitcoin_punish_timelock: PunishTimelock,
}

pub trait GetExecutionParams {
    fn get_execution_params() -> ExecutionParams;
}

#[derive(Clone, Copy)]
pub struct Mainnet;

#[derive(Clone, Copy)]
pub struct Testnet;

#[derive(Clone, Copy)]
pub struct Regtest;

impl GetExecutionParams for Mainnet {
    fn get_execution_params() -> ExecutionParams {
        ExecutionParams {
            bob_time_to_act: *mainnet::BOB_TIME_TO_ACT,
            bitcoin_finality_confirmations: mainnet::BITCOIN_FINALITY_CONFIRMATIONS,
            bitcoin_avg_block_time: *mainnet::BITCOIN_AVG_BLOCK_TIME,
            monero_finality_confirmations: mainnet::MONERO_FINALITY_CONFIRMATIONS,
            bitcoin_cancel_timelock: mainnet::BITCOIN_CANCEL_TIMELOCK,
            bitcoin_punish_timelock: mainnet::BITCOIN_PUNISH_TIMELOCK,
        }
    }
}

impl GetExecutionParams for Testnet {
    fn get_execution_params() -> ExecutionParams {
        ExecutionParams {
            bob_time_to_act: *testnet::BOB_TIME_TO_ACT,
            bitcoin_finality_confirmations: testnet::BITCOIN_FINALITY_CONFIRMATIONS,
            bitcoin_avg_block_time: *testnet::BITCOIN_AVG_BLOCK_TIME,
            monero_finality_confirmations: testnet::MONERO_FINALITY_CONFIRMATIONS,
            bitcoin_cancel_timelock: testnet::BITCOIN_CANCEL_TIMELOCK,
            bitcoin_punish_timelock: testnet::BITCOIN_PUNISH_TIMELOCK,
        }
    }
}

impl GetExecutionParams for Regtest {
    fn get_execution_params() -> ExecutionParams {
        ExecutionParams {
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
    use crate::execution_params::*;

    // For each step, we are giving Bob 10 minutes to act.
    pub static BOB_TIME_TO_ACT: Lazy<Duration> = Lazy::new(|| Duration::from_secs(10 * 60));

    pub static BITCOIN_FINALITY_CONFIRMATIONS: u32 = 3;

    pub static BITCOIN_AVG_BLOCK_TIME: Lazy<Duration> = Lazy::new(|| Duration::from_secs(10 * 60));

    pub static MONERO_FINALITY_CONFIRMATIONS: u32 = 15;

    // Set to 12 hours, arbitrary value to be reviewed properly
    pub static BITCOIN_CANCEL_TIMELOCK: CancelTimelock = CancelTimelock::new(72);
    pub static BITCOIN_PUNISH_TIMELOCK: PunishTimelock = PunishTimelock::new(72);
}

mod testnet {
    use crate::execution_params::*;

    pub static BOB_TIME_TO_ACT: Lazy<Duration> = Lazy::new(|| Duration::from_secs(60 * 60));

    // This does not reflect recommended values for mainnet!
    pub static BITCOIN_FINALITY_CONFIRMATIONS: u32 = 1;

    pub static BITCOIN_AVG_BLOCK_TIME: Lazy<Duration> = Lazy::new(|| Duration::from_secs(5 * 60));

    // This does not reflect recommended values for mainnet!
    pub static MONERO_FINALITY_CONFIRMATIONS: u32 = 5;

    // This does not reflect recommended values for mainnet!
    pub static BITCOIN_CANCEL_TIMELOCK: CancelTimelock = CancelTimelock::new(12);
    pub static BITCOIN_PUNISH_TIMELOCK: PunishTimelock = PunishTimelock::new(6);
}

mod regtest {
    use crate::execution_params::*;

    // In test, we set a shorter time to fail fast
    pub static BOB_TIME_TO_ACT: Lazy<Duration> = Lazy::new(|| Duration::from_secs(30));

    pub static BITCOIN_FINALITY_CONFIRMATIONS: u32 = 1;

    pub static BITCOIN_AVG_BLOCK_TIME: Lazy<Duration> = Lazy::new(|| Duration::from_secs(5));

    pub static MONERO_FINALITY_CONFIRMATIONS: u32 = 1;

    pub static BITCOIN_CANCEL_TIMELOCK: CancelTimelock = CancelTimelock::new(100);

    pub static BITCOIN_PUNISH_TIMELOCK: PunishTimelock = PunishTimelock::new(50);
}
