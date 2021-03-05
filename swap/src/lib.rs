#![warn(
    unused_extern_crates,
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
#![allow(
    non_snake_case,
    missing_debug_implementations,
    missing_copy_implementations
)]

pub mod asb;
pub mod bitcoin;
pub mod cli;
pub mod database;
pub mod execution_params;
pub mod fs;
pub mod kraken;
pub mod monero;
pub mod network;
pub mod protocol;
pub mod seed;
pub mod trace;

mod monero_ext;
mod serde_peer_id;
