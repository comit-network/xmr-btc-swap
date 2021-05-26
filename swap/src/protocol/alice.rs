//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use std::sync::Arc;

use uuid::Uuid;

use crate::database::Database;
use crate::env::Config;
pub use crate::network::asb::behaviour::{Behaviour, OutEvent};
use crate::{bitcoin, monero};

pub use self::event_loop::{EventLoop, EventLoopHandle};
pub use self::recovery::cancel::cancel;
pub use self::recovery::punish::punish;
pub use self::recovery::redeem::redeem;
pub use self::recovery::refund::refund;
pub use self::recovery::safely_abort::safely_abort;
pub use self::recovery::{cancel, punish, redeem, refund, safely_abort};
pub use self::state::*;
pub use self::swap::{run, run_until};

pub mod event_loop;
mod recovery;
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
