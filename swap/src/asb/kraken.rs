use crate::asb::{LatestRate, Rate};
use crate::kraken;
use anyhow::Result;
use tokio::sync::watch::Receiver;

#[derive(Clone)]
pub struct RateService {
    receiver: Receiver<Result<Rate, kraken::Error>>,
}

impl LatestRate for RateService {
    type Error = kraken::Error;

    fn latest_rate(&mut self) -> Result<Rate, Self::Error> {
        (*self.receiver.borrow()).clone()
    }
}

impl RateService {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            receiver: kraken::connect().await?,
        })
    }
}
