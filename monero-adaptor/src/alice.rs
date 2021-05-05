use crate::{
    final_challenge, AdaptorSignature, Commitment, DleqProof, Message0, Message1, Message2,
    Message3, Opening, RING_SIZE,
};
use anyhow::Result;
use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use hash_edwards_to_edwards::hash_point_to_point;
use rand::rngs::OsRng;

pub enum AliceState {
    WaitingForCommitment(Alice0),
    WaitingForScalar(Alice1),
    PublishXmr,
    RedeemBtc,
    PunishXmr,
}

pub struct Alice0 {
    // secret index is always 0
    ring: [EdwardsPoint; RING_SIZE],
    fake_responses: [Scalar; RING_SIZE - 1],
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
}

impl Alice0 {
    pub fn new(
        ring: [EdwardsPoint; RING_SIZE],
        msg: [u8; 32],
        R_a: EdwardsPoint,
        R_prime_a: EdwardsPoint,
        s_prime_a: Scalar,
    ) -> Result<Self> {
        let mut fake_responses = [Scalar::zero(); RING_SIZE - 1];
        for response in fake_responses.iter_mut().take(RING_SIZE - 1) {
            *response = Scalar::random(&mut OsRng);
        }
        let alpha_a = Scalar::random(&mut OsRng);

        let p_k = ring[0];
        let H_p_pk = hash_point_to_point(p_k);

        let I_a = s_prime_a * H_p_pk;
        let I_hat_a = alpha_a * H_p_pk;
        let T_a = alpha_a * ED25519_BASEPOINT_POINT;

        Ok(Alice0 {
            ring,
            fake_responses,
            msg,
            R_a,
            R_prime_a,
            s_prime_a,
            alpha_a,
            H_p_pk,
            I_a,
            I_hat_a,
            T_a,
        })
    }

    pub fn next_message(&self) -> Message0 {
        Message0 {
            pi_a: DleqProof::new(
                ED25519_BASEPOINT_POINT,
                self.T_a,
                self.H_p_pk,
                self.I_hat_a,
                self.alpha_a,
            ),
            c_a: Commitment::new(self.fake_responses, self.I_a, self.I_hat_a, self.T_a),
        }
    }

    pub fn receive(self, msg: Message1) -> Result<Alice1> {
        msg.pi_b
            .verify(ED25519_BASEPOINT_POINT, msg.T_b, self.H_p_pk, msg.I_hat_b)?;

        let (h_last, h_0) = final_challenge(
            self.fake_responses,
            self.ring,
            self.T_a,
            msg.T_b,
            self.R_a,
            self.I_hat_a,
            msg.I_hat_b,
            self.R_prime_a,
            self.I_a,
            msg.I_b,
            self.msg,
        )?;

        let s_0_a = self.alpha_a - h_last * self.s_prime_a;

        Ok(Alice1 {
            fake_responses: self.fake_responses,
            h_0,
            I_b: msg.I_b,
            s_0_a,
            I_a: self.I_a,
            I_hat_a: self.I_hat_a,
            T_a: self.T_a,
        })
    }
}

pub struct Alice1 {
    fake_responses: [Scalar; RING_SIZE - 1],
    I_a: EdwardsPoint,
    I_hat_a: EdwardsPoint,
    T_a: EdwardsPoint,
    h_0: Scalar,
    I_b: EdwardsPoint,
    s_0_a: Scalar,
}

impl Alice1 {
    pub fn next_message(&self) -> Message2 {
        Message2 {
            d_a: Opening::new(self.fake_responses, self.I_a, self.I_hat_a, self.T_a),
            s_0_a: self.s_0_a,
        }
    }

    pub fn receive(self, msg: Message3) -> Alice2 {
        let adaptor_sig = AdaptorSignature {
            s_0_a: self.s_0_a,
            s_0_b: msg.s_0_b,
            fake_responses: self.fake_responses,
            h_0: self.h_0,
            I: self.I_a + self.I_b,
        };

        Alice2 { adaptor_sig }
    }
}

pub struct Alice2 {
    pub adaptor_sig: AdaptorSignature,
}
