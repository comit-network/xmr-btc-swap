#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]

use anyhow::{bail, Result};
use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use hash_edwards_to_edwards::hash_point_to_point;
use rand::rngs::OsRng;
use std::convert::TryInto;
use tiny_keccak::{Hasher, Keccak};

pub mod alice;
pub mod bob;

const RING_SIZE: usize = 11;
const DOMAIN_TAG: &str = "CSLAG_c";

fn challenge(
    s_i: Scalar,
    pk_i: EdwardsPoint,
    h_prev: Scalar,
    I: EdwardsPoint,
    mut prefix: Keccak,
) -> Result<Scalar> {
    let L_i = s_i * ED25519_BASEPOINT_POINT + h_prev * pk_i;

    let H_p_pk_i = hash_point_to_point(pk_i);

    let R_i = s_i * H_p_pk_i + h_prev * I;

    prefix.update(&L_i.compress().as_bytes().to_vec());
    prefix.update(&R_i.compress().as_bytes().to_vec());

    let mut output = [0u8; 64];
    prefix.finalize(&mut output);

    Ok(Scalar::from_bytes_mod_order_wide(&output))
}

#[allow(clippy::too_many_arguments)]
fn final_challenge(
    fake_responses: [Scalar; RING_SIZE - 1],
    ring: [EdwardsPoint; RING_SIZE],
    T_a: EdwardsPoint,
    T_b: EdwardsPoint,
    R_a: EdwardsPoint,
    I_hat_a: EdwardsPoint,
    I_hat_b: EdwardsPoint,
    R_prime_a: EdwardsPoint,
    I_a: EdwardsPoint,
    I_b: EdwardsPoint,
    msg: [u8; 32],
) -> Result<(Scalar, Scalar)> {
    let h_0 = {
        let ring = ring
            .iter()
            .flat_map(|pk| pk.compress().as_bytes().to_vec())
            .collect::<Vec<u8>>();

        let mut keccak = tiny_keccak::Keccak::v512();
        keccak.update(DOMAIN_TAG.as_bytes());
        keccak.update(ring.as_slice());
        keccak.update(&msg);
        keccak.update((T_a + T_b + R_a).compress().as_bytes());
        keccak.update((I_hat_a + I_hat_b + R_prime_a).compress().as_bytes());
        let mut output = [0u8; 64];
        keccak.finalize(&mut output);

        Scalar::from_bytes_mod_order_wide(&output)
    };
    // ring size is 11

    let ring_concat = ring
        .iter()
        .flat_map(|pk| pk.compress().as_bytes().to_vec())
        .collect::<Vec<u8>>();

    let mut keccak = tiny_keccak::Keccak::v512();
    keccak.update(DOMAIN_TAG.as_bytes());
    keccak.update(ring_concat.as_slice());
    keccak.update(&msg);

    let I = I_a + I_b;

    let h_last = fake_responses
        .iter()
        .enumerate()
        .fold(h_0, |h_prev, (i, s_i)| {
            let pk_i = ring[i + 1];
            // TODO: Do not unwrap here
            challenge(*s_i, pk_i, h_prev, I, keccak.clone()).unwrap()
        });

    Ok((h_last, h_0))
}

#[derive(Clone)]
pub struct AdaptorSignature {
    s_0_a: Scalar,
    s_0_b: Scalar,
    fake_responses: [Scalar; RING_SIZE - 1],
    h_0: Scalar,
    /// Key image of the real key in the ring.
    I: EdwardsPoint,
}

impl AdaptorSignature {
    pub fn adapt(self, y: Scalar) -> Signature {
        let r_last = self.s_0_a + self.s_0_b + y;

        let responses = self
            .fake_responses
            .iter()
            .chain([r_last].iter())
            .copied()
            .collect::<Vec<_>>()
            .try_into()
            .expect("correct response size");

        Signature {
            responses,
            h_0: self.h_0,
            I: self.I,
        }
    }
}

pub struct Signature {
    pub responses: [Scalar; RING_SIZE],
    pub h_0: Scalar,
    /// Key image of the real key in the ring.
    pub I: EdwardsPoint,
}

impl Signature {
    #[cfg(test)]
    pub fn verify(&self, ring: [EdwardsPoint; RING_SIZE], msg: &[u8; 32]) -> Result<bool> {
        let ring_concat = ring
            .iter()
            .flat_map(|pk| pk.compress().as_bytes().to_vec())
            .collect::<Vec<u8>>();

        let mut prefix = tiny_keccak::Keccak::v512();
        prefix.update(DOMAIN_TAG.as_bytes());
        prefix.update(ring_concat.as_slice());
        prefix.update(msg);

        let mut h = self.h_0;

        for (i, s_i) in self.responses.iter().enumerate() {
            let pk_i = ring[(i + 1) % RING_SIZE];
            h = challenge(*s_i, pk_i, h, self.I, prefix.clone())?;
        }

        Ok(h == self.h_0)
    }
}

struct DleqProof {
    s: Scalar,
    c: Scalar,
}

