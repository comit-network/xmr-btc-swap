use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use hash_edwards_to_edwards::hash_point_to_point;
use rand::{CryptoRng, Rng};

use crate::commitment::Commitment;
use crate::dleq_proof::DleqProof;
use crate::messages::{Message0, Message1, Message2, Message3};
use crate::{AdaptorSignature, HalfAdaptorSignature};
use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;

pub struct Bob0 {
    ring: [EdwardsPoint; 11],
    msg: [u8; 32],
    commitment_ring: [EdwardsPoint; 11],
    pseudo_output_commitment: EdwardsPoint,
    R_a: EdwardsPoint,
    R_prime_a: EdwardsPoint,
    s_b: Scalar,
    alpha_b: Scalar,
    H_p_pk: EdwardsPoint,
    I_b: EdwardsPoint,
    I_hat_b: EdwardsPoint,
    T_b: EdwardsPoint,
    z: Scalar,
}

impl Bob0 {
    pub fn new(
        ring: [EdwardsPoint; 11],
        msg: [u8; 32],
        commitment_ring: [EdwardsPoint; 11],
        pseudo_output_commitment: EdwardsPoint,
        R_a: EdwardsPoint,
        R_prime_a: EdwardsPoint,
        s_b: Scalar,
        z: Scalar,
        rng: &mut (impl Rng + CryptoRng),
    ) -> anyhow::Result<Self> {
        let alpha_b = Scalar::random(rng);

        let p_k = ring[0];
        let H_p_pk = hash_point_to_point(p_k);

        let I_b = s_b * H_p_pk;
        let I_hat_b = alpha_b * H_p_pk;
        let T_b = alpha_b * ED25519_BASEPOINT_POINT;

        Ok(Bob0 {
            ring,
            msg,
            commitment_ring,
            pseudo_output_commitment,
            R_a,
            R_prime_a,
            s_b,
            alpha_b,
            H_p_pk,
            I_b,
            I_hat_b,
            T_b,
            z,
        })
    }

    pub fn receive(self, msg: Message0) -> Bob1 {
        Bob1 {
            ring: self.ring,
            msg: self.msg,
            commitment_ring: self.commitment_ring,
            pseudo_output_commitment: self.pseudo_output_commitment,
            R_a: self.R_a,
            R_prime_a: self.R_prime_a,
            s_b: self.s_b,
            alpha_b: self.alpha_b,
            H_p_pk: self.H_p_pk,
            I_b: self.I_b,
            I_hat_b: self.I_hat_b,
            T_b: self.T_b,
            pi_a: msg.pi_a,
            c_a: msg.c_a,
            z: self.z,
        }
    }
}

pub struct Bob1 {
    ring: [EdwardsPoint; 11],
    msg: [u8; 32],
    commitment_ring: [EdwardsPoint; 11],
    pseudo_output_commitment: EdwardsPoint,
    R_a: EdwardsPoint,
    R_prime_a: EdwardsPoint,
    s_b: Scalar,
    alpha_b: Scalar,
    H_p_pk: EdwardsPoint,
    I_b: EdwardsPoint,
    I_hat_b: EdwardsPoint,
    T_b: EdwardsPoint,
    pi_a: DleqProof,
    c_a: Commitment,
    z: Scalar,
}

impl Bob1 {
    pub fn next_message(&self, rng: &mut (impl Rng + CryptoRng)) -> Message1 {
        Message1 {
            I_b: self.I_b,
            T_b: self.T_b,
            I_hat_b: self.I_hat_b,
            pi_b: DleqProof::new(
                ED25519_BASEPOINT_POINT,
                self.T_b,
                self.H_p_pk,
                self.I_hat_b,
                self.alpha_b,
                rng,
            ),
        }
    }

    pub fn receive(self, msg: Message2) -> anyhow::Result<Bob2> {
        let (fake_responses, I_a, I_hat_a, T_a) = msg.d_a.open(self.c_a)?;

        self.pi_a
            .verify(ED25519_BASEPOINT_POINT, T_a, self.H_p_pk, I_hat_a)?;

        let I = I_a + self.I_b;
        let sig = monero::clsag::sign(
            &self.msg,
            self.s_b,
            0,
            self.H_p_pk,
            self.alpha_b,
            &self.ring,
            &self.commitment_ring,
            fake_responses,
            self.z,
            self.pseudo_output_commitment,
            T_a + self.T_b + self.R_a,
            I_hat_a + self.I_hat_b + self.R_prime_a,
            I,
        );

        let s_0_b = sig.s[0];
        let sig = HalfAdaptorSignature {
            s_0_half: s_0_b,
            fake_responses,
            h_0: sig.c1,
            D: sig.D,
        };
        let adaptor_sig = sig.complete(msg.s_0_a);

        Ok(Bob2 { s_0_b, adaptor_sig })
    }
}

pub struct Bob2 {
    s_0_b: Scalar,
    pub adaptor_sig: AdaptorSignature,
}

impl Bob2 {
    pub fn next_message(&self) -> Message3 {
        Message3 { s_0_b: self.s_0_b }
    }
}
