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
    msg: &[u8; 32],
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
        b"CLSAG_agg_0" || ring || commitment_ring || I || D_inv_8 || pseudo_output_commitment
    );
    let mu_C = hash_to_scalar!(
        b"CLSAG_agg_1" || ring || commitment_ring || I || D_inv_8 || pseudo_output_commitment
    );

    dbg!(hex::encode(mu_P.as_bytes()));
    dbg!(hex::encode(mu_C.as_bytes()));

    let adjusted_commitment_ring = &commitment_ring - pseudo_output_commitment;

    let compute_ring_element = |L: EdwardsPoint, R: EdwardsPoint| {
        hash_to_scalar!(
            b"CLSAG_round" || ring || commitment_ring || pseudo_output_commitment || msg || L || R
        )
    };

    let h_0 = compute_ring_element(L_0, R_0);

    let h_last = fake_responses
        .iter()
        .enumerate()
        .fold(h_0, |h_prev, (i, s_i)| {
            let pk_i = ring[i + 1];

            let L_i = compute_L(
                h_prev,
                mu_P,
                mu_C,
                *s_i,
                pk_i,
                adjusted_commitment_ring[i + 1],
            );
            let R_i = compute_R(h_prev, mu_P, mu_C, *s_i, pk_i, I, D_inv_8);

            dbg!(hex::encode(L_i.compress().as_bytes()));
            dbg!(hex::encode(R_i.compress().as_bytes()));

            let h = compute_ring_element(L_i, R_i);
            dbg!(hex::encode(h.as_bytes()));

            h
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
    let ring = Ring::new(ring);
    let commitment_ring = Ring::new(commitment_ring);
    let D = D_inv_8 * Scalar::from(8u8);

    let mu_P = hash_to_scalar!(
        b"CLSAG_agg_0" || ring || commitment_ring || I || D_inv_8 || pseudo_output_commitment
    );
    let mu_C = hash_to_scalar!(
        b"CLSAG_agg_1" || ring || commitment_ring || I || D_inv_8 || pseudo_output_commitment
    );

    let adjusted_commitment_ring = &commitment_ring - pseudo_output_commitment;

    let mut h = h_0;

    for (i, s_i) in responses.iter().enumerate() {
        let pk_i = ring[(i + 1) % RING_SIZE];

        let L_i = compute_L(
            h,
            mu_P,
            mu_C,
            *s_i,
            pk_i,
            adjusted_commitment_ring[(i + 1) % RING_SIZE],
        );
        let R_i = compute_R(h, mu_P, mu_C, *s_i, pk_i, I, D);

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

#[derive(Clone, Debug)]
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
    use itertools::Itertools;
    use rand::rngs::OsRng;
    use rand::SeedableRng;

    #[test]
    fn const_is_inv_eight() {
        let inv_eight = Scalar::from(8u8).invert();

        assert_eq!(inv_eight, INV_EIGHT);
    }

    #[test]
    fn sign_and_verify() {
        let mut rng = rand::rngs::StdRng::from_seed([0u8; 32]);

        let msg_to_sign = b"hello world, monero is amazing!!";

        let signing_key = Scalar::random(&mut rng);
        let signing_pk = signing_key * ED25519_BASEPOINT_POINT;
        let H_p_pk = hash_point_to_point(signing_pk);

        let alpha = Scalar::random(&mut rng);

        let mut ring = random_array(|| Scalar::random(&mut rng) * ED25519_BASEPOINT_POINT);
        ring[0] = signing_pk;

        let real_commitment_blinding = Scalar::random(&mut rng);
        let mut commitment_ring =
            random_array(|| Scalar::random(&mut rng) * ED25519_BASEPOINT_POINT);
        commitment_ring[0] = real_commitment_blinding * ED25519_BASEPOINT_POINT; // + 0 * H

        // TODO: document
        let pseudo_output_commitment = commitment_ring[0];

        let signature = sign(
            msg_to_sign,
            signing_key,
            H_p_pk,
            alpha,
            &ring,
            &commitment_ring,
            random_array(|| Scalar::random(&mut rng)),
            Scalar::zero(),
            pseudo_output_commitment,
            alpha * ED25519_BASEPOINT_POINT,
            alpha * H_p_pk,
            signing_key * H_p_pk,
        );

        signature.responses.iter().enumerate().for_each(|(i, res)| {
            println!(
                r#"epee::string_tools::hex_to_pod("{}", clsag.s[{}]);"#,
                hex::encode(res.as_bytes()),
                i
            );
        });
        println!("{}", hex::encode(signature.h_0.as_bytes()));
        println!("{}", hex::encode(signature.D.compress().as_bytes()));

        let I = hex::encode(signature.I.compress().to_bytes());
        println!("{}", I);

        let msg = hex::encode(msg_to_sign);
        println!("{}", msg);

        ring.iter().zip(commitment_ring.iter()).for_each(|(pk, c)| {
            println!(
                "std::make_tuple(\"{}\",\"{}\"),",
                hex::encode(pk.compress().to_bytes()),
                hex::encode(c.compress().to_bytes())
            );
        });

        println!(
            "{}",
            hex::encode(pseudo_output_commitment.compress().to_bytes())
        );

        assert!(verify(
            &signature,
            msg_to_sign,
            &ring,
            &commitment_ring,
            pseudo_output_commitment
        ))
    }

    fn random_array<T: Default + Copy, const N: usize>(rng: impl FnMut() -> T) -> [T; N] {
        let mut ring = [T::default(); N];
        ring[..].fill_with(rng);

        ring
    }
}
