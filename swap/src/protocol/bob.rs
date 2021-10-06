use std::sync::Arc;

use anyhow::Result;
use uuid::Uuid;

use crate::protocol::Database;
use crate::{bitcoin, cli, env, monero};

pub use self::state::*;
pub use self::swap::{run, run_until};
use std::convert::TryInto;

pub mod state;
pub mod swap;

pub struct Swap {
    pub state: BobState,
    pub event_loop_handle: cli::EventLoopHandle,
    pub db: Arc<dyn Database + Send + Sync>,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub env_config: env::Config,
    pub id: Uuid,
    pub monero_receive_address: monero::Address,
}

impl Swap {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Arc<dyn Database + Send + Sync>,
        id: Uuid,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        env_config: env::Config,
        event_loop_handle: cli::EventLoopHandle,
        monero_receive_address: monero::Address,
        bitcoin_change_address: bitcoin::Address,
        btc_amount: bitcoin::Amount,
    ) -> Self {
        Self {
            state: BobState::Started {
                btc_amount,
                change_address: bitcoin_change_address,
            },
            event_loop_handle,
            db,
            bitcoin_wallet,
            monero_wallet,
            env_config,
            id,
            monero_receive_address,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn from_db(
        db: Arc<dyn Database + Send + Sync>,
        id: Uuid,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        env_config: env::Config,
        event_loop_handle: cli::EventLoopHandle,
        monero_receive_address: monero::Address,
    ) -> Result<Self> {
        let state = db.get_state(id).await?.try_into()?;

        Ok(Self {
            state,
            event_loop_handle,
            db,
            bitcoin_wallet,
            monero_wallet,
            env_config,
            id,
            monero_receive_address,
        })
    }
}
