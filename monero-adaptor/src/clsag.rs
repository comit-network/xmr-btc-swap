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

    dbg!(hex::encode(mu_P.as_bytes()));
    dbg!(hex::encode(mu_C.as_bytes()));

    let adjusted_commitment_ring = commitment_ring.map(|point| point - pseudo_output_commitment);

    let compute_ring_element = |L: EdwardsPoint, R: EdwardsPoint| {
        hash_to_scalar!(
            b"CLSAG_round" || ring || commitment_ring || pseudo_output_commitment || msg || L || R
        )
    };

    let h_signing_index = compute_ring_element(L, R);

    dbg!(hex::encode(L.compress().as_bytes()));
    dbg!(hex::encode(R.compress().as_bytes()));
    dbg!(hex::encode(h_signing_index.as_bytes()));

    let element_after_signing_key = (signing_key_index + 1) % RING_SIZE;
    let mut h_0 = Scalar::zero();
    // let h_prev_signing_index = itertools::izip!(responses, ring,
    // adjusted_commitment_ring)     .enumerate()
    //     .shift_by(element_after_signing_key)
    //     .take(RING_SIZE - 1)
    //     .fold(h_signing_index, |h_prev, (i, (s, P, C))| {
    //         let L_i = compute_L(h_prev, mu_P, mu_C, s, *P, C);
    //         let R_i = compute_R(h_prev, mu_P, mu_C, s, *P, I, D);

    //         dbg!(hex::encode(L_i.compress().as_bytes()));
    //         dbg!(hex::encode(R_i.compress().as_bytes()));

    //         let h = compute_ring_element(L_i, R_i);
    //         dbg!(hex::encode(h.as_bytes()));

    //         if i == RING_SIZE - 1 {
    //             h_0 = h
    //         }

    //         h
    //     });

    let mut h_prev = h_signing_index;
    let mut i = (signing_key_index + 1) % RING_SIZE;

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

        dbg!(hex::encode(L_i.compress().as_bytes()));
        dbg!(hex::encode(R_i.compress().as_bytes()));

        let h = compute_ring_element(L_i, R_i);
        dbg!(hex::encode(h.as_bytes()));

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

    dbg!(hex::encode(mu_P.as_bytes()));
    dbg!(hex::encode(mu_C.as_bytes()));

    let adjusted_commitment_ring = commitment_ring.map(|point| point - pseudo_output_commitment);

    let h_0_computed = itertools::izip!(responses, ring, adjusted_commitment_ring).fold(
        h_0,
        |h, (s_i, pk_i, adjusted_commitment_i)| {
            dbg!(hex::encode(h.as_bytes()));
            dbg!(hex::encode(pk_i.compress().as_bytes()));
            dbg!(hex::encode(adjusted_commitment_i.compress().as_bytes()));

            let L_i = compute_L(h, mu_P, mu_C, s_i, *pk_i, adjusted_commitment_i);
            let R_i = compute_R(h, mu_P, mu_C, s_i, *pk_i, I, D);

            dbg!(hex::encode(L_i.compress().as_bytes()));
            dbg!(hex::encode(R_i.compress().as_bytes()));

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
    use curve25519_dalek::edwards::CompressedEdwardsY;
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
    fn sign_and_verify() {
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
        ring[0] = signing_pk;

        let real_commitment_blinding = Scalar::random(&mut rng);
        let mut commitment_ring =
            random_array(|| Scalar::random(&mut rng) * ED25519_BASEPOINT_POINT);
        commitment_ring[0] = real_commitment_blinding * ED25519_BASEPOINT_POINT
            + Scalar::from(amount_to_spend) * H.point.decompress().unwrap();

        let fee_key = Scalar::from(fee) * H.point.decompress().unwrap();

        let out_pk_blinding = Scalar::random(&mut rng);
        let out_pk = out_pk_blinding * ED25519_BASEPOINT_POINT
            + Scalar::from(output_amount) * H.point.decompress().unwrap();

        let pseudo_output_commitment = fee_key + out_pk;

        let mut responses = random_array(|| Scalar::random(&mut rng));
        responses[0] = signing_key;

        let signature = sign(
            msg_to_sign,
            H_p_pk,
            alpha,
            &ring,
            &commitment_ring,
            responses,
            0,
            real_commitment_blinding - out_pk_blinding,
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

    #[test]
    fn sign_and_verify_non_zero_signing_index() {
        let mut rng = rand::rngs::StdRng::from_seed([0u8; 32]);

        let msg_to_sign = b"hello world, monero is amazing!!";

        let signing_key = Scalar::random(&mut rng);
        let signing_pk = signing_key * ED25519_BASEPOINT_POINT;
        let H_p_pk = hash_point_to_point(signing_pk);

        let alpha = Scalar::random(&mut rng);

        let amount_to_spend = 1000000u32;
        let fee = 10000u32;
        let output_amount = amount_to_spend - fee;

        let signing_key_index = 3;

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

    #[test]
    fn static_assert() {
        let mut rng = rand::rngs::StdRng::from_seed([0u8; 32]);

        let msg_to_sign = b"hello world, monero is amazing!!";

        let signing_key = Scalar::random(&mut rng);
        let signing_pk = signing_key * ED25519_BASEPOINT_POINT;
        let H_p_pk = hash_point_to_point(signing_pk);

        let alpha = Scalar::random(&mut rng);

        let amount_to_spend = 1000000u32;
        let fee = 10000u32;
        let output_amount = amount_to_spend - fee;

        let signing_key_index = 3;

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

        signature.responses.iter().enumerate().for_each(|(i, res)| {
            println!(
                r#"epee::string_tools::hex_to_pod("{}", clsag.s[{}]);"#,
                hex::encode(res.as_bytes()),
                i
            );
        });
        println!(
            r#"epee::string_tools::hex_to_pod("{}", clsag.c1);"#,
            hex::encode(signature.h_0.as_bytes())
        );
        println!(
            r#"epee::string_tools::hex_to_pod("{}", clsag.D);"#,
            hex::encode(signature.D.compress().as_bytes())
        );
        println!(
            r#"epee::string_tools::hex_to_pod("{}", clsag.I);"#,
            hex::encode(signature.I.compress().to_bytes())
        );
        println!(
            r#"epee::string_tools::hex_to_pod("{}", msg);"#,
            hex::encode(&msg_to_sign)
        );

        ring.iter()
            .zip(commitment_ring.iter())
            .enumerate()
            .for_each(|(i, (pk, c))| {
                println!(
                    r#"epee::string_tools::hex_to_pod("{}", pubs[{}].dest);"#,
                    hex::encode(&pk.compress().to_bytes()),
                    i
                );
                println!(
                    r#"epee::string_tools::hex_to_pod("{}", pubs[{}].mask);"#,
                    hex::encode(&c.compress().to_bytes()),
                    i
                );
            });

        println!(
            r#"epee::string_tools::hex_to_pod("{}", Cout);"#,
            hex::encode(pseudo_output_commitment.compress().to_bytes())
        );

        let expected_signature = Signature {
            responses: [
                Scalar::from_bytes_mod_order([
                    127, 142, 51, 31, 218, 255, 98, 112, 178, 104, 118, 103, 42, 209, 28, 255, 175,
                    29, 138, 139, 68, 163, 81, 163, 67, 147, 22, 122, 147, 68, 92, 9,
                ]),
                Scalar::from_bytes_mod_order([
                    55, 39, 188, 124, 195, 144, 38, 253, 71, 140, 38, 58, 171, 17, 180, 6, 190,
                    186, 215, 8, 221, 240, 236, 13, 35, 83, 180, 101, 185, 38, 24, 7,
                ]),
                Scalar::from_bytes_mod_order([
                    178, 43, 254, 4, 205, 192, 188, 255, 9, 191, 86, 139, 224, 193, 97, 50, 207,
                    87, 107, 168, 241, 80, 216, 69, 94, 148, 214, 13, 241, 171, 101, 4,
                ]),
                Scalar::from_bytes_mod_order([
                    153, 106, 211, 43, 37, 114, 141, 155, 71, 44, 95, 94, 105, 251, 233, 25, 150,
                    218, 135, 42, 8, 197, 134, 108, 40, 180, 142, 7, 11, 131, 221, 5,
                ]),
                Scalar::from_bytes_mod_order([
                    84, 122, 135, 162, 200, 132, 34, 227, 238, 27, 159, 142, 81, 164, 223, 65, 58,
                    17, 233, 222, 253, 52, 5, 62, 246, 249, 23, 155, 221, 211, 120, 3,
                ]),
                Scalar::from_bytes_mod_order([
                    169, 56, 43, 12, 229, 34, 23, 219, 132, 73, 217, 100, 237, 187, 48, 61, 105,
                    241, 193, 229, 231, 8, 32, 73, 39, 207, 13, 74, 86, 145, 183, 12,
                ]),
                Scalar::from_bytes_mod_order([
                    2, 190, 48, 136, 226, 121, 206, 13, 155, 94, 222, 193, 23, 157, 75, 230, 88,
                    194, 56, 236, 197, 82, 150, 7, 66, 95, 201, 13, 187, 112, 75, 14,
                ]),
                Scalar::from_bytes_mod_order([
                    230, 241, 238, 19, 133, 32, 25, 92, 11, 232, 189, 24, 58, 32, 193, 154, 8, 3,
                    209, 81, 241, 44, 188, 197, 104, 156, 219, 19, 219, 47, 147, 6,
                ]),
                Scalar::from_bytes_mod_order([
                    141, 52, 223, 252, 61, 32, 137, 198, 134, 251, 231, 16, 39, 85, 248, 169, 134,
                    142, 170, 78, 24, 62, 141, 41, 232, 202, 54, 7, 222, 100, 36, 8,
                ]),
                Scalar::from_bytes_mod_order([
                    4, 202, 35, 58, 27, 151, 118, 247, 118, 36, 208, 126, 2, 161, 233, 57, 151,
                    110, 172, 133, 160, 248, 53, 50, 31, 99, 20, 2, 205, 7, 51, 7,
                ]),
                Scalar::from_bytes_mod_order([
                    10, 170, 95, 224, 138, 210, 12, 240, 229, 196, 185, 129, 209, 241, 52, 97, 215,
                    199, 36, 116, 183, 243, 83, 157, 179, 216, 14, 206, 110, 36, 216, 2,
                ]),
            ],
            h_0: Scalar::from_bytes_mod_order([
                60, 49, 64, 139, 212, 108, 178, 124, 109, 0, 127, 114, 21, 125, 105, 19, 5, 26,
                213, 68, 136, 72, 0, 234, 108, 167, 116, 85, 57, 112, 166, 14,
            ]),
            I: CompressedEdwardsY([
                173, 170, 74, 7, 45, 185, 155, 35, 46, 139, 60, 64, 47, 7, 169, 45, 119, 126, 207,
                95, 125, 217, 110, 236, 27, 126, 228, 106, 254, 188, 101, 80,
            ])
            .decompress()
            .unwrap(),
            D: CompressedEdwardsY([
                38, 225, 116, 236, 62, 79, 109, 179, 10, 190, 180, 148, 105, 45, 232, 16, 92, 125,
                110, 50, 198, 186, 16, 217, 245, 22, 178, 155, 200, 68, 100, 86,
            ])
            .decompress()
            .unwrap(),
        };

        assert_eq!(signature.I, expected_signature.I);
        assert_eq!(signature.D, expected_signature.D);
        assert_eq!(signature.h_0, expected_signature.h_0);

        for (actual, expected) in signature
            .responses
            .iter()
            .zip(expected_signature.responses.iter())
        {
            assert_eq!(actual, expected)
        }
    }

    fn random_array<T: Default + Copy, const N: usize>(rng: impl FnMut() -> T) -> [T; N] {
        let mut ring = [T::default(); N];
        ring[..].fill_with(rng);

        ring
    }
}
