use crate::monero;
use bitcoin::hashes::core::{fmt, fmt::Display};
use serde::{Deserialize, Serialize};

pub mod alice;
pub mod bob;

#[derive(Debug, Copy, Clone)]
pub struct StartingBalances {
    pub xmr: crate::monero::Amount,
    pub btc: bitcoin::Amount,
}

/// XMR/BTC swap amounts.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SwapAmounts {
    /// Amount of BTC to swap.
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc: bitcoin::Amount,
    /// Amount of XMR to swap.
    #[serde(with = "monero::monero_amount")]
    pub xmr: crate::monero::Amount,
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
