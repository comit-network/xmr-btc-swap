//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::database::Database;
use crate::env::Config;
use crate::{bitcoin, monero};
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
mod spot_price;
pub mod state;
pub mod swap;

pub struct Swap {
    pub state: AliceState,
    pub event_loop_handle: EventLoopHandle,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub env_config: Config,
    pub swap_id: Uuid,
    pub db: Arc<Database>,
}
