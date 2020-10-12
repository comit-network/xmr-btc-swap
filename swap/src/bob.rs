//! Run an XMR/BTC swap in the role of Bob.
//! Bob holds BTC and wishes receive XMR.

use crate::SwapParams;
use anyhow::Result;
use xmr_btc::monero;

/// Request to swap `btc` with Alice.
///
/// Connects to a node running an instance in the role of Alice and requests to
/// swap `btc` i.e., this gets the rate from Alice for `btc`.
pub async fn request_swap_btc(_btc: ::bitcoin::Amount) -> Result<SwapParams> {
    todo!("Get rate from Alice for this many btc")
}

/// Request to swap `xmr` with Alice.
///
/// Connects to a node running an instance in the role of Alice and requests to
/// swap `xmr` i.e., this gets the rate from Alice for `xmr`.
pub async fn request_swap_xmr(_xmr: monero::Amount) -> Result<SwapParams> {
    todo!("Get rate from Alice for this many xmr")
}

/// XMR/BTC swap in the role of Bob.
pub fn swap(_: SwapParams) -> Result<()> {
    todo!("Run the swap as Bob")
}
