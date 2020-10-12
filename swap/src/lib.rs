use xmr_btc::monero;

pub mod alice;
pub mod bob;

/// XMR/BTC swap parameters.
pub struct SwapParams {
    /// Amount of BTC to swap.
    pub btc: bitcoin::Amount,
    /// Amount of XMR to swap.
    pub xmr: monero::Amount,
}
