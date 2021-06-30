use std::fmt::{Display, Formatter};

#[derive(Debug, PartialEq)]
pub enum XmrBtcNamespace {
    Mainnet,
    Testnet,
}

impl Display for XmrBtcNamespace {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            XmrBtcNamespace::Mainnet => write!(f, "xmr-btc-swap-mainnet"),
            XmrBtcNamespace::Testnet => write!(f, "xmr-btc-swap-testnet"),
        }
    }
}
