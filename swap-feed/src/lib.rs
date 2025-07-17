pub mod kraken;
pub mod rate;
pub mod traits;

// Re-exports for convenience
pub use kraken::{connect, PriceUpdates, Error as KrakenError};
pub use rate::{Rate, FixedRate, KrakenRate};
pub use traits::LatestRate;

// Core functions
pub fn connect_kraken(url: url::Url) -> anyhow::Result<kraken::PriceUpdates> {
    kraken::connect(url)
}