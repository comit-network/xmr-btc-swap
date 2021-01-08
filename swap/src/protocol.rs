use crate::monero;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub mod alice;
pub mod bob;

#[derive(Debug, Clone, Copy)]
pub enum ExpiredTimelocks {
    None,
    Cancel,
    Punish,
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
