#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![warn(clippy::needless_pass_by_value)]

pub(crate) mod alice;
pub(crate) mod bob;
pub(crate) mod commitment;
pub(crate) mod dleq_proof;
pub(crate) mod messages;

pub use self::alice::*;
pub use self::bob::*;
pub use self::commitment::*;
pub use self::messages::*;

use curve25519_dalek::scalar::Scalar;
use monero::util::ringct::Clsag;

pub struct AdaptorSignature {
    inner: Clsag,
    signing_kex_index: usize,
}

pub struct HalfAdaptorSignature {
    inner: Clsag,
    signing_kex_index: usize,
    stupid_constant: Scalar,
}

impl HalfAdaptorSignature {
    fn complete(self, s_other_half: Scalar) -> AdaptorSignature {
        let mut sig = self.inner;
        let signing_kex_index = self.signing_kex_index;

        sig.s[signing_kex_index] += s_other_half;
        sig.s[signing_kex_index] += self.stupid_constant;

        AdaptorSignature {
            inner: sig,
            signing_kex_index,
        }
    }

    fn s_half(&self) -> Scalar {
        self.inner.s[self.signing_kex_index]
    }
}

impl AdaptorSignature {
    pub fn adapt(self, y: Scalar) -> Clsag {
        let mut sig = self.inner;
        sig.s[self.signing_kex_index] += y;

        sig
    }
}
