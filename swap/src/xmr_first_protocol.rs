use crate::bitcoin::Txid;
use crate::protocol::CROSS_CURVE_PROOF_SYSTEM;
use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use ecdsa_fun::fun::Point;
use hash_edwards_to_edwards::hash_point_to_point;
use monero_adaptor::AdaptorSignature;
use rand::rngs::OsRng;

pub mod alice;
pub mod bob;
mod state_machine;
pub mod transactions;

pub struct Alice {
    pub a: crate::bitcoin::SecretKey,
    pub s_a: crate::monero::Scalar,
    r_a: Scalar,
    // private view keys
    pub v_a: crate::monero::PrivateViewKey,
    pub v_b: crate::monero::PrivateViewKey,
    pub S_a: EdwardsPoint,
    pub S_b: crate::monero::PublicKey,
    pub R_a: EdwardsPoint,
    pub S_prime_a: Point,
    pub R_prime_a: EdwardsPoint,
    pub pk_a: crate::bitcoin::PublicKey,
    pub pk_b: crate::bitcoin::PublicKey,
    pub K_a: crate::monero::PublicViewKey,
    pub K_b: crate::monero::PublicViewKey,
}

pub struct Bob {
    b: crate::bitcoin::SecretKey,
    pub s_b: crate::monero::Scalar,
    // private view keys
    pub v_a: crate::monero::PrivateViewKey,
    pub v_b: crate::monero::PrivateViewKey,
    pub S_a: EdwardsPoint,
    pub S_b: crate::monero::PublicKey,
    pub R_a: EdwardsPoint,
    pub S_prime_a: Point,
    pub R_prime_a: EdwardsPoint,
    pub pk_a: crate::bitcoin::PublicKey,
    pub pk_b: crate::bitcoin::PublicKey,
    pub K_a: crate::monero::PublicViewKey,
    pub K_b: crate::monero::PublicViewKey,
}

pub fn setup() -> (Alice, Bob) {
    let v_a = crate::monero::PrivateViewKey::new_random(&mut OsRng);
    let v_b = crate::monero::PrivateViewKey::new_random(&mut OsRng);

    let a = crate::bitcoin::SecretKey::new_random(&mut OsRng);
    let b = crate::bitcoin::SecretKey::new_random(&mut OsRng);

    let s_a = crate::monero::Scalar::random(&mut OsRng);

    let s_b = crate::monero::Scalar::random(&mut OsRng);
    let S_b = monero::PublicKey::from_private_key(&monero::PrivateKey { scalar: s_b });

    let (_dleq_proof_s_a, (S_prime_a, S_a)) = CROSS_CURVE_PROOF_SYSTEM.prove(&s_a, &mut OsRng);

    let (r_a, R_a, R_prime_a) = {
        let r_a = Scalar::random(&mut OsRng);
        let R_a = r_a * ED25519_BASEPOINT_POINT;

        let pk_hashed_to_point = hash_point_to_point(S_a);

        let R_prime_a = r_a * pk_hashed_to_point;

        (r_a, R_a, R_prime_a)
    };

    let K_a = v_a.public();
    let K_b = v_b.public();

    let pk_a = a.public();
    let pk_b = b.public();

    let alice = Alice {
        a,
        v_a,
        v_b,
        s_a,
        S_a,
        S_b,
        r_a,
        R_a,
        S_prime_a,
        R_prime_a,
        pk_a,
        pk_b,
        K_a,
        K_b,
    };

    let bob = Bob {
        b,
        v_a,
        v_b,
        s_b,
        S_a,
        S_b,
        R_a,
        S_prime_a,
        R_prime_a,
        pk_a,
        pk_b,
        K_a,
        K_b,
    };

    (alice, bob)
}
