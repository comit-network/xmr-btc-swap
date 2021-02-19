use crate::nectar::{LatestRate, Rate};

pub const RATE: f64 = 0.01;

#[derive(Clone)]
pub struct RateService(Rate);

impl LatestRate for RateService {
    type Error = anyhow::Error;

    fn latest_rate(&mut self) -> anyhow::Result<Rate> {
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
