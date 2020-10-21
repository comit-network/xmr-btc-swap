use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub mod alice;
pub mod bitcoin;
pub mod bob;
pub mod monero;
pub mod network;

pub const ONE_BTC: u64 = 100_000_000;

pub type Never = std::convert::Infallible;

/// Commands sent from Bob to the main task.
#[derive(Clone, Copy, Debug)]
pub enum Cmd {
    VerifyAmounts(SwapParams),
}

/// Responses send from the main task back to Bob.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rsp {
    Verified,
    Abort,
}

/// XMR/BTC swap parameters.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct SwapParams {
    /// Amount of BTC to swap.
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc: bitcoin::Amount,
    /// Amount of XMR to swap.
    pub xmr: monero::Amount,
}

impl Display for SwapParams {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} for {}", self.btc, self.xmr)
    }
}
