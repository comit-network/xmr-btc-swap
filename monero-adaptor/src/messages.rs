use crate::commitment::{Commitment, Opening};
use crate::dleq_proof::DleqProof;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;

// Alice Sends this to Bob
pub struct Message0 {
    pub c_a: Commitment,
    pub pi_a: DleqProof,
}

// Bob sends this to ALice
pub struct Message1 {
    pub I_b: EdwardsPoint,
    pub T_b: EdwardsPoint,
    pub I_hat_b: EdwardsPoint,
    pub pi_b: DleqProof,
}

// Alice sends this to Bob
pub struct Message2 {
    pub d_a: Opening,
    pub s_0_a: Scalar,
}

// Bob sends this to Alice
#[derive(Clone, Copy)]
pub struct Message3 {
    pub s_0_b: Scalar,
}
