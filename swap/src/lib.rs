#![allow(non_snake_case)]

use crate::swap_amounts::SwapAmounts;

pub mod alice;
pub mod bitcoin;
pub mod bob;
pub mod cli;
pub mod monero;
pub mod network;
pub mod serde;
pub mod state;
pub mod storage;
pub mod swap_amounts;
pub mod trace;

#[cfg(test)]
pub mod tests;

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
