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

use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use monero::util::ringct::Clsag;

pub struct AdaptorSignature {
    s_0: Scalar,
    fake_responses: [Scalar; 10],
    h_0: Scalar,
    /// Commitment key image `D = z * hash_to_p3(signing_public_key)`
    D: EdwardsPoint,
}

pub struct HalfAdaptorSignature {
    s_0_half: Scalar,
    fake_responses: [Scalar; 10],
    h_0: Scalar,
    /// Commitment key image `D = z * hash_to_p3(signing_public_key)`
    D: EdwardsPoint,
}

impl HalfAdaptorSignature {
    fn complete(self, s_other_half: Scalar) -> AdaptorSignature {
        AdaptorSignature {
            s_0: self.s_0_half + s_other_half,
            fake_responses: self.fake_responses,
            h_0: self.h_0,
            D: self.D,
        }
    }
}

impl AdaptorSignature {
    pub fn adapt(self, y: Scalar) -> Clsag {
        let r_last = self.s_0 + y;

        Clsag {
            s: std::iter::once(r_last)
                .chain(self.fake_responses.iter().copied())
                .collect(),
            D: self.D,
            c1: self.h_0,
        }
    }
}
