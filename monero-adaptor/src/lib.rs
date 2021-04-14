use anyhow::Result;
use curve25519_dalek;
use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use curve25519_dalek::digest::Digest;
use curve25519_dalek::ristretto::RistrettoPoint;
use curve25519_dalek::scalar::Scalar;
use rand::rngs::OsRng;
use sha2::Sha512;

const RING_SIZE: usize = 11;

fn final_challenge(
    i: usize,
    fake_responses: [Scalar; RING_SIZE - 1],
    ring: [RistrettoPoint; RING_SIZE],
    h_prev: Scalar,
    I_a: RistrettoPoint,
    I_b: RistrettoPoint,
    msg: [u8; 32],
) -> Scalar {
    let L = fake_responses[i] * RISTRETTO_BASEPOINT_POINT + h_prev * ring[i];

    let H_pk_i: RistrettoPoint =
        RistrettoPoint::hash_from_bytes::<Sha512>(ring[i].compress().as_bytes());

    let I = I_a + I_b;
    let R = fake_responses[i] * H_pk_i + I;

    let mut bytes = vec![];

    // todo: add tag and ring
    bytes.append(&mut msg.to_vec());
    bytes.append(&mut L.compress().as_bytes().to_vec());
    bytes.append(&mut R.compress().as_bytes().to_vec());

    let mut hasher = Sha512::new().chain(bytes);
    let h = Scalar::from_hash(hasher);

    if i >= RING_SIZE - 2 {
        h
    } else {
        final_challenge(i + 1, fake_responses, ring, h, I_a, I_b, msg)
    }
}

struct AdaptorSig;

fn adaptor_sig(
    s_0_a: Scalar,
    s_0_b: Scalar,
    h_0: Scalar,
    pk: RistrettoPoint,
    I: RistrettoPoint,
) -> AdaptorSig {
    let s_prime_0 = s_0_a + s_0_b;
    let l_0 = s_prime_0 * RISTRETTO_BASEPOINT_POINT + h_0 * pk;

    let H_pk: RistrettoPoint = RistrettoPoint::hash_from_bytes::<Sha512>(pk.compress().as_bytes());
    let r_0 = s_prime_0 * H_pk + h_0 * I;

    AdaptorSig
}

struct Alice0 {
    // secret index is always 0
    ring: [RistrettoPoint; RING_SIZE],
    fake_responses: [Scalar; RING_SIZE - 1],
    msg: [u8; 32],
    // encryption key
    R_a: RistrettoPoint,
    // R'a = r_a*H_p(p_k) where p_k is the signing public key
    R_prime_a: RistrettoPoint,
    // this is not s_a cos of something to with one-time-address??
    s_prime_a: Scalar,
    // secret value:
    alpha_a: Scalar,
}

impl Alice0 {
    fn new(
        ring: [RistrettoPoint; RING_SIZE],
        msg: [u8; 32],
        R_a: RistrettoPoint,
        R_prime_a: RistrettoPoint,
        s_prime_a: Scalar,
    ) -> Self {
        let mut fake_responses = [Scalar::zero(); RING_SIZE - 1];

        for i in 0..(RING_SIZE - 1) {
            fake_responses[i] = Scalar::random(&mut OsRng);
        }

        Alice0 {
            ring,
            fake_responses,
            msg,
            R_a,
            R_prime_a,
            s_prime_a,
            alpha_a: Scalar::random(&mut OsRng),
        }
    }
    fn next_message(&self) -> Message0 {
        let p_k = self.ring.first().unwrap().compress();
        // H_p(p_k)
        let base_key_hashed_to_point: RistrettoPoint =
            RistrettoPoint::hash_from_bytes::<Sha512>(p_k.as_bytes());
        // key image
        let I_a = self.s_prime_a * base_key_hashed_to_point;
        let I_hat_a = self.alpha_a * base_key_hashed_to_point;

        let T_a = self.s_prime_a * RISTRETTO_BASEPOINT_POINT;

        Message0 {
            pi_a: DleqProof::new(
                RISTRETTO_BASEPOINT_POINT,
                T_a,
                base_key_hashed_to_point,
                I_hat_a,
                self.s_prime_a,
            ),
            c_a: Commitment::new(self.fake_responses, I_a, I_hat_a, T_a),
        }
    }

