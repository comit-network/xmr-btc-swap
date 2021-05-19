use anyhow::bail;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use rand::{CryptoRng, Rng};
use tiny_keccak::{Hasher, Keccak};

pub struct DleqProof {
    s: Scalar,
    c: Scalar,
}

impl DleqProof {
    pub fn new(
        G: EdwardsPoint,
        xG: EdwardsPoint,
        H: EdwardsPoint,
        xH: EdwardsPoint,
        x: Scalar,
        rng: &mut (impl Rng + CryptoRng),
    ) -> Self {
        let r = Scalar::random(rng);
        let rG = r * G;
        let rH = r * H;

        let mut keccak = Keccak::v256();
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

    pub fn verify(
        &self,
        G: EdwardsPoint,
        xG: EdwardsPoint,
        H: EdwardsPoint,
        xH: EdwardsPoint,
    ) -> anyhow::Result<()> {
        let s = self.s;
        let c = self.c;

        let rG = (s * G) + (-c * xG);
        let rH = (s * H) + (-c * xH);

        let mut keccak = Keccak::v256();
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
