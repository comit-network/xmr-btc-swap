use crate::asb::Rate;

#[derive(Clone, Copy, Debug)]
pub struct FixedRate(Rate);

impl FixedRate {
    pub const RATE: f64 = 0.01;

    pub fn value(&self) -> Rate {
        self.0
    }
}

impl Default for FixedRate {
    fn default() -> Self {
        let ask = bitcoin::Amount::from_btc(Self::RATE).expect("Static value should never fail");

        Self(Rate::new(ask))
    }
}
