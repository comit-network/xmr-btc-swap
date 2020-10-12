//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.

use anyhow::Result;
use std::thread;

/// Entrypoint for an XMR/BTC swap in the role of Alice.
pub fn swap() -> Result<()> {
    // Bob initiates the swap via network communication.
    thread::park();

    Ok(())
}
