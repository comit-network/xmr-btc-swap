use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
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
