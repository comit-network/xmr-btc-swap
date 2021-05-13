use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use hash_edwards_to_edwards::hash_point_to_point;
use std::iter::{Cycle, Skip, Take};

pub const RING_SIZE: usize = 11;

const INV_EIGHT: Scalar = Scalar::from_bits([
    121, 47, 220, 226, 41, 229, 6, 97, 208, 218, 28, 125, 179, 157, 211, 7, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 6,
]);

pub fn sign(
    msg: &[u8; 32],
    H_p_pk: EdwardsPoint,
    alpha: Scalar,
    ring: &[EdwardsPoint; RING_SIZE],
    commitment_ring: &[EdwardsPoint; RING_SIZE],
    mut responses: [Scalar; RING_SIZE],
    signing_key_index: usize,
    z: Scalar,
    pseudo_output_commitment: EdwardsPoint,
    L: EdwardsPoint,
    R: EdwardsPoint,
    I: EdwardsPoint,
) -> Signature {
    let D = z * H_p_pk;
    let D_inv_8 = D * INV_EIGHT;

    let mu_P = hash_to_scalar!(
        b"CLSAG_agg_0" || ring || commitment_ring || I || D_inv_8 || pseudo_output_commitment
    );
    let mu_C = hash_to_scalar!(
        b"CLSAG_agg_1" || ring || commitment_ring || I || D_inv_8 || pseudo_output_commitment
    );

    let adjusted_commitment_ring = commitment_ring.map(|point| point - pseudo_output_commitment);

    let compute_ring_element = |L: EdwardsPoint, R: EdwardsPoint| {
        hash_to_scalar!(
            b"CLSAG_round" || ring || commitment_ring || pseudo_output_commitment || msg || L || R
        )
    };

    let h_signing_index = compute_ring_element(L, R);

    let mut h_prev = h_signing_index;
    let mut i = (signing_key_index + 1) % RING_SIZE;
    let mut h_0 = Scalar::zero();

    if i == 0 {
        h_0 = h_signing_index
    }

    while i != signing_key_index {
        let L_i = compute_L(
            h_prev,
            mu_P,
            mu_C,
            responses[i],
            ring[i],
            adjusted_commitment_ring[i],
        );
        let R_i = compute_R(h_prev, mu_P, mu_C, responses[i], ring[i], I, D);

        let h = compute_ring_element(L_i, R_i);

        i = (i + 1) % RING_SIZE;
        if i == 0 {
            h_0 = h
        }

        h_prev = h
    }

    responses[signing_key_index] =
        alpha - h_prev * ((mu_P * responses[signing_key_index]) + (mu_C * z));

    Signature {
        responses,
        h_0,
        I,
        D: D_inv_8,
    }
}

fn sign2(
    msg: &[u8; 32],
    H_p_pk: EdwardsPoint,
    alpha: Scalar,
    ring: &[EdwardsPoint; RING_SIZE],
    commitment_ring: &[EdwardsPoint; RING_SIZE],
    fake_responses: [Scalar; RING_SIZE - 1],
    signing_key: Scalar,
    signing_key_index: usize,
    z: Scalar,
    pseudo_output_commitment: EdwardsPoint,
    L: EdwardsPoint,
    R: EdwardsPoint,
    I: EdwardsPoint,
) -> Signature {
    let D = z * H_p_pk;
    let D_inv_8 = D * INV_EIGHT;

    let mu_P = hash_to_scalar!(
        b"CLSAG_agg_0" || ring || commitment_ring || I || D_inv_8 || pseudo_output_commitment
    );
    let mu_C = hash_to_scalar!(
        b"CLSAG_agg_1" || ring || commitment_ring || I || D_inv_8 || pseudo_output_commitment
    );

    let adjusted_commitment_ring = commitment_ring.map(|point| point - pseudo_output_commitment);

    let compute_ring_element = |L: EdwardsPoint, R: EdwardsPoint| {
        hash_to_scalar!(
            b"CLSAG_round" || ring || commitment_ring || pseudo_output_commitment || msg || L || R
        )
    };

    let h_signing_index = compute_ring_element(L, R);

    let mut h_prev = h_signing_index;
    let mut i = (signing_key_index + 1) % RING_SIZE;
    let mut h_0 = Scalar::zero();

    if i == 0 {
        h_0 = h_signing_index
    }

    let mut responses = [Scalar::zero(); 11];

    while i != signing_key_index {
        let s_i = fake_responses[i % 10];
        responses[i] = s_i;

        let L_i = compute_L(
            h_prev,
            mu_P,
            mu_C,
            s_i,
            ring[i],
            adjusted_commitment_ring[i],
        );
        let R_i = compute_R(h_prev, mu_P, mu_C, s_i, ring[i], I, D);

        let h = compute_ring_element(L_i, R_i);

        i = (i + 1) % RING_SIZE;
        if i == 0 {
            h_0 = h
        }

        h_prev = h
    }

    responses[signing_key_index] = alpha - h_prev * ((mu_P * signing_key) + (mu_C * z));

    Signature {
        responses,
        h_0,
        I,
        D: D_inv_8,
    }
}

