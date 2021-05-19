use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::scalar::Scalar;
use hash_edwards_to_edwards::hash_point_to_point;
use monero::util::key::H;
use monero_adaptor::{Alice0, Bob0};
use rand::rngs::OsRng;
use rand::thread_rng;

#[test]
fn sign_and_verify_success() {
    let msg_to_sign = b"hello world, monero is amazing!!";

    let s_prime_a = Scalar::random(&mut OsRng);
    let s_b = Scalar::random(&mut OsRng);

    let pk = (s_prime_a + s_b) * ED25519_BASEPOINT_POINT;

    let (r_a, R_a, R_prime_a) = {
        let r_a = Scalar::random(&mut OsRng);
        let R_a = r_a * ED25519_BASEPOINT_POINT;

        let H_p_pk = hash_point_to_point(pk);

        let R_prime_a = r_a * H_p_pk;

        (r_a, R_a, R_prime_a)
    };

    let amount_to_spend = 1000000u32;
    let fee = 10000u32;
    let output_amount = amount_to_spend - fee;

    let mut ring = random_array(|| Scalar::random(&mut thread_rng()) * ED25519_BASEPOINT_POINT);
    let mut commitment_ring =
        random_array(|| Scalar::random(&mut thread_rng()) * ED25519_BASEPOINT_POINT);

    ring[0] = pk;

    let real_commitment_blinding = Scalar::random(&mut thread_rng());
    commitment_ring[0] =
        real_commitment_blinding * ED25519_BASEPOINT_POINT + Scalar::from(amount_to_spend) * *H;

    let fee_key = Scalar::from(fee) * *H;

    let out_pk_blinding = Scalar::random(&mut thread_rng());
    let out_pk = out_pk_blinding * ED25519_BASEPOINT_POINT + Scalar::from(output_amount) * *H;

    let pseudo_output_commitment = fee_key + out_pk;

    let alice = Alice0::new(
        ring,
        *msg_to_sign,
        commitment_ring,
        pseudo_output_commitment,
        R_a,
        R_prime_a,
        s_prime_a,
        real_commitment_blinding - out_pk_blinding,
        &mut OsRng,
    )
    .unwrap();
    let bob = Bob0::new(
        ring,
        *msg_to_sign,
        commitment_ring,
        pseudo_output_commitment,
        R_a,
        R_prime_a,
        s_b,
        real_commitment_blinding - out_pk_blinding,
        &mut OsRng,
    )
    .unwrap();

    let msg = alice.next_message(&mut OsRng);
    let bob = bob.receive(msg);

    let msg = bob.next_message(&mut OsRng);
    let alice = alice.receive(msg).unwrap();

    let msg = alice.next_message();
    let bob = bob.receive(msg).unwrap();

    let msg = bob.next_message();
    let alice = alice.receive(msg);

    let sig = alice.adaptor_sig.adapt(r_a);

    assert!(monero::clsag::verify(
        &sig,
        msg_to_sign,
        &ring,
        &commitment_ring,
        alice.I,
        pseudo_output_commitment,
    ));
}

fn random_array<T: Default + Copy, const N: usize>(rng: impl FnMut() -> T) -> [T; N] {
    let mut ring = [T::default(); N];
    ring[..].fill_with(rng);

    ring
}
