#![warn(
    unused_extern_crates,
    missing_debug_implementations,
    missing_copy_implementations,
    rust_2018_idioms,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::fallible_impl_from,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::dbg_macro
)]
#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![forbid(unsafe_code)]
#![allow(non_snake_case)]

#[derive(Debug, Clone, Copy)]
pub enum Epoch {
    T0,
    T1,
    T2,
}

#[macro_use]
mod utils {

    macro_rules! impl_try_from_parent_enum {
        ($type:ident, $parent:ident) => {
            impl TryFrom<$parent> for $type {
                type Error = anyhow::Error;
                fn try_from(from: $parent) -> Result<Self> {
                    if let $parent::$type(inner) = from {
                        Ok(inner)
                    } else {
                        Err(anyhow::anyhow!(
                            "Failed to convert parent state to child state"
                        ))
                    }
                }
            }
        };
    }

    macro_rules! impl_from_child_enum {
        ($type:ident, $parent:ident) => {
            impl From<$type> for $parent {
                fn from(from: $type) -> Self {
                    $parent::$type(from)
                }
            }
        };
    }
}

pub mod alice;
pub mod bitcoin;
pub mod bob;
pub mod config;
pub mod monero;
pub mod serde;
pub mod transport;

use crate::bitcoin::{BlockHeight, TransactionBlockHeight, WatchForRawTransaction};
pub use cross_curve_dleq;

pub async fn current_epoch<W>(
    bitcoin_wallet: &W,
    refund_timelock: u32,
    punish_timelock: u32,
    lock_tx_id: ::bitcoin::Txid,
) -> anyhow::Result<Epoch>
where
    W: WatchForRawTransaction + TransactionBlockHeight + BlockHeight,
{
    let current_block_height = bitcoin_wallet.block_height().await;
    let t0 = bitcoin_wallet.transaction_block_height(lock_tx_id).await;
    let t1 = t0 + refund_timelock;
    let t2 = t1 + punish_timelock;

    match (current_block_height < t1, current_block_height < t2) {
        (true, _) => Ok(Epoch::T0),
        (false, true) => Ok(Epoch::T1),
        (false, false) => Ok(Epoch::T2),
    }
}