#[must_use]
pub fn verify(
    &Signature {
        I,
        h_0,
        D: D_inv_8,
        responses,
        ..
    }: &Signature,
    msg: &[u8; 32],
    ring: &[EdwardsPoint; RING_SIZE],
    commitment_ring: &[EdwardsPoint; RING_SIZE],
    pseudo_output_commitment: EdwardsPoint,
) -> bool {
    let D = D_inv_8 * Scalar::from(8u8);

    let mu_P = hash_to_scalar!(
        b"CLSAG_agg_0" || ring || commitment_ring || I || D_inv_8 || pseudo_output_commitment
    );
    let mu_C = hash_to_scalar!(
        b"CLSAG_agg_1" || ring || commitment_ring || I || D_inv_8 || pseudo_output_commitment
    );

    let adjusted_commitment_ring = commitment_ring.map(|point| point - pseudo_output_commitment);

    let h_0_computed = itertools::izip!(responses, ring, adjusted_commitment_ring).fold(
        h_0,
        |h, (s_i, pk_i, adjusted_commitment_i)| {
            let L_i = compute_L(h, mu_P, mu_C, s_i, *pk_i, adjusted_commitment_i);
            let R_i = compute_R(h, mu_P, mu_C, s_i, *pk_i, I, D);

            hash_to_scalar!(
                b"CLSAG_round"
                    || ring
                    || commitment_ring
                    || pseudo_output_commitment
                    || msg
                    || L_i
                    || R_i
            )
        },
    );

    h_0_computed == h_0
}

#[derive(Clone, Debug, PartialEq)]
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
    s_i: Scalar,
    pk_i: EdwardsPoint,
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

trait IteratorExt {
    fn shift_by(self, num: usize) -> ShiftBy<Self>
    where
        Self: ExactSizeIterator + Sized + Clone,
    {
        let length = self.len();

        ShiftBy::new(self, num, length)
    }
}

struct ShiftBy<I> {
    inner: Take<Skip<Cycle<I>>>,
}

impl<I: Iterator + Clone> ShiftBy<I> {
    fn new(iter: I, num: usize, length: usize) -> Self {
        Self {
            inner: iter.cycle().skip(num).take(length),
        }
    }
}

impl<I> IteratorExt for I where I: ExactSizeIterator {}

