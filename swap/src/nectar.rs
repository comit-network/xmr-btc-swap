pub mod command;
pub mod config;
pub mod fixed_rate;
pub mod kraken;

mod amounts;

pub use amounts::Rate;

pub trait LatestRate {
    fn latest_rate(&mut self) -> Rate;
}