impl DleqProof {
    fn new(
        G: EdwardsPoint,
        xG: EdwardsPoint,
        H: EdwardsPoint,
        xH: EdwardsPoint,
        x: Scalar,
    ) -> Self {
        let r = Scalar::random(&mut OsRng);
        let rG = r * G;
        let rH = r * H;

        let mut keccak = tiny_keccak::Keccak::v256();
        keccak.update(G.compress().as_bytes());
        keccak.update(xG.compress().as_bytes());
        keccak.update(H.compress().as_bytes());
        keccak.update(xH.compress().as_bytes());
        keccak.update(rG.compress().as_bytes());
        keccak.update(rH.compress().as_bytes());

        let mut output = [0u8; 32];
        keccak.finalize(&mut output);

        let c = Scalar::from_bytes_mod_order(output);

        let s = r + c * x;

        Self { s, c }
    }

    fn verify(
        &self,
        G: EdwardsPoint,
        xG: EdwardsPoint,
        H: EdwardsPoint,
        xH: EdwardsPoint,
    ) -> Result<()> {
        let s = self.s;
        let c = self.c;

        let rG = (s * G) + (-c * xG);
        let rH = (s * H) + (-c * xH);

        let mut keccak = tiny_keccak::Keccak::v256();
        keccak.update(G.compress().as_bytes());
        keccak.update(xG.compress().as_bytes());
        keccak.update(H.compress().as_bytes());
        keccak.update(xH.compress().as_bytes());
        keccak.update(rG.compress().as_bytes());
        keccak.update(rH.compress().as_bytes());

        let mut output = [0u8; 32];
        keccak.finalize(&mut output);

        let c_prime = Scalar::from_bytes_mod_order(output);

        if c != c_prime {
            bail!("invalid DLEQ proof")
        }

        Ok(())
    }
}

#[derive(PartialEq)]
struct Commitment([u8; 32]);

impl Commitment {
    fn new(
        fake_responses: [Scalar; RING_SIZE - 1],
        I_a: EdwardsPoint,
        I_hat_a: EdwardsPoint,
        T_a: EdwardsPoint,
    ) -> Self {
        let fake_responses = fake_responses
            .iter()
            .flat_map(|r| r.as_bytes().to_vec())
            .collect::<Vec<u8>>();

        let mut keccak = tiny_keccak::Keccak::v256();
        keccak.update(&fake_responses);
        keccak.update(I_a.compress().as_bytes());
        keccak.update(I_hat_a.compress().as_bytes());
        keccak.update(T_a.compress().as_bytes());

        let mut output = [0u8; 32];
        keccak.finalize(&mut output);

        Self(output)
    }
}

struct Opening {
    fake_responses: [Scalar; RING_SIZE - 1],
    I_a: EdwardsPoint,
    I_hat_a: EdwardsPoint,
    T_a: EdwardsPoint,
}

impl Opening {
    fn new(
        fake_responses: [Scalar; RING_SIZE - 1],
        I_a: EdwardsPoint,
        I_hat_a: EdwardsPoint,
        T_a: EdwardsPoint,
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
        EdwardsPoint,
        EdwardsPoint,
        EdwardsPoint,
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
    I_b: EdwardsPoint,
    T_b: EdwardsPoint,
    I_hat_b: EdwardsPoint,
    pi_b: DleqProof,
}

// Alice sends this to Bob
pub struct Message2 {
    d_a: Opening,
    s_0_a: Scalar,
}

// Bob sends this to Alice
#[derive(Clone, Copy)]
pub struct Message3 {
    s_0_b: Scalar,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alice::Alice0;
    use crate::bob::Bob0;

    #[test]
    fn sign_and_verify_success() {
        let msg_to_sign = b"hello world, monero is amazing!!";

        let s_prime_a = Scalar::random(&mut OsRng);
        let s_b = Scalar::random(&mut OsRng);

        let pk = (s_prime_a + s_b) * ED25519_BASEPOINT_POINT;

        let (r_a, R_a, R_prime_a) = {
            let r_a = Scalar::random(&mut OsRng);
            let R_a = r_a * ED25519_BASEPOINT_POINT;

            let pk_hashed_to_point = hash_point_to_point(pk);

            let R_prime_a = r_a * pk_hashed_to_point;

            (r_a, R_a, R_prime_a)
        };

        let mut ring = [EdwardsPoint::default(); RING_SIZE];
        ring[0] = pk;

        ring[1..].fill_with(|| {
            let x = Scalar::random(&mut OsRng);

            x * ED25519_BASEPOINT_POINT
        });

        let alice = Alice0::new(ring, *msg_to_sign, R_a, R_prime_a, s_prime_a).unwrap();
        let bob = Bob0::new(ring, *msg_to_sign, R_a, R_prime_a, s_b).unwrap();

        let msg = alice.next_message();
        let bob = bob.receive(msg);

        let msg = bob.next_message();
        let alice = alice.receive(msg).unwrap();

        let msg = alice.next_message();
        let bob = bob.receive(msg).unwrap();

        let msg = bob.next_message();
        let alice = alice.receive(msg);

        let sig = alice.adaptor_sig.adapt(r_a);

        assert!(sig.verify(ring, msg_to_sign).unwrap());
    }
}