impl<I> Iterator for ShiftBy<I>
where
    I: Iterator + Clone,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monero::util::key::H;
    use rand::SeedableRng;

    #[test]
    fn test_shift_by() {
        let array = ["a", "b", "c", "d", "e"];

        let shifted = array.iter().copied().shift_by(2).collect::<Vec<_>>();

        assert_eq!(shifted, vec!["c", "d", "e", "a", "b"])
    }

    #[test]
    fn const_is_inv_eight() {
        let inv_eight = Scalar::from(8u8).invert();

        assert_eq!(inv_eight, INV_EIGHT);
    }

    #[test]
    fn sign_and_verify_at_every_index() {
        for signing_key_index in 0..11 {
            let mut rng = rand::rngs::StdRng::from_seed([0u8; 32]);

            let msg_to_sign = b"hello world, monero is amazing!!";

            let signing_key = Scalar::random(&mut rng);
            let signing_pk = signing_key * ED25519_BASEPOINT_POINT;
            let H_p_pk = hash_point_to_point(signing_pk);

            let alpha = Scalar::random(&mut rng);

            let amount_to_spend = 1000000u32;
            let fee = 10000u32;
            let output_amount = amount_to_spend - fee;

            let mut ring = random_array(|| Scalar::random(&mut rng) * ED25519_BASEPOINT_POINT);
            ring[signing_key_index] = signing_pk;

            let real_commitment_blinding = Scalar::random(&mut rng);
            let mut commitment_ring =
                random_array(|| Scalar::random(&mut rng) * ED25519_BASEPOINT_POINT);
            commitment_ring[signing_key_index] = real_commitment_blinding * ED25519_BASEPOINT_POINT
                + Scalar::from(amount_to_spend) * H.point.decompress().unwrap();

            let fee_key = Scalar::from(fee) * H.point.decompress().unwrap();

            let out_pk_blinding = Scalar::random(&mut rng);
            let out_pk = out_pk_blinding * ED25519_BASEPOINT_POINT
                + Scalar::from(output_amount) * H.point.decompress().unwrap();

            let pseudo_output_commitment = fee_key + out_pk;

            let mut responses = random_array(|| Scalar::random(&mut rng));
            responses[signing_key_index] = signing_key;

            let signature = sign(
                msg_to_sign,
                H_p_pk,
                alpha,
                &ring,
                &commitment_ring,
                responses,
                signing_key_index,
                real_commitment_blinding - out_pk_blinding,
                pseudo_output_commitment,
                alpha * ED25519_BASEPOINT_POINT,
                alpha * H_p_pk,
                signing_key * H_p_pk,
            );

            assert!(verify(
                &signature,
                msg_to_sign,
                &ring,
                &commitment_ring,
                pseudo_output_commitment
            ))
        }
    }

    #[test]
    fn sign2_and_verify_at_every_index() {
        for signing_key_index in 0..11 {
            let mut rng = rand::rngs::StdRng::from_seed([0u8; 32]);

            let msg_to_sign = b"hello world, monero is amazing!!";

            let signing_key = Scalar::random(&mut rng);
            let signing_pk = signing_key * ED25519_BASEPOINT_POINT;
            let H_p_pk = hash_point_to_point(signing_pk);

            let alpha = Scalar::random(&mut rng);

            let amount_to_spend = 1000000u32;
            let fee = 10000u32;
            let output_amount = amount_to_spend - fee;

            let mut ring = random_array(|| Scalar::random(&mut rng) * ED25519_BASEPOINT_POINT);
            ring[signing_key_index] = signing_pk;

            let real_commitment_blinding = Scalar::random(&mut rng);
            let mut commitment_ring =
                random_array(|| Scalar::random(&mut rng) * ED25519_BASEPOINT_POINT);
            commitment_ring[signing_key_index] = real_commitment_blinding * ED25519_BASEPOINT_POINT
                + Scalar::from(amount_to_spend) * H.point.decompress().unwrap();

            let fee_key = Scalar::from(fee) * H.point.decompress().unwrap();

            let out_pk_blinding = Scalar::random(&mut rng);
            let out_pk = out_pk_blinding * ED25519_BASEPOINT_POINT
                + Scalar::from(output_amount) * H.point.decompress().unwrap();

            let pseudo_output_commitment = fee_key + out_pk;

            let responses = random_array(|| Scalar::random(&mut rng));

            let signature = sign2(
                msg_to_sign,
                H_p_pk,
                alpha,
                &ring,
                &commitment_ring,
                responses,
                signing_key,
                signing_key_index,
                real_commitment_blinding - out_pk_blinding,
                pseudo_output_commitment,
                alpha * ED25519_BASEPOINT_POINT,
                alpha * H_p_pk,
                signing_key * H_p_pk,
            );

            assert!(verify(
                &signature,
                msg_to_sign,
                &ring,
                &commitment_ring,
                pseudo_output_commitment
            ))
        }
    }

    fn random_array<T: Default + Copy, const N: usize>(rng: impl FnMut() -> T) -> [T; N] {
        let mut ring = [T::default(); N];
        ring[..].fill_with(rng);

        ring
    }
}
