pub mod kraken;
pub mod rate;
pub mod traits;

// Re-exports for convenience
pub use kraken::{connect, Error as KrakenError, PriceUpdates};
pub use rate::{FixedRate, KrakenRate, Rate};
pub use traits::LatestRate;

// Core functions
pub fn connect_kraken(url: url::Url) -> anyhow::Result<kraken::PriceUpdates> {
    kraken::connect(url)
}
