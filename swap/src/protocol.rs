pub mod alice;
pub mod bob;

#[derive(Debug, Clone)]
pub struct StartingBalances {
    pub xmr: crate::monero::Amount,
    pub btc: bitcoin::Amount,
}
