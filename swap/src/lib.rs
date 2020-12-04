#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub mod alice;
pub mod bitcoin;
pub mod bob;
pub mod cli;
pub mod monero;
pub mod network;
pub mod recover;
pub mod state;
pub mod storage;
pub mod tor;

// REFUND_TIMELOCK determines the interval between lock-time until TX_cancel is
// allowed, PUNISH_TIMELOCK determines the interval between TX_cancel and
// TX_punish being allowed.
//
// *[1]
//     |----REFUND_TIMELOCK--|
//                            *[2]
//                                |----PUNISH_TIMELOCK----|
//                                                         *[3]
// [1] LockTime point
// [2] TX_cancel+TX_Refund point
// [3] TX_punish point
//
// Given the above, setting both to 24 blocks (roughly 4h) is reasonable.
// TODO: More reasoning what are "good" timelocks
pub const REFUND_TIMELOCK: u32 = 24; // Relative timelock, this is number of blocks.
pub const PUNISH_TIMELOCK: u32 = 24;

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
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
// TODO(Franck): review necessity of this struct
pub struct SwapAmounts {
    /// Amount of BTC to swap.
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc: bitcoin::Amount,
    /// Amount of XMR to swap.
    #[serde(with = "xmr_btc::serde::monero_amount")]
    pub xmr: monero::Amount,
}

// TODO: Display in XMR and BTC (not picos and sats).
impl Display for SwapAmounts {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} sats for {} piconeros",
            self.btc.as_sat(),
            self.xmr.as_piconero()
        )
    }
}
