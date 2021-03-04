use conquer_once::Lazy;
use ecdsa_fun::fun::marker::Mark;
use sha2::Sha256;
use sigma_fun::ext::dl_secp256k1_ed25519_eq::CrossCurveDLEQ;
use sigma_fun::HashTranscript;

pub mod alice;
pub mod bob;

pub static CROSS_CURVE_PROOF_SYSTEM: Lazy<
    CrossCurveDLEQ<HashTranscript<Sha256, rand_chacha::ChaCha20Rng>>,
> = Lazy::new(|| {
    CrossCurveDLEQ::<HashTranscript<Sha256, rand_chacha::ChaCha20Rng>>::new(
        (*ecdsa_fun::fun::G).mark::<ecdsa_fun::fun::marker::Normal>(),
        curve25519_dalek::constants::ED25519_BASEPOINT_POINT,
    )
});

#[derive(Debug, Copy, Clone)]
pub struct StartingBalances {
    pub xmr: crate::monero::Amount,
    pub btc: bitcoin::Amount,
}
