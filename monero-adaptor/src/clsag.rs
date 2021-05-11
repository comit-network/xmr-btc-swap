use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use hash_edwards_to_edwards::hash_point_to_point;
use std::convert::TryInto;
use std::ops::{Index, Sub};

pub const RING_SIZE: usize = 11;

const INV_EIGHT: Scalar = Scalar::from_bits([
    121, 47, 220, 226, 41, 229, 6, 97, 208, 218, 28, 125, 179, 157, 211, 7, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 6,
]);

pub fn sign(
    msg: &[u8],
    signing_key: Scalar,
    H_p_pk: EdwardsPoint,
    alpha: Scalar,
    ring: &[EdwardsPoint; RING_SIZE],
    commitment_ring: &[EdwardsPoint; RING_SIZE],
    fake_responses: [Scalar; RING_SIZE - 1],
    z: Scalar,
    pseudo_output_commitment: EdwardsPoint,
    L_0: EdwardsPoint,
    R_0: EdwardsPoint,
    I: EdwardsPoint,
) -> Signature {
    let D = z * H_p_pk;
    let D_inv_8 = D * INV_EIGHT;
    let ring = Ring::new(ring);
    let commitment_ring = Ring::new(commitment_ring);

    let mu_P = hash_to_scalar!(
        b"CLSAG_agg_0" || ring || commitment_ring || I || H_p_pk || pseudo_output_commitment
    );
    let mu_C = hash_to_scalar!(
        b"CLSAG_agg_1" || ring || commitment_ring || I || H_p_pk || pseudo_output_commitment
    );

    let compute_ring_element = |L: EdwardsPoint, R: EdwardsPoint| {
        hash_to_scalar!(
            b"CLSAG_round" || ring || commitment_ring || pseudo_output_commitment || msg || L || R
        )
    };

    let h_0 = compute_ring_element(L_0, R_0);
    let adjusted_commitment_ring = &commitment_ring - pseudo_output_commitment;

    let h_last = fake_responses
        .iter()
        .enumerate()
        .fold(h_0, |h_prev, (i, s_i)| {
            let pk_i = ring[i + 1];

            let L_i = compute_L(h_prev, mu_P, mu_C, *s_i, pk_i, adjusted_commitment_ring[i]);
            let R_i = compute_R(h_prev, mu_P, mu_C, pk_i, *s_i, I, D_inv_8);

            compute_ring_element(L_i, R_i)
        });

    let s_last = alpha - h_last * ((mu_P * signing_key) + (mu_C * z));

    Signature {
        responses: [
            fake_responses[0],
            fake_responses[1],
            fake_responses[2],
            fake_responses[3],
            fake_responses[4],
            fake_responses[5],
            fake_responses[6],
            fake_responses[7],
            fake_responses[8],
            fake_responses[9],
            s_last,
        ],
        h_0,
        I,
        D,
    }
}

#[must_use]
pub fn verify(
    &Signature {
        I,
        h_0,
        D,
        responses,
        ..
    }: &Signature,
    msg: &[u8],
    ring: &[EdwardsPoint; RING_SIZE],
    commitment_ring: &[EdwardsPoint; RING_SIZE],
    pseudo_output_commitment: EdwardsPoint,
    H_p_pk: EdwardsPoint,
) -> bool {
    let ring = Ring::new(ring);
    let commitment_ring = Ring::new(commitment_ring);

    let mu_P = hash_to_scalar!(
        b"CLSAG_agg_0" || ring || commitment_ring || I || H_p_pk || pseudo_output_commitment
    );
    let mu_C = hash_to_scalar!(
        b"CLSAG_agg_1" || ring || commitment_ring || I || H_p_pk || pseudo_output_commitment
    );
    let adjusted_commitment_ring = &commitment_ring - pseudo_output_commitment;

    let mut h = h_0;

    for (i, s_i) in responses.iter().enumerate() {
        let pk_i = ring[(i + 1) % RING_SIZE];

        let L_i = compute_L(h, mu_P, mu_C, *s_i, pk_i, adjusted_commitment_ring[i]);
        let R_i = compute_R(h, mu_P, mu_C, pk_i, *s_i, I, D);

        h = hash_to_scalar!(
            b"CLSAG_round"
                || ring
                || commitment_ring
                || pseudo_output_commitment
                || msg
                || L_i
                || R_i
        );
    }

    h == h_0
}