    fn receive(self, msg: Message1) -> Result<Alice1> {
        let p_k = self.ring.first().unwrap().compress();
        let base_key_hashed_to_point: RistrettoPoint =
            RistrettoPoint::hash_from_bytes::<Sha512>(p_k.as_bytes());
        msg.pi_b.verify(
            RISTRETTO_BASEPOINT_POINT,
            msg.T_b,
            base_key_hashed_to_point,
            msg.I_hat_b,
        )?;

        let I_a = self.s_prime_a * base_key_hashed_to_point;
        let T_a = self.s_prime_a * RISTRETTO_BASEPOINT_POINT;

        let h_1 = {
            Sha512::new()
                .chain(self.msg)
                .chain((T_a + msg.T_b + self.R_a).compress().as_bytes())
                .chain((I_a + msg.I_b + self.R_prime_a).compress().as_bytes())
        };

        let h_0 = final_challenge(
            1,
            self.fake_responses,
            self.ring,
            Scalar::from_hash(h_1),
            I_a,
            msg.I_b,
            self.msg,
        );

        let s_0_a = self.alpha_a - h_0 * self.s_prime_a;

        Ok(Alice1 {
            ring: self.ring,
            fake_responses: self.fake_responses,
            msg: self.msg,
            R_a: self.R_a,
            R_prime_a: self.R_prime_a,
            s_prime_a: self.s_prime_a,
            alpha_a: self.alpha_a,
            s_0_a,
        })
    }
}

struct Alice1 {
    // secret index is always 0
    ring: [RistrettoPoint; RING_SIZE],
    fake_responses: [Scalar; RING_SIZE - 1],
    msg: [u8; 32],
    // encryption key
    R_a: RistrettoPoint,
    // R'a = r_a*H_p(p_k) where p_k is the signing public key
    R_prime_a: RistrettoPoint,
    // this is not s_a cos of something to with one-time-address??
    s_prime_a: Scalar,
    // secret value:
    alpha_a: Scalar,
    s_0_a: Scalar,
}

impl Alice1 {
    fn next_message(&self) -> Message2 {
        let base_key_hashed_to_point: RistrettoPoint = RistrettoPoint::hash_from_bytes::<Sha512>(
            self.ring.first().unwrap().compress().as_bytes(),
        );
        let I_a = self.s_prime_a * base_key_hashed_to_point;
        let T_a = self.s_prime_a * RISTRETTO_BASEPOINT_POINT;
        let I_hat_a = self.alpha_a * base_key_hashed_to_point;
        Message2 {
            d_a: Opening::new(self.fake_responses, I_a, I_hat_a, T_a),
            s_0_a: self.s_0_a,
        }
    }
}

struct Bob0 {
    // secret index is always 0
    ring: [RistrettoPoint; RING_SIZE],
    msg: [u8; 32],
    // encryption key
    R_a: RistrettoPoint,
    // R'a = r_a*H_p(p_k) where p_k is the signing public key
    R_prime_a: RistrettoPoint,
    s_b: Scalar,
    // secret value:
    alpha_b: Scalar,
}

impl Bob0 {
    fn new(
        ring: [RistrettoPoint; RING_SIZE],
        msg: [u8; 32],
        R_b: RistrettoPoint,
        R_prime_b: RistrettoPoint,
        s_b: Scalar,
    ) -> Self {
        Bob0 {
            ring,
            msg,
            R_a: R_b,
            R_prime_a: R_prime_b,
            alpha_b: Scalar::random(&mut OsRng),
            s_b,
        }
    }

    fn receive(self, msg: Message0) -> Bob1 {
        Bob1 {
            ring: self.ring,
            msg: self.msg,
            R_a: self.R_a,
            R_prime_a: self.R_prime_a,
            s_b: self.s_b,
            alpha_b: self.alpha_b,
            pi_a: msg.pi_a,
            c_a: msg.c_a,
        }
    }
}

struct Bob1 {
    // secret index is always 0
    ring: [RistrettoPoint; RING_SIZE],
    msg: [u8; 32],
    // encryption key
    R_a: RistrettoPoint,
    // R'a = r_a*H_p(p_k) where p_k is the signing public key
    R_prime_a: RistrettoPoint,
    s_b: Scalar,
    // secret value:
    alpha_b: Scalar,
    pi_a: DleqProof,
    c_a: Commitment,
}

impl Bob1 {
    fn next_message(&self) -> Message1 {
        let p_k = self.ring.first().unwrap().compress();
        // H_p(p_k)
        let base_key_hashed_to_point: RistrettoPoint =
            RistrettoPoint::hash_from_bytes::<Sha512>(p_k.as_bytes());
        // key image
        let I_b = self.s_b * base_key_hashed_to_point;
        let I_hat_b = self.alpha_b * base_key_hashed_to_point;

        let T_b = self.s_b * RISTRETTO_BASEPOINT_POINT;

        Message1 {
            I_b,
            T_b,
            I_hat_b,
            pi_b: DleqProof::new(
                RISTRETTO_BASEPOINT_POINT,
                T_b,
                base_key_hashed_to_point,
                I_hat_b,
                self.s_b,
            ),
        }
    }

