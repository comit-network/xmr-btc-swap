pub mod command;
pub mod config;
pub mod fixed_rate;
pub mod kraken;

mod amounts;

pub use amounts::Rate;

pub trait LatestRate {
    type Error: std::error::Error + Send + Sync + 'static;

    fn latest_rate(&mut self) -> Result<Rate, Self::Error>;
}
