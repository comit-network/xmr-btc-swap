//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::env::Config;
use crate::{asb, bitcoin, monero};
use std::sync::Arc;
use uuid::Uuid;

pub use self::state::*;
pub use self::swap::{run, run_until};
use crate::protocol::Database;

pub mod state;
pub mod swap;

pub struct Swap<D: Database> {
    pub state: AliceState,
    pub event_loop_handle: asb::EventLoopHandle,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub env_config: Config,
    pub swap_id: Uuid,
    pub db: D,
}
