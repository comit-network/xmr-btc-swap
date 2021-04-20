#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]

// include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

extern "C" {
    fn hash_to_scalar(hash: *const u8, scalar: *mut u8);
    fn hash_to_p3(hash: *const u8, p3: *mut ge_p3);
    fn ge_p3_tobytes(bytes: *mut u8, hash8_p3: *const ge_p3);
}

use anyhow::{bail, Result};
use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use curve25519_dalek::digest::Digest;
use curve25519_dalek::ristretto::RistrettoPoint;
use curve25519_dalek::scalar::Scalar;
use rand::rngs::OsRng;
use sha2::Sha512;
use std::convert::{TryFrom, TryInto};

const RING_SIZE: usize = 11;
const KEY_TAG: &str = "CSLAG_0";
const DOMAIN_TAG: &str = "CSLAG_c";

#[repr(C)]
#[derive(Debug)]
struct ge_p3 {
    X: [i32; 10],
    Y: [i32; 10],
    Z: [i32; 10],
    T: [i32; 10],
}

fn challenge(
    s_i: Scalar,
    pk_i: RistrettoPoint,
    h_prev: Scalar,
    I: RistrettoPoint,
    prefix: Sha512,
) -> Scalar {
    let L_i = s_i * RISTRETTO_BASEPOINT_POINT + h_prev * pk_i;

    let H_p_pk_i = {
        let H_p = Sha512::new()
            .chain(KEY_TAG.to_string())
            .chain(pk_i.compress().as_bytes());
        RistrettoPoint::from_hash(H_p)
    };

    let R_i = s_i * H_p_pk_i + h_prev * I;

    let mut bytes = vec![];
    bytes.append(&mut L_i.compress().as_bytes().to_vec());
    bytes.append(&mut R_i.compress().as_bytes().to_vec());

    let hasher = prefix.chain(bytes);
    Scalar::from_hash(hasher)
}

fn foo(
    fake_responses: [Scalar; RING_SIZE - 1],
    ring: [RistrettoPoint; RING_SIZE],
    T_a: RistrettoPoint,
    T_b: RistrettoPoint,
    R_a: RistrettoPoint,
    I_hat_a: RistrettoPoint,
    I_hat_b: RistrettoPoint,
    R_prime_a: RistrettoPoint,
    I_a: RistrettoPoint,
    I_b: RistrettoPoint,
    msg: [u8; 32],
) -> (Scalar, Scalar) {
    let h_0 = {
        let ring = ring
            .iter()
            .flat_map(|pk| pk.compress().as_bytes().to_vec())
            .collect::<Vec<u8>>();

        let h_0 = Sha512::new()
            .chain(DOMAIN_TAG.to_string())
            .chain(ring)
            .chain(msg)
            .chain((T_a + T_b + R_a).compress().as_bytes())
            .chain((I_hat_a + I_hat_b + R_prime_a).compress().as_bytes());
        Scalar::from_hash(h_0)
    };
    // ring size is 11
    let h_last = final_challenge(
        fake_responses,
        <[RistrettoPoint; 11]>::try_from(ring).unwrap(),
        h_0,
        I_a + I_b,
        msg,
    );

    (h_last, h_0)
}

fn final_challenge(
    fake_responses: [Scalar; RING_SIZE - 1],
    ring: [RistrettoPoint; RING_SIZE],
    h_0: Scalar,
    I: RistrettoPoint,
    msg: [u8; 32],
) -> Scalar {
    let mut ring_concat = ring
        .iter()
        .flat_map(|pk| pk.compress().as_bytes().to_vec())
        .collect::<Vec<u8>>();

    let mut bytes = vec![];

    bytes.append(&mut DOMAIN_TAG.as_bytes().to_vec());
    bytes.append(&mut ring_concat);
    bytes.append(&mut msg.to_vec());

    let prefix = Sha512::default().chain(bytes);

    let mut h = h_0;

    for (i, s_i) in fake_responses.iter().enumerate() {
        let pk_i = ring[i + 1];
        h = challenge(*s_i, pk_i, h, I, prefix.clone());
    }

    h
}

pub struct AdaptorSignature {
    s_0_a: Scalar,
    s_0_b: Scalar,
    fake_responses: [Scalar; RING_SIZE - 1],
    h_0: Scalar,
    /// Key image of the real key in the ring.
    I: RistrettoPoint,
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
    pub I: RistrettoPoint,
}

