use serde::{Deserialize, Serialize};

pub mod alice;
pub mod bob;
pub mod network;

pub const ONE_BTC: u64 = 100_000_000;

pub type Never = std::convert::Infallible;

/// Commands sent from Bob to the main task.
#[derive(Debug)]
pub enum Cmd {
    VerifyAmounts(SwapParams),
}

/// Responses send from the main task back to Bob.
#[derive(Debug, PartialEq)]
pub enum Rsp {
    Verified,
    Abort,
}

/// XMR/BTC swap parameters.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct SwapParams {
    /// Amount of BTC to swap.
    pub btc: bitcoin::Amount,
    /// Amount of XMR to swap.
    pub xmr: monero::Amount,
}

// FIXME: Amount modules are a quick hack so we can derive serde.

pub mod monero {
    use serde::{Deserialize, Serialize};
    use std::fmt;

    #[derive(Copy, Clone, Debug, Serialize, Deserialize)]
    pub struct Amount(u64);

    impl Amount {
        /// Create an [Amount] with piconero precision and the given number of
        /// piconeros.
        ///
        /// A piconero (a.k.a atomic unit) is equal to 1e-12 XMR.
        pub fn from_piconero(amount: u64) -> Self {
            Amount(amount)
        }
        pub fn as_piconero(&self) -> u64 {
            self.0
        }
    }

    impl From<Amount> for u64 {
        fn from(from: Amount) -> u64 {
            from.0
        }
    }

    impl fmt::Display for Amount {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{} piconeros", self.0)
        }
    }
}

pub mod bitcoin {
    use serde::{Deserialize, Serialize};
    use std::fmt;

    #[derive(Copy, Clone, Debug, Serialize, Deserialize)]
    pub struct Amount(u64);

    impl Amount {
        /// The zero amount.
        pub const ZERO: Amount = Amount(0);
        /// Exactly one satoshi.
        pub const ONE_SAT: Amount = Amount(1);
        /// Exactly one bitcoin.
        pub const ONE_BTC: Amount = Amount(100_000_000);

        /// Create an [Amount] with satoshi precision and the given number of
        /// satoshis.
        pub fn from_sat(satoshi: u64) -> Amount {
            Amount(satoshi)
        }
    }

    impl fmt::Display for Amount {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{} satoshis", self.0)
        }
    }
}
