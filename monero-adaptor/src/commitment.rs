use anyhow::{bail, Result};
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use tiny_keccak::{Hasher, Keccak};

#[derive(PartialEq)]
pub struct Commitment([u8; 32]);

impl Commitment {
    pub fn new(
        fake_responses: [Scalar; 10],
        I_a: EdwardsPoint,
        I_hat_a: EdwardsPoint,
        T_a: EdwardsPoint,
    ) -> Self {
        let fake_responses = fake_responses
            .iter()
            .flat_map(|r| r.as_bytes().to_vec())
            .collect::<Vec<u8>>();

        let mut keccak = Keccak::v256();
        keccak.update(&fake_responses);
        keccak.update(I_a.compress().as_bytes());
        keccak.update(I_hat_a.compress().as_bytes());
        keccak.update(T_a.compress().as_bytes());

        let mut output = [0u8; 32];
        keccak.finalize(&mut output);

        Self(output)
    }
}

pub struct Opening {
    fake_responses: [Scalar; 10],
    I_a: EdwardsPoint,
    I_hat_a: EdwardsPoint,
    T_a: EdwardsPoint,
}

impl Opening {
    pub fn new(
        fake_responses: [Scalar; 10],
        I_a: EdwardsPoint,
        I_hat_a: EdwardsPoint,
        T_a: EdwardsPoint,
    ) -> Self {
        Self {
            fake_responses,
            I_a,
            I_hat_a,
            T_a,
        }
    }

    pub fn open(
        self,
        commitment: Commitment,
    ) -> Result<([Scalar; 10], EdwardsPoint, EdwardsPoint, EdwardsPoint)> {
        let self_commitment =
            Commitment::new(self.fake_responses, self.I_a, self.I_hat_a, self.T_a);

        if self_commitment == commitment {
            Ok((self.fake_responses, self.I_a, self.I_hat_a, self.T_a))
        } else {
            bail!("opening does not match commitment")
        }
    }
}