impl Signature {
    #[cfg(test)]
    pub fn to_nazgul_signature(&self, ring: &[RistrettoPoint; RING_SIZE]) -> nazgul::clsag::CLSAG {
        let ring = ring.iter().map(|pk| vec![*pk]).collect();

        nazgul::clsag::CLSAG {
            challenge: self.h_0,
            responses: self.responses.to_vec(),
            ring,
            key_images: vec![self.I],
        }
    }

    fn verify(&self, ring: [RistrettoPoint; RING_SIZE], msg: &[u8; 32]) -> bool {
        let mut ring_concat = ring
            .iter()
            .flat_map(|pk| pk.compress().as_bytes().to_vec())
            .collect::<Vec<u8>>();

        let mut bytes = vec![];

        bytes.append(&mut DOMAIN_TAG.as_bytes().to_vec());
        bytes.append(&mut ring_concat);
        bytes.append(&mut msg.to_vec());

        let prefix = Sha512::default().chain(bytes);

        let mut h = self.h_0;

        for (i, s_i) in self.responses.iter().enumerate() {
            let pk_i = ring[(i + 1) % RING_SIZE];
            h = challenge(*s_i, pk_i, h, self.I, prefix.clone());
        }

        h == self.h_0
    }
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
    H_p_pk: RistrettoPoint,
    I_a: RistrettoPoint,
    I_hat_a: RistrettoPoint,
    T_a: RistrettoPoint,
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
        let alpha_a = Scalar::random(&mut OsRng);

        let p_k = ring[0].compress();
        let H_p_pk = {
            let H_p = Sha512::new()
                .chain(KEY_TAG.to_string())
                .chain(p_k.as_bytes());
            RistrettoPoint::from_hash(H_p)
        };

        let I_a = s_prime_a * H_p_pk;
        let I_hat_a = alpha_a * H_p_pk;
        let T_a = alpha_a * RISTRETTO_BASEPOINT_POINT;

