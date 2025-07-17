use crate::rate::Rate;

pub trait LatestRate {
    type Error: std::error::Error + Send + Sync + 'static;

    fn latest_rate(&mut self) -> Result<Rate, Self::Error>;
}

// Future: Allow for different price feed sources
pub trait PriceFeed: Sized {
    type Error: std::error::Error + Send + Sync + 'static;
    type Update;
    
    async fn connect(url: url::Url) -> Result<Self, Self::Error>;
    async fn next_update(&mut self) -> Result<Self::Update, Self::Error>;
}