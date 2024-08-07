use libp2p::rendezvous::Namespace;
use std::fmt;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum XmrBtcNamespace {
    Mainnet,
    Testnet,
}

const MAINNET: &str = "xmr-btc-swap-mainnet";
const TESTNET: &str = "xmr-btc-swap-testnet";

impl fmt::Display for XmrBtcNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XmrBtcNamespace::Mainnet => write!(f, "{}", MAINNET),
            XmrBtcNamespace::Testnet => write!(f, "{}", TESTNET),
        }
    }
}

impl From<XmrBtcNamespace> for Namespace {
    fn from(namespace: XmrBtcNamespace) -> Self {
        match namespace {
            XmrBtcNamespace::Mainnet => Namespace::from_static(MAINNET),
            XmrBtcNamespace::Testnet => Namespace::from_static(TESTNET),
        }
    }
}

impl XmrBtcNamespace {
    pub fn from_is_testnet(testnet: bool) -> XmrBtcNamespace {
        if testnet {
            XmrBtcNamespace::Testnet
        } else {
            XmrBtcNamespace::Mainnet
        }
    }
}