        Alice0 {
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
        }
    }

    pub fn next_message(&self) -> Message0 {
        Message0 {
            pi_a: DleqProof::new(
                RISTRETTO_BASEPOINT_POINT,
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
            .verify(RISTRETTO_BASEPOINT_POINT, msg.T_b, self.H_p_pk, msg.I_hat_b)?;

        let (h_last, h_0) = foo(
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
        );

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
    I_a: RistrettoPoint,
    I_hat_a: RistrettoPoint,
    T_a: RistrettoPoint,
    h_0: Scalar,
    I_b: RistrettoPoint,
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
    H_p_pk: RistrettoPoint,
    I_b: RistrettoPoint,
    I_hat_b: RistrettoPoint,
    T_b: RistrettoPoint,
}

impl Bob0 {
    pub fn new(
        ring: [RistrettoPoint; RING_SIZE],
        msg: [u8; 32],
        R_a: RistrettoPoint,
        R_prime_a: RistrettoPoint,
        s_b: Scalar,
    ) -> Self {
        let alpha_b = Scalar::random(&mut OsRng);

        let p_k = ring.first().unwrap().compress();
        let H_p_pk = {
            let H_p = Sha512::new()
                .chain(KEY_TAG.to_string())
                .chain(p_k.as_bytes());
            RistrettoPoint::from_hash(H_p)
        };

        let I_b = s_b * H_p_pk;
        let I_hat_b = alpha_b * H_p_pk;
        let T_b = alpha_b * RISTRETTO_BASEPOINT_POINT;

        Bob0 {
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
    ring: [RistrettoPoint; RING_SIZE],
    msg: [u8; 32],
    // encryption key
    R_a: RistrettoPoint,
    // R'a = r_a*H_p(p_k) where p_k is the signing public key
    R_prime_a: RistrettoPoint,
    s_b: Scalar,
    // secret value:
    alpha_b: Scalar,
    H_p_pk: RistrettoPoint,
    I_b: RistrettoPoint,
    I_hat_b: RistrettoPoint,
    T_b: RistrettoPoint,
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
                RISTRETTO_BASEPOINT_POINT,
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
            .verify(RISTRETTO_BASEPOINT_POINT, T_a, self.H_p_pk, I_hat_a)?;

        let (h_last, h_0) = foo(
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
        );

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
            .chain(G.compress().as_bytes())
            .chain(xG.compress().as_bytes())
            .chain(H.compress().as_bytes())
            .chain(xH.compress().as_bytes())
            .chain(rG.compress().as_bytes())
            .chain(rH.compress().as_bytes());
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

        let rG = (s * G) + (-c * xG);
        let rH = (s * H) + (-c * xH);

        let hash = Sha512::new()
            .chain(G.compress().as_bytes())
            .chain(xG.compress().as_bytes())
            .chain(H.compress().as_bytes())
            .chain(xH.compress().as_bytes())
            .chain(rG.compress().as_bytes())
            .chain(rH.compress().as_bytes());
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
#[derive(Clone, Copy)]
pub struct Message3 {
    s_0_b: Scalar,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_success() {
        let msg_to_sign = b"hello world, monero is amazing!!";

        let s_prime_a = Scalar::random(&mut OsRng);
        let s_b = Scalar::random(&mut OsRng);

        let pk = (s_prime_a + s_b) * RISTRETTO_BASEPOINT_POINT;

        let (r_a, R_a, R_prime_a) = {
            let r_a = Scalar::random(&mut OsRng);
            let R_a = r_a * RISTRETTO_BASEPOINT_POINT;

            let pk_hashed_to_point = {
                let H_p = Sha512::new()
                    .chain(KEY_TAG.to_string())
                    .chain(pk.compress().as_bytes());
                RistrettoPoint::from_hash(H_p)
            };

            let R_prime_a = r_a * pk_hashed_to_point;

            (r_a, R_a, R_prime_a)
        };

        let mut ring = [RistrettoPoint::default(); RING_SIZE];
        ring[0] = pk;

        ring[1..].fill_with(|| RistrettoPoint::random(&mut OsRng));

        let alice = Alice0::new(ring, *msg_to_sign, R_a, R_prime_a, s_prime_a);
        let bob = Bob0::new(ring, *msg_to_sign, R_a, R_prime_a, s_b);

        let msg = alice.next_message();
        let bob = bob.receive(msg);

        let msg = bob.next_message();
        let alice = alice.receive(msg).unwrap();

        let msg = alice.next_message();
        let bob = bob.receive(msg).unwrap();

        let msg = bob.next_message();
        let alice = alice.receive(msg);

        let sig = alice.adaptor_sig.adapt(r_a);

        assert!(sig.verify(ring, msg_to_sign));
    }
}

#[cfg(test)]
mod tests2 {
    use super::*;
    use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};

    #[test]
    fn test_hash_to_scalar() {
        let mut scalar = [0u8; 32];

        let input = "0b6a0ae839214674e9b275aa1986c6352ec7ec6c4ae583ab5a62b947a9dee972";
        let decoded_input = hex::decode(input).unwrap();

        unsafe { hash_to_scalar(decoded_input.as_ptr() as *const u8, &mut scalar as *mut u8) };

        let scalar = Scalar::from_bytes_mod_order(scalar);
        let scalar_hex = hex::encode(scalar.as_bytes());

        assert_eq!(
            scalar_hex,
            "24f9167e1a3eaab18119c225577f0ecc7a488a309e54e2721cbaea62c3db3a06"
        );
    }

    #[test]
    fn test_hash_to_p3() {
        // not zero assertion fails
        // let input =
        // "83efb774657700e37291f4b8dd10c839d1c739fd135c07a2fd7382334dafdd6a";
        // let decoded_input = hex::decode(input).unwrap();

        let input = "a7fbdeeccb597c2d5fdaf2ea2e10cbfcd26b5740903e7f6d46bcbf9a90384fc6";
        let decoded_input = hex::decode(input).unwrap();

        let mut p3 = ge_p3 {
            X: [0; 10],
            Y: [0; 10],
            Z: [0; 10],
            T: [0; 10],
        };

        let mut compressed = [0u8; 32];

        unsafe {
            hash_to_p3(decoded_input.as_ptr() as *const u8, &mut p3);
            dbg!(&p3);
            ge_p3_tobytes(&mut compressed as *mut u8, &p3);
        };

        dbg!(&compressed);

        let actual = CompressedEdwardsY::from_slice(&compressed[..]);

        let decoded =
            hex::decode("f055ba2d0d9828ce2e203d9896bfda494d7830e7e3a27fa27d5eaa825a79a19c")
                .unwrap();
        let expected = CompressedEdwardsY::from_slice(decoded.as_slice());

        assert_eq!(expected, actual);
    }
}
