#![allow(non_snake_case)]

use anyhow::{bail, Result};
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

    let hasher = Sha512::new().chain(bytes);
    let h = Scalar::from_hash(hasher);

    if i >= RING_SIZE - 2 {
        h
    } else {
        final_challenge(i + 1, fake_responses, ring, h, I_a, I_b, msg)
    }
}

pub struct AdaptorSig {
    s_0_a: Scalar,
    s_0_b: Scalar,
    fake_responses: [Scalar; RING_SIZE - 1],
    h_0: Scalar,
    /// Key image of the real key in the ring.
    I: RistrettoPoint,
}

pub struct Alice0 {
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
    pub fn new(
        ring: [RistrettoPoint; RING_SIZE],
        msg: [u8; 32],
        R_a: RistrettoPoint,
        R_prime_a: RistrettoPoint,
        s_prime_a: Scalar,
    ) -> Self {
        let mut fake_responses = [Scalar::zero(); RING_SIZE - 1];
        for response in fake_responses.iter_mut().take(RING_SIZE - 1) {
            *response = Scalar::random(&mut OsRng);
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

    pub fn next_message(&self) -> Message0 {
        let p_k = self.ring.first().unwrap().compress();
        // H_p(p_k)
        let base_key_hashed_to_point: RistrettoPoint =
            RistrettoPoint::hash_from_bytes::<Sha512>(p_k.as_bytes());
        // key image
        let I_a = self.s_prime_a * base_key_hashed_to_point;
        let I_hat_a = self.alpha_a * base_key_hashed_to_point;

        let T_a = self.alpha_a * RISTRETTO_BASEPOINT_POINT;

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

    pub fn receive(self, msg: Message1) -> Result<Alice1> {
        let p_k = self.ring.first().unwrap().compress();
        let base_key_hashed_to_point: RistrettoPoint =
            RistrettoPoint::hash_from_bytes::<Sha512>(p_k.as_bytes());
        msg.pi_b.verify(
            RISTRETTO_BASEPOINT_POINT,
            msg.T_b,
            base_key_hashed_to_point,
            msg.I_hat_b,
        )?;

        let T_a = self.alpha_a * RISTRETTO_BASEPOINT_POINT;
        let I_hat_a = self.alpha_a * base_key_hashed_to_point;

        let h_0 = {
            let h_0 = Sha512::new()
                .chain(self.msg)
                .chain((T_a + msg.T_b + self.R_a).compress().as_bytes())
                .chain(
                    (I_hat_a + msg.I_hat_b + self.R_prime_a)
                        .compress()
                        .as_bytes(),
                );
            Scalar::from_hash(h_0)
        };

        let I_a = self.s_prime_a * base_key_hashed_to_point;
        let h_last = final_challenge(
            1,
            self.fake_responses,
            self.ring,
            h_0,
            I_a,
            msg.I_b,
            self.msg,
        );

        let s_0_a = self.alpha_a - h_last * self.s_prime_a;

        Ok(Alice1 {
            ring: self.ring,
            fake_responses: self.fake_responses,
            msg: self.msg,
            R_a: self.R_a,
            R_prime_a: self.R_prime_a,
            s_prime_a: self.s_prime_a,
            alpha_a: self.alpha_a,
            h_0,
            I_b: msg.I_b,
            s_0_a,
        })
    }
}

pub struct Alice1 {
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
    h_0: Scalar,
    I_b: RistrettoPoint,
    s_0_a: Scalar,
}

impl Alice1 {
    pub fn next_message(&self) -> Message2 {
        let base_key_hashed_to_point: RistrettoPoint = RistrettoPoint::hash_from_bytes::<Sha512>(
            self.ring.first().unwrap().compress().as_bytes(),
        );
        let I_a = self.s_prime_a * base_key_hashed_to_point;
        let T_a = self.alpha_a * RISTRETTO_BASEPOINT_POINT;
        let I_hat_a = self.alpha_a * base_key_hashed_to_point;
        Message2 {
            d_a: Opening::new(self.fake_responses, I_a, I_hat_a, T_a),
            s_0_a: self.s_0_a,
        }
    }

    pub fn receive(self, msg: Message3) -> Alice2 {
        let base_key_hashed_to_point: RistrettoPoint = RistrettoPoint::hash_from_bytes::<Sha512>(
            self.ring.first().unwrap().compress().as_bytes(),
        );
        let I_a = self.s_prime_a * base_key_hashed_to_point;

        let adaptor_sig = AdaptorSig {
            s_0_a: self.s_0_a,
            s_0_b: msg.s_0_b,
            fake_responses: self.fake_responses,
            h_0: self.h_0,
            I: I_a + self.I_b,
        };

        Alice2 { adaptor_sig }
    }
}

pub struct Alice2 {
    pub adaptor_sig: AdaptorSig,
}

pub struct Bob0 {
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
    pub fn new(
        ring: [RistrettoPoint; RING_SIZE],
        msg: [u8; 32],
        R_a: RistrettoPoint,
        R_prime_a: RistrettoPoint,
        s_b: Scalar,
    ) -> Self {
        Bob0 {
            ring,
            msg,
            R_a,
            R_prime_a,
            alpha_b: Scalar::random(&mut OsRng),
            s_b,
        }
    }

    pub fn receive(self, msg: Message0) -> Bob1 {
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

pub struct Bob1 {
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
    pub fn next_message(&self) -> Message1 {
        let p_k = self.ring.first().unwrap().compress();
        // H_p(p_k)
        let base_key_hashed_to_point: RistrettoPoint =
            RistrettoPoint::hash_from_bytes::<Sha512>(p_k.as_bytes());
        // key image
        let I_b = self.s_b * base_key_hashed_to_point;
        let I_hat_b = self.alpha_b * base_key_hashed_to_point;

        let T_b = self.alpha_b * RISTRETTO_BASEPOINT_POINT;

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

    pub fn receive(self, msg: Message2) -> Result<Bob2> {
        let (fake_responses, I_a, I_hat_a, T_a) = msg.d_a.open(self.c_a)?;

        let base_key_hashed_to_point: RistrettoPoint = RistrettoPoint::hash_from_bytes::<Sha512>(
            self.ring.first().unwrap().compress().as_bytes(),
        );

        self.pi_a.verify(
            RISTRETTO_BASEPOINT_POINT,
            T_a,
            base_key_hashed_to_point,
            I_hat_a,
        )?;

        let T_b = self.alpha_b * RISTRETTO_BASEPOINT_POINT;
        let I_hat_b = self.alpha_b * base_key_hashed_to_point;

        let h_0 = {
            let h_0 = Sha512::new()
                .chain(self.msg)
                .chain((T_a + T_b + self.R_a).compress().as_bytes())
                .chain((I_hat_a + I_hat_b + self.R_prime_a).compress().as_bytes());
            Scalar::from_hash(h_0)
        };

        let I_b = self.s_b * base_key_hashed_to_point;
        let h_last = final_challenge(1, fake_responses, self.ring, h_0, I_a, I_b, self.msg);

        let s_0_b = self.alpha_b - h_last * self.s_b;

        let adaptor_sig = AdaptorSig {
            s_0_a: msg.s_0_a,
            s_0_b,
            fake_responses,
            h_0,
            I: I_a + I_b,
        };

        Ok(Bob2 { s_0_b, adaptor_sig })
    }
}

pub struct Bob2 {
    s_0_b: Scalar,
    pub adaptor_sig: AdaptorSig,
}

impl Bob2 {
    pub fn next_message(&self) -> Message3 {
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
        let r = Scalar::random(&mut OsRng);
        let rG = r * G;
        let rH = r * H;

        let hash = Sha512::new()
            .chain(dbg!(G.compress()).as_bytes())
            .chain(dbg!(xG.compress()).as_bytes())
            .chain(dbg!(H.compress()).as_bytes())
            .chain(dbg!(xH.compress()).as_bytes())
            .chain(dbg!(rG.compress()).as_bytes())
            .chain(dbg!(rH.compress()).as_bytes());
        let c = Scalar::from_hash(hash);

        let s = r + c * x;

        Self { s, c }
    }
    fn verify(
        &self,
        G: RistrettoPoint,
        xG: RistrettoPoint,
        H: RistrettoPoint,
        xH: RistrettoPoint,
    ) -> Result<()> {
        let s = self.s;
        let c = self.c;

        let rG = {
            let sG = s * G;

            sG - c * xG
        };

        let rH = {
            let sH = s * H;

            sH - c * xH
        };

        let hash = Sha512::new()
            .chain(dbg!(G.compress()).as_bytes())
            .chain(dbg!(xG.compress()).as_bytes())
            .chain(dbg!(H.compress()).as_bytes())
            .chain(dbg!(xH.compress()).as_bytes())
            .chain(dbg!(rG.compress()).as_bytes())
            .chain(dbg!(rH.compress()).as_bytes());
        let c_prime = Scalar::from_hash(hash);

        if c != c_prime {
            bail!("invalid DLEQ proof")
        }

        Ok(())
    }
}

#[derive(PartialEq)]
struct Commitment([u8; 64]);

impl Commitment {
    fn new(
        fake_responses: [Scalar; RING_SIZE - 1],
        I_a: RistrettoPoint,
        I_hat_a: RistrettoPoint,
        T_a: RistrettoPoint,
    ) -> Self {
        let fake_responses = fake_responses
            .iter()
            .flat_map(|r| r.as_bytes().to_vec())
            .collect::<Vec<u8>>();

        let hash = Sha512::new()
            .chain(fake_responses)
            .chain(I_a.compress().as_bytes())
            .chain(I_hat_a.compress().as_bytes())
            .chain(T_a.compress().as_bytes())
            .finalize();

        let mut commitment = [0u8; 64];
        commitment.copy_from_slice(&hash);

        Self(commitment)
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
        commitment: Commitment,
    ) -> Result<(
        [Scalar; RING_SIZE - 1],
        RistrettoPoint,
        RistrettoPoint,
        RistrettoPoint,
    )> {
        let self_commitment =
            Commitment::new(self.fake_responses, self.I_a, self.I_hat_a, self.T_a);

        if self_commitment == commitment {
            Ok((self.fake_responses, self.I_a, self.I_hat_a, self.T_a))
        } else {
            bail!("opening does not match commitment")
        }
    }
}

// Alice Sends this to Bob
pub struct Message0 {
    c_a: Commitment,
    pi_a: DleqProof,
}

// Bob sends this to ALice
pub struct Message1 {
    I_b: RistrettoPoint,
    T_b: RistrettoPoint,
    I_hat_b: RistrettoPoint,
    pi_b: DleqProof,
}

// Alice sends this to Bob
pub struct Message2 {
    d_a: Opening,
    s_0_a: Scalar,
}

// Bob sends this to Alice
pub struct Message3 {
    s_0_b: Scalar,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_success() {
        let msg = b"hello world, monero is amazing!!";

        let s_prime_a = Scalar::random(&mut OsRng);
        let s_b = Scalar::random(&mut OsRng);

        let pk = (s_prime_a + s_b) * RISTRETTO_BASEPOINT_POINT;

        let (r_a, R_a, R_prime_a) = {
            let r_a = Scalar::random(&mut OsRng);
            let R_a = r_a * RISTRETTO_BASEPOINT_POINT;

            let pk_hashed_to_point: RistrettoPoint =
                RistrettoPoint::hash_from_bytes::<Sha512>(pk.compress().as_bytes());
            let R_prime_a = r_a * pk_hashed_to_point;

            (r_a, R_a, R_prime_a)
        };

        let mut ring = [RistrettoPoint::default(); RING_SIZE];
        ring[0] = pk;

        for member in ring[1..].iter_mut().take(RING_SIZE - 1) {
            *member = RistrettoPoint::random(&mut OsRng);
        }

        let alice = Alice0::new(ring, *msg, R_a, R_prime_a, s_prime_a);
        let bob = Bob0::new(ring, *msg, R_a, R_prime_a, s_b);

        let msg = alice.next_message();
        let bob = bob.receive(msg);

        let msg = bob.next_message();
        let alice = alice.receive(msg).unwrap();

        let msg = alice.next_message();
        let bob = bob.receive(msg).unwrap();

        let msg = bob.next_message();
        let alice = alice.receive(msg);
    }
}
