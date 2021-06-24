use crate::database::Database;
use crate::{bitcoin, env, monero};
use anyhow::Result;
use std::sync::Arc;
use uuid::Uuid;

pub use self::behaviour::{Behaviour, OutEvent};
pub use self::cancel::cancel;
pub use self::event_loop::{EventLoop, EventLoopHandle};
pub use self::refund::refund;
pub use self::state::*;
pub use self::swap::{run, run_until};

mod behaviour;
pub mod cancel;
pub mod event_loop;
pub mod refund;
pub mod state;
pub mod swap;
mod swap_setup;

pub struct Swap {
    pub state: BobState,
    pub event_loop_handle: EventLoopHandle,
    pub db: Database,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub env_config: env::Config,
    pub id: Uuid,
    pub receive_monero_address: monero::Address,
}

impl Swap {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Database,
        id: Uuid,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        env_config: env::Config,
        event_loop_handle: EventLoopHandle,
        receive_monero_address: monero::Address,
        btc_amount: bitcoin::Amount,
    ) -> Self {
        Self {
            state: BobState::Started { btc_amount },
            event_loop_handle,
            db,
            bitcoin_wallet,
            monero_wallet,
            env_config,
            id,
            receive_monero_address,
        }
    }

    pub fn from_db(
        db: Database,
        id: Uuid,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        env_config: env::Config,
        event_loop_handle: EventLoopHandle,
        receive_monero_address: monero::Address,
    ) -> Result<Self> {
        let state = db.get_state(id)?.try_into_bob()?.into();

        Ok(Self {
            state,
            event_loop_handle,
            db,
            bitcoin_wallet,
            monero_wallet,
            env_config,
            id,
            receive_monero_address,
        })
    }
}
