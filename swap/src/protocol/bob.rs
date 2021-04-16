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
mod execution_setup;
pub mod refund;
pub mod state;
pub mod swap;

pub struct Swap {
    pub state: BobState,
    pub event_loop_handle: EventLoopHandle,
    pub db: Database,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub env_config: env::Config,
    pub swap_id: Uuid,
    pub receive_monero_address: monero::Address,
}

pub struct Builder {
    swap_id: Uuid,
    db: Database,

    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,

    init_params: InitParams,
    env_config: env::Config,

    event_loop_handle: EventLoopHandle,

    receive_monero_address: monero::Address,
}

enum InitParams {
    None,
    New { btc_amount: bitcoin::Amount },
}

impl Builder {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Database,
        swap_id: Uuid,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        env_config: env::Config,
        event_loop_handle: EventLoopHandle,
        receive_monero_address: monero::Address,
    ) -> Self {
        Self {
            swap_id,
            db,
            bitcoin_wallet,
            monero_wallet,
            init_params: InitParams::None,
            env_config,
            event_loop_handle,
            receive_monero_address,
        }
    }

    pub fn with_init_params(self, btc_amount: bitcoin::Amount) -> Self {
        Self {
            init_params: InitParams::New { btc_amount },
            ..self
        }
    }

    pub fn build(self) -> Result<Swap> {
        let state = match self.init_params {
            InitParams::New { btc_amount } => BobState::Started { btc_amount },
            InitParams::None => self.db.get_state(self.swap_id)?.try_into_bob()?.into(),
        };

        Ok(Swap {
            state,
            event_loop_handle: self.event_loop_handle,
            db: self.db,
            bitcoin_wallet: self.bitcoin_wallet.clone(),
            monero_wallet: self.monero_wallet.clone(),
            swap_id: self.swap_id,
            env_config: self.env_config,
            receive_monero_address: self.receive_monero_address,
        })
    }
}
