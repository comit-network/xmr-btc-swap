use crate::{
    final_challenge, AdaptorSignature, Commitment, DleqProof, Message0, Message1, Message2,
    Message3, RING_SIZE,
};
use anyhow::Result;
use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use hash_edwards_to_edwards::hash_point_to_point;
use rand::rngs::OsRng;

pub enum BobState {
    WaitingForCommitment(Bob0),
    WaitingForOpening(Bob1),
    PublishXmr,
    RedeemBtc,
    PunishXmr,
}

pub struct Bob0 {
    // secret index is always 0
    ring: [EdwardsPoint; RING_SIZE],
    msg: [u8; 32],
    // encryption key
    R_a: EdwardsPoint,
    // R'a = r_a*H_p(p_k) where p_k is the signing public key
    R_prime_a: EdwardsPoint,
    s_b: Scalar,
    // secret value:
    alpha_b: Scalar,
    H_p_pk: EdwardsPoint,
    I_b: EdwardsPoint,
    I_hat_b: EdwardsPoint,
    T_b: EdwardsPoint,
}

impl Bob0 {
    pub fn new(
        ring: [EdwardsPoint; RING_SIZE],
        msg: [u8; 32],
        R_a: EdwardsPoint,
        R_prime_a: EdwardsPoint,
        s_b: Scalar,
    ) -> Result<Self> {
        let alpha_b = Scalar::random(&mut OsRng);

        let p_k = ring[0];
        let H_p_pk = hash_point_to_point(p_k);

        let I_b = s_b * H_p_pk;
        let I_hat_b = alpha_b * H_p_pk;
        let T_b = alpha_b * ED25519_BASEPOINT_POINT;

        Ok(Bob0 {
            ring,
            msg,
            R_a,
            R_prime_a,
            s_b,
            alpha_b,
            H_p_pk,
            I_b,
            I_hat_b,
            T_b,
        })
    }

    pub fn receive(self, msg: Message0) -> Bob1 {
        Bob1 {
            ring: self.ring,
            msg: self.msg,
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
        }
    }
}

pub struct Bob1 {
    // secret index is always 0
    ring: [EdwardsPoint; RING_SIZE],
    msg: [u8; 32],
    // encryption key
    R_a: EdwardsPoint,
    // R'a = r_a*H_p(p_k) where p_k is the signing public key
    R_prime_a: EdwardsPoint,
    s_b: Scalar,
    // secret value:
    alpha_b: Scalar,
    H_p_pk: EdwardsPoint,
    I_b: EdwardsPoint,
    I_hat_b: EdwardsPoint,
    T_b: EdwardsPoint,
    pi_a: DleqProof,
    c_a: Commitment,
}

impl Bob1 {
    pub fn next_message(&self) -> Message1 {
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
            ),
        }
    }

    pub fn receive(self, msg: Message2) -> Result<Bob2> {
        let (fake_responses, I_a, I_hat_a, T_a) = msg.d_a.open(self.c_a)?;

        self.pi_a
            .verify(ED25519_BASEPOINT_POINT, T_a, self.H_p_pk, I_hat_a)?;

        let (h_last, h_0) = final_challenge(
            fake_responses,
            self.ring,
            T_a,
            self.T_b,
            self.R_a,
            I_hat_a,
            self.I_hat_b,
            self.R_prime_a,
            I_a,
            self.I_b,
            self.msg,
        )?;

        let s_0_b = self.alpha_b - h_last * self.s_b;

        let adaptor_sig = AdaptorSignature {
            s_0_a: msg.s_0_a,
            s_0_b,
            fake_responses,
            h_0,
            I: I_a + self.I_b,
        };

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
