pub mod alice;
pub mod bob;

#[derive(Debug, Copy, Clone)]
pub struct StartingBalances {
    pub xmr: crate::monero::Amount,
    pub btc: bitcoin::Amount,
}
