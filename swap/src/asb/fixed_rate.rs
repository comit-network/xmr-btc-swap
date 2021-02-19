use crate::asb::{LatestRate, Rate};
use std::convert::Infallible;

pub const RATE: f64 = 0.01;

#[derive(Clone)]
pub struct RateService(Rate);

impl LatestRate for RateService {
    type Error = Infallible;

    fn latest_rate(&mut self) -> Result<Rate, Infallible> {
        Ok(self.0)
    }
}

impl Default for RateService {
    fn default() -> Self {
        Self(Rate {
            ask: bitcoin::Amount::from_btc(RATE).expect("Static value should never fail"),
        })
    }
}
