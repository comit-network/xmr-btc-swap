use crate::bitcoin::{CancelTimelock, PunishTimelock};
use std::time::Duration;
use time::NumericalStdDurationShort;

#[derive(Debug, Copy, Clone)]
pub struct ExecutionParams {
    pub bob_time_to_act: Duration,
    pub bitcoin_finality_confirmations: u32,
    pub bitcoin_avg_block_time: Duration,
    pub bitcoin_cancel_timelock: CancelTimelock,
    pub bitcoin_punish_timelock: PunishTimelock,
    pub monero_avg_block_time: Duration,
    pub monero_finality_confirmations: u32,
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
            bob_time_to_act: 10.minutes(),
            bitcoin_finality_confirmations: 3,
            bitcoin_avg_block_time: 10.minutes(),
            bitcoin_cancel_timelock: CancelTimelock::new(72),
            bitcoin_punish_timelock: PunishTimelock::new(72),
            monero_avg_block_time: 2.minutes(),
            monero_finality_confirmations: 15,
        }
    }
}

impl GetExecutionParams for Testnet {
    fn get_execution_params() -> ExecutionParams {
        ExecutionParams {
            bob_time_to_act: 60.minutes(),
            bitcoin_finality_confirmations: 1,
            bitcoin_avg_block_time: 5.minutes(),
            bitcoin_cancel_timelock: CancelTimelock::new(12),
            bitcoin_punish_timelock: PunishTimelock::new(6),
            monero_avg_block_time: 2.minutes(),
            monero_finality_confirmations: 10,
        }
    }
}

impl GetExecutionParams for Regtest {
    fn get_execution_params() -> ExecutionParams {
        ExecutionParams {
            bob_time_to_act: 30.seconds(),
            bitcoin_finality_confirmations: 1,
            bitcoin_avg_block_time: 5.seconds(),
            bitcoin_cancel_timelock: CancelTimelock::new(100),
            bitcoin_punish_timelock: PunishTimelock::new(50),
            monero_avg_block_time: 1.seconds(),
            monero_finality_confirmations: 10,
        }
    }
}
