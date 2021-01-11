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

use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub mod bitcoin;
pub mod cli;
pub mod config;
pub mod database;
pub mod fs;
pub mod monero;
pub mod network;
pub mod protocol;
pub mod seed;
pub mod trace;

pub type Never = std::convert::Infallible;

/// Commands sent from Bob to the main task.
#[derive(Clone, Copy, Debug)]
pub enum Cmd {
    VerifyAmounts(SwapAmounts),
}

/// Responses sent from the main task back to Bob.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rsp {
    VerifiedAmounts,
    Abort,
}

/// XMR/BTC swap amounts.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
// TODO(Franck): review necessity of this struct
pub struct SwapAmounts {
    /// Amount of BTC to swap.
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc: bitcoin::Amount,
    /// Amount of XMR to swap.
    #[serde(with = "monero::monero_amount")]
    pub xmr: monero::Amount,
}

// TODO: Display in XMR and BTC (not picos and sats).
impl Display for SwapAmounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} sats for {} piconeros",
            self.btc.as_sat(),
            self.xmr.as_piconero()
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ExpiredTimelocks {
    None,
    Cancel,
    Punish,
}
