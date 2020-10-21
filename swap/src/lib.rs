use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub mod alice;
pub mod bitcoin;
pub mod bob;
pub mod monero;
pub mod network;

pub const ONE_BTC: u64 = 100_000_000;

const REFUND_TIMELOCK: u32 = 10; // FIXME: What should this be?
const PUNISH_TIMELOCK: u32 = 20; // FIXME: What should this be?

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
    pub btc: ::bitcoin::Amount,
    /// Amount of XMR to swap.
    #[serde(with = "crate::monero::amount_serde")]
    pub xmr: xmr_btc::monero::Amount,
}

impl Display for SwapParams {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} sats for {} piconeros",
            self.btc.as_sat(),
            self.xmr.as_piconero()
        )
    }
}