pub struct Signature {
    pub responses: [Scalar; RING_SIZE],
    pub h_0: Scalar,
    /// Key image of the real key in the ring.
    pub I: EdwardsPoint,
    pub D: EdwardsPoint,
}

// L_i = s_i * G + c_p * pk_i + c_c * (commitment_i - pseudoutcommitment)
fn compute_L(
    h_prev: Scalar,
    mu_P: Scalar,
    mu_C: Scalar,
    s_i: Scalar,
    pk_i: EdwardsPoint,
    adjusted_commitment_i: EdwardsPoint,
) -> EdwardsPoint {
    let c_p = h_prev * mu_P;
    let c_c = h_prev * mu_C;

    (s_i * ED25519_BASEPOINT_POINT) + (c_p * pk_i) + c_c * adjusted_commitment_i
}

// R_i = s_i * H_p_pk_i + c_p * I + c_c * (z * hash_to_point(signing pk))
fn compute_R(
    h_prev: Scalar,
    mu_P: Scalar,
    mu_C: Scalar,
    pk_i: EdwardsPoint,
    s_i: Scalar,
    I: EdwardsPoint,
    D: EdwardsPoint,
) -> EdwardsPoint {
    let c_p = h_prev * mu_P;
    let c_c = h_prev * mu_C;

    let H_p_pk_i = hash_point_to_point(pk_i);

    (s_i * H_p_pk_i) + (c_p * I) + c_c * D
}

impl From<Signature> for monero::util::ringct::Clsag {
    fn from(from: Signature) -> Self {
        Self {
            s: from
                .responses
                .iter()
                .map(|s| monero::util::ringct::Key { key: s.to_bytes() })
                .collect(),
            c1: monero::util::ringct::Key {
                key: from.h_0.to_bytes(),
            },
            D: monero::util::ringct::Key {
                key: from.D.compress().to_bytes(),
            },
        }
    }
}

#[derive(Clone)]
pub(crate) struct Ring<'a> {
    points: &'a [EdwardsPoint; 11],
    bytes: [u8; 32 * 11],
}

impl<'a> Ring<'a> {
    fn new(points: &[EdwardsPoint; 11]) -> Ring<'_> {
        let mut bytes = [0u8; 32 * 11];

        for (i, element) in points.iter().enumerate() {
            let start = i * 32;
            let end = (i + 1) * 32;

            bytes[start..end].copy_from_slice(element.compress().as_bytes());
        }

        Ring { points, bytes }
    }
}

impl<'a, 'b> Sub<EdwardsPoint> for &'b Ring<'a> {
    type Output = [EdwardsPoint; 11];

    fn sub(self, rhs: EdwardsPoint) -> Self::Output {
        self.points
            .iter()
            .map(|point| point - rhs)
            .collect::<Vec<_>>()
            .try_into()
            .expect("arrays have same length")
    }
}

impl<'a> AsRef<[u8]> for Ring<'a> {
    fn as_ref(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}

impl<'a> Index<usize> for Ring<'a> {
    type Output = EdwardsPoint;

    fn index(&self, index: usize) -> &Self::Output {
        &self.points[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn const_is_inv_eight() {
        let inv_eight = Scalar::from(8u8).invert();

        assert_eq!(inv_eight, INV_EIGHT);
    }

    #[test]
    fn sign_and_verify() {
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

        let mut commitment_ring = [EdwardsPoint::default(); RING_SIZE];

        let real_commitment_blinding = Scalar::random(&mut OsRng);
        commitment_ring[0] = real_commitment_blinding * ED25519_BASEPOINT_POINT; // + 0 * H
        commitment_ring[1..].fill_with(|| {
            let x = Scalar::random(&mut OsRng);
            x * ED25519_BASEPOINT_POINT
        });

        // TODO: document
        let pseudo_output_commitment = commitment_ring[0];

        let signature = sign(
            msg_to_sign,
            s_prime_a,
            todo!(),
            todo!(),
            &ring,
            &commitment_ring,
            todo!(),
            todo!(),
            pseudo_output_commitment,
            todo!(),
            todo!(),
            todo!(),
        );

        assert!(verify(
            &signature,
            msg_to_sign,
            &ring,
            &commitment_ring,
            pseudo_output_commitment,
            todo!()
        ))
    }
}
