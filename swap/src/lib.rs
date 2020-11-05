use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub mod alice;
pub mod bitcoin;
pub mod bob;
pub mod monero;
pub mod network;
pub mod storage;
pub mod tor;

const REFUND_TIMELOCK: u32 = 300; // Relative timelock, 300 chosen for test purposes where we have
                                  // 1block/second
const PUNISH_TIMELOCK: u32 = 10; // FIXME: What should this be?

pub type Never = std::convert::Infallible;

/// Commands sent from Bob to the main task.
#[derive(Clone, Copy, Debug)]
pub enum Cmd {
    VerifyAmounts(SwapAmounts),
}

/// Responses sent from the main task back to Bob.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rsp {
    VerifiedAmounts,
    Abort,
}

/// XMR/BTC swap amounts.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct SwapAmounts {
    /// Amount of BTC to swap.
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc: ::bitcoin::Amount,
    /// Amount of XMR to swap.
    #[serde(with = "xmr_btc::serde::monero_amount")]
    pub xmr: xmr_btc::monero::Amount,
}

// TODO: Display in XMR and BTC (not picos and sats).
impl Display for SwapAmounts {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} sats for {} piconeros",
            self.btc.as_sat(),
            self.xmr.as_piconero()
        )
    }
}

#[derive(Copy, Clone, Debug)]
pub struct TorConf {
    pub control_port: u16,
    pub proxy_port: u16,
    pub service_port: u16,
}

impl Default for TorConf {
    fn default() -> Self {
        Self {
            control_port: 9051,
            proxy_port: 9050,
            service_port: 9090,
        }
    }
}

impl TorConf {
    pub fn with_control_port(self, control_port: u16) -> Self {
        Self {
            control_port,
            ..self
        }
    }

    pub fn with_proxy_port(self, proxy_port: u16) -> Self {
        Self { proxy_port, ..self }
    }

    pub fn with_service_port(self, service_port: u16) -> Self {
        Self {
            service_port,
            ..self
        }
    }
}
