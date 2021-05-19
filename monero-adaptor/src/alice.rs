use crate::commitment::{Commitment, Opening};
use crate::dleq_proof::DleqProof;
use crate::messages::{Message0, Message1, Message2, Message3};
use crate::{AdaptorSignature, HalfAdaptorSignature};
use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use hash_edwards_to_edwards::hash_point_to_point;
use rand::{CryptoRng, Rng};

pub struct Alice0 {
    // secret index is always 0
    ring: [EdwardsPoint; 11],
    fake_responses: [Scalar; 10],
    commitment_ring: [EdwardsPoint; 11],
    pseudo_output_commitment: EdwardsPoint,
    msg: [u8; 32],
    // encryption key
    R_a: EdwardsPoint,
    // R'a = r_a*H_p(p_k) where p_k is the signing public key
    R_prime_a: EdwardsPoint,
    // this is not s_a cos of something to with one-time-address??
    s_prime_a: Scalar,
    // secret value:
    alpha_a: Scalar,
    H_p_pk: EdwardsPoint,
    I_a: EdwardsPoint,
    I_hat_a: EdwardsPoint,
    T_a: EdwardsPoint,
    z: Scalar,
}

impl Alice0 {
    pub fn new(
        ring: [EdwardsPoint; 11],
        msg: [u8; 32],
        commitment_ring: [EdwardsPoint; 11],
        pseudo_output_commitment: EdwardsPoint,
        R_a: EdwardsPoint,
        R_prime_a: EdwardsPoint,
        s_prime_a: Scalar,
        z: Scalar,
        rng: &mut (impl Rng + CryptoRng),
    ) -> anyhow::Result<Self> {
        let mut fake_responses = [Scalar::zero(); 10];
        for response in fake_responses.iter_mut().take(10) {
            *response = Scalar::random(rng);
        }
        let alpha_a = Scalar::random(rng);

        let p_k = ring[0];
        let H_p_pk = hash_point_to_point(p_k);

        let I_a = s_prime_a * H_p_pk;
        let I_hat_a = alpha_a * H_p_pk;
        let T_a = alpha_a * ED25519_BASEPOINT_POINT;

        Ok(Alice0 {
            ring,
            fake_responses,
            commitment_ring,
            pseudo_output_commitment,
            msg,
            R_a,
            R_prime_a,
            s_prime_a,
            alpha_a,
            H_p_pk,
            I_a,
            I_hat_a,
            T_a,
            z,
        })
    }

    pub fn next_message(&self, rng: &mut (impl Rng + CryptoRng)) -> Message0 {
        Message0 {
            pi_a: DleqProof::new(
                ED25519_BASEPOINT_POINT,
                self.T_a,
                self.H_p_pk,
                self.I_hat_a,
                self.alpha_a,
                rng,
            ),
            c_a: Commitment::new(self.fake_responses, self.I_a, self.I_hat_a, self.T_a),
        }
    }

    pub fn receive(self, msg: Message1) -> anyhow::Result<Alice1> {
        msg.pi_b
            .verify(ED25519_BASEPOINT_POINT, msg.T_b, self.H_p_pk, msg.I_hat_b)?;

        let I = self.I_a + msg.I_b;
        let sig = monero::clsag::sign(
            &self.msg,
            self.s_prime_a,
            0,
            self.H_p_pk,
            self.alpha_a,
            &self.ring,
            &self.commitment_ring,
            self.fake_responses,
            self.z,
            self.pseudo_output_commitment,
            self.T_a + msg.T_b + self.R_a,
            self.I_hat_a + msg.I_hat_b + self.R_prime_a,
            I,
        );

        let sig = HalfAdaptorSignature {
            s_0_half: sig.s[0],
            fake_responses: self.fake_responses,
            h_0: sig.c1,
            D: sig.D,
        };

        Ok(Alice1 {
            fake_responses: self.fake_responses,
            I_a: self.I_a,
            I_hat_a: self.I_hat_a,
            T_a: self.T_a,
            sig,
            I,
        })
    }
}

pub struct Alice1 {
    fake_responses: [Scalar; 10],
    I_a: EdwardsPoint,
    I_hat_a: EdwardsPoint,
    T_a: EdwardsPoint,
    sig: HalfAdaptorSignature,
    I: EdwardsPoint,
}

impl Alice1 {
    pub fn next_message(&self) -> Message2 {
        Message2 {
            d_a: Opening::new(self.fake_responses, self.I_a, self.I_hat_a, self.T_a),
            s_0_a: self.sig.s_0_half,
        }
    }

    pub fn receive(self, msg: Message3) -> Alice2 {
        let adaptor_sig = self.sig.complete(msg.s_0_b);

        Alice2 {
            adaptor_sig,
            I: self.I,
        }
    }
}

pub struct Alice2 {
    pub adaptor_sig: AdaptorSignature,
    pub I: EdwardsPoint,
}