    fn receive(self, msg: Message2) -> Result<Bob2> {
        let (fake_responses, I_a, I_hat_a, T_a) = msg.d_a.open(self.c_a)?;

        let base_key_hashed_to_point: RistrettoPoint = RistrettoPoint::hash_from_bytes::<Sha512>(
            self.ring.first().unwrap().compress().as_bytes(),
        );

        let I_b = self.s_b * base_key_hashed_to_point;
        let T_b = self.s_b * RISTRETTO_BASEPOINT_POINT;

        let h_1 = {
            Sha512::new()
                .chain(self.msg)
                .chain((T_a + T_b + self.R_a).compress().as_bytes())
                .chain((I_a + I_b + self.R_prime_a).compress().as_bytes())
        };

        let h_0 = final_challenge(
            1,
            fake_responses,
            self.ring,
            Scalar::from_hash(h_1),
            I_a,
            I_b,
            self.msg,
        );

        let s_0_b = self.alpha_b - h_0 * self.s_b;

        Ok(Bob2 {
            ring: self.ring,
            msg: self.msg,
            R_b: self.R_a,
            R_prime_b: self.R_prime_a,
            s_b: self.s_b,
            alpha_b: self.alpha_b,
            pi_a: self.pi_a,
            fake_responses,
            I_a: I_b,
            I_hat_a,
            T_a: T_b,
            s_0_a: msg.s_0_a,
            s_0_b,
        })
    }
}

struct Bob2 {
    // secret index is always 0
    ring: [RistrettoPoint; RING_SIZE],
    msg: [u8; 32],
    // encryption key
    R_b: RistrettoPoint,
    // R'a = r_a*H_p(p_k) where p_k is the signing public key
    R_prime_b: RistrettoPoint,
    s_b: Scalar,
    alpha_b: Scalar,
    // secret value:
    s_0_b: Scalar,
    s_0_a: Scalar,
    pi_a: DleqProof,
    fake_responses: [Scalar; RING_SIZE - 1],
    I_a: RistrettoPoint,
    I_hat_a: RistrettoPoint,
    T_a: RistrettoPoint,
}

impl Bob2 {
    fn next_message(&self) -> Message3 {
        Message3 { s_0_b: self.s_0_b }
    }
}
struct DleqProof {
    s: Scalar,
    c: Scalar,
}

impl DleqProof {
    fn new(
        G: RistrettoPoint,
        xG: RistrettoPoint,
        H: RistrettoPoint,
        xH: RistrettoPoint,
        x: Scalar,
    ) -> Self {
        todo!()
    }
    fn verify(
        &self,
        G: RistrettoPoint,
        xG: RistrettoPoint,
        H: RistrettoPoint,
        xH: RistrettoPoint,
    ) -> Result<()> {
        todo!()
    }
}

struct Commitment([u8; 32]);

impl Commitment {
    fn new(
        fake_responses: [Scalar; RING_SIZE - 1],
        I_a: RistrettoPoint,
        I_hat_a: RistrettoPoint,
        T_a: RistrettoPoint,
    ) -> Self {
        todo!()
    }
}

struct Opening {
    fake_responses: [Scalar; RING_SIZE - 1],
    I_a: RistrettoPoint,
    I_hat_a: RistrettoPoint,
    T_a: RistrettoPoint,
}

impl Opening {
    fn new(
        fake_responses: [Scalar; RING_SIZE - 1],
        I_a: RistrettoPoint,
        I_hat_a: RistrettoPoint,
        T_a: RistrettoPoint,
    ) -> Self {
        Self {
            fake_responses,
            I_a,
            I_hat_a,
            T_a,
        }
    }
    fn open(
        self,
        c_a: Commitment,
    ) -> Result<(
        [Scalar; RING_SIZE - 1],
        RistrettoPoint,
        RistrettoPoint,
        RistrettoPoint,
    )> {
        Ok((todo!()))
    }
}

// Alice Sends this to Bob
struct Message0 {
    c_a: Commitment,
    pi_a: DleqProof,
}

// Bob sends this to ALice
struct Message1 {
    I_b: RistrettoPoint,
    T_b: RistrettoPoint,
    I_hat_b: RistrettoPoint,
    pi_b: DleqProof,
}

// Alice sends this to Bob
struct Message2 {
    d_a: Opening,
    s_0_a: Scalar,
}

// Bob sends this to Alice
struct Message3 {
    s_0_b: Scalar,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_success() {
        let mut fake_responses = [Scalar::random(&mut OsRng); RING_SIZE - 1];
        dbg!(fake_responses);
    }
}
