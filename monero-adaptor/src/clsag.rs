use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use hash_edwards_to_edwards::hash_point_to_point;

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

    let h_1 = compute_ring_element(L_0, R_0); // if our real key is on index 0, the first hash is index 1

    dbg!(hex::encode(L_0.compress().as_bytes()));
    dbg!(hex::encode(R_0.compress().as_bytes()));
    dbg!(hex::encode(h_1.as_bytes()));

    // if we start at h_1, the final element is h_0
    let h_0 = fake_responses
        .iter()
        .enumerate()
        .fold(h_1, |h_prev, (i, s_i)| {
            let pk_i = ring[i + 1];

            let L_i = compute_L(
                h_prev,
                mu_P,
                mu_C,
                *s_i,
                pk_i,
                adjusted_commitment_ring[i + 1],
            );
            let R_i = compute_R(h_prev, mu_P, mu_C, *s_i, pk_i, I, D);

            dbg!(hex::encode(L_i.compress().as_bytes()));
            dbg!(hex::encode(R_i.compress().as_bytes()));

            let h = compute_ring_element(L_i, R_i);
            dbg!(hex::encode(h.as_bytes()));

            h
        });

    // h_0 gives us s_0
    let s_0 = alpha - h_0 * ((mu_P * signing_key) + (mu_C * z));

    Signature {
        responses: [
            s_0,
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

    let mut h = h_0;

    for (i, s_i) in responses.iter().enumerate() {
        let pk_i = ring[i % RING_SIZE];

        let adjusted_commitment_i = adjusted_commitment_ring[i % RING_SIZE];

        dbg!(hex::encode(pk_i.compress().as_bytes()));
        dbg!(hex::encode(adjusted_commitment_i.compress().as_bytes()));

        let L_i = compute_L(h, mu_P, mu_C, *s_i, pk_i, adjusted_commitment_i);
        let R_i = compute_R(h, mu_P, mu_C, *s_i, pk_i, I, D);

        dbg!(hex::encode(L_i.compress().as_bytes()));
        dbg!(hex::encode(R_i.compress().as_bytes()));

        h = hash_to_scalar!(
            b"CLSAG_round"
                || ring
                || commitment_ring
                || pseudo_output_commitment
                || msg
                || L_i
                || R_i
        );

        dbg!(hex::encode(h.as_bytes()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn const_is_inv_eight() {
        let inv_eight = Scalar::from(8u8).invert();

        assert_eq!(inv_eight, INV_EIGHT);
    }

    // #[test]
    // fn verify_own() {
    //     use hex_literal::hex;
    //
    //     let signature = Signature {
    //         responses: [
    //             Scalar::from_bytes_mod_order(hex!("3a890a9668162dcbaf507644a4ee267c8f724199a2e7cd88cb6ecaee6687a007")),
    //             Scalar::from_bytes_mod_order(hex!("fabef4c7e78f64bd776c671d4a6ce6446abccc9a39ed8e366a13f38022112e08")),
    //             Scalar::from_bytes_mod_order(hex!("24bc969dbecbf35aac0d935827dba5cd4f15421d3b556542bd3bf8007440070a")),
    //             Scalar::from_bytes_mod_order(hex!("b2724373414086ab487c49314c10bbb29dc929184bb67ee8a2af08cd42df0100")),
    //             Scalar::from_bytes_mod_order(hex!("a61f5827347d7539259690ef2dd5c66c6220b5818e93d7fed103f30329b1290b")),
    //             Scalar::from_bytes_mod_order(hex!("f1377aba0ab16e0cc39f05e3732a47a2710a3d4a37b41a5fbf8ce700a4c20006")),
    //             Scalar::from_bytes_mod_order(hex!("b31c6c8e2b3f3d590bf40d0279ca8a8dd1efb825f9942bcf15abc44dea9e200b")),
    //             Scalar::from_bytes_mod_order(hex!("076a931f7763b54599aae33b4eda2dd6b89392f558a38e11dfe60d109fd4c806")),
    //             Scalar::from_bytes_mod_order(hex!("a8f3351144db0f827e8ec22044f843c89df996bf95db8a06134de4f26c214905")),
    //             Scalar::from_bytes_mod_order(hex!("c69c078d0bcb5485e296377b522af29d0317eba9ef05bfeb8214e7569944c00a")),
    //             Scalar::from_bytes_mod_order(hex!("060a75a948f5e58dcfe9f2e5a026f837bf6f13f6297ad0c0218fea6f0385ca0c")),
    //         ],
    //         h_0: Scalar::from_bytes_mod_order(hex!("18d972021968f19022810d6e2312b6a8d5f9e6a1d4d70169a2132844674ba10a")),
    //         I: CompressedEdwardsY(hex!("bb96750d51722c25bfac800163dc1c44ba00801f70458b57da1dbb0a98e2196f")).decompress().unwrap(),
    //         D: CompressedEdwardsY(hex!("ab3954aa6bda2476c34a657a2624150e4c76a19ddb9fcd5f15ed5c2b62a34b91")).decompress().unwrap()
    //     };
    //
    //     let result = verify(&signature, &hex!("f9bde7592500046752e751303b466d8906749c58ee8fc9b9dd768c12378dcd8e"), &[
    //         CompressedEdwardsY(hex!("f17e12d090554a3f5b3e0368e54bbf2301bf0ab431762e630acc6f6e85887dc6")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("a6404a8f9733810f54ac052abcd422f7afdc3744d0a036c3df1e5f57e9a46cee")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("1e3aa56f30207ae6b8128d0e94bc25ebfd10e9b3cfcb8d0fec78b4871db1a284")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("94972ef98177cd72c2e65c5dcdf003b601409e2d362d0e658203611c8e6ab1bf")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("8cb12ad1b64ac557a628304dbe2f9c028284be82a4d62fcfcd121082f5684bdc")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("0328c0a04722bcaa47756a9fb9fc185ca801e18d7cc8838afbc2e3370a399574")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("b6bf5222add24b62f3a892dbdecc64f9726e2a3f4062aafb53b7299be0ff3a4d")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("1a5b92bb34e8e6f67e880a0286f749682ca04e438caf1d0f070bca05dc1b3f3d")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("4e6b517d7bbffce134ee464d98e05c8eee6bcadac36b9d9e1e322b53a7ec97b5")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("911824d7a6b35e47a96ddba6e1c0e622763dd3734c85ddcb2b8cb27becfdb200")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("cb441384992cede75c9d4044126728c89796aa2aae1936208bafd0ab1eb4d83c")).decompress().unwrap(),
    //     ], &[
    //         CompressedEdwardsY(hex!("7e8066f0fcb0ae40cafc953bc7508ee08fd64abeb0c155ef61248ee740e0c4be")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("29b9cdf249ad0647966a57ba907ab7764a830cc2fb504bae0b6a2d0edc1278b7")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("110ffbc7b76b0b1ba7759e2339007ca2463db0ee2aeedce15a27af85436d4614")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("9bb749be705747d9c28168c0446d589b3ac18949fa0087e230805aaff5a9982f")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("36c39958ddcad401d85d63883da510505650321ad7a26859e8b1b6c28204d274")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("05b58165401774696e788bd57a1257834358222d2f4384e39e4001403713dff2")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("aa2c0ec04f2a37942cbb11b48add610f50323a531b9a16ddf4e9661082ac34f1")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("f034b8ff2cbd2729a7c19c0cc59a053fcbe1123f10acb0ec9e86bf122f0d3b12")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("50a3f64bab0f0136578d06613239b914f3746baba8855bd95b8a56f671b6dcee")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("96e9dc7a96a19c9ebaeb33ab94e7e9d86d88df1c1b11006b297b74f529f37f5a")).decompress().unwrap(),
    //         CompressedEdwardsY(hex!("ead60b7504850c7293e99f0f13823d0f0e99dd5f0dcce6f71a5f1990dd25e8ae")).decompress().unwrap(),
    //     ], CompressedEdwardsY(hex!("cd0d4bf52b489bff3a9f4d50587908c3cb16274e86b8514d67178321e75a491b")).decompress().unwrap());
    //
    //     assert!(result)
    // }

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
