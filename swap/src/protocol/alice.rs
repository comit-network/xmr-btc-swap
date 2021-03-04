//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::database::Database;
use crate::execution_params::ExecutionParams;
use crate::{bitcoin, monero};
use std::sync::Arc;
use uuid::Uuid;

pub use self::behaviour::{Behaviour, OutEvent};
pub use self::event_loop::{EventLoop, EventLoopHandle};
pub use self::execution_setup::Message1;
pub use self::state::*;
pub use self::swap::{run, run_until};
pub use self::transfer_proof::TransferProof;
pub use execution_setup::Message3;

mod behaviour;
mod encrypted_signature;
pub mod event_loop;
mod execution_setup;
pub mod state;
mod steps;
pub mod swap;
mod transfer_proof;

pub struct Swap {
    pub state: AliceState,
    pub event_loop_handle: EventLoopHandle,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub execution_params: ExecutionParams,
    pub swap_id: Uuid,
    pub db: Arc<Database>,
}
