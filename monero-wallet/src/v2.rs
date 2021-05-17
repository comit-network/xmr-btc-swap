use crate::{CalculateKeyOffsetBoundaries, DecoyInput, FetchDecoyInputs};
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use monero::util::ringct::Clsag;
use monero::{Address, KeyPair, OwnedTxOut, Transaction};
use rand::{CryptoRng, RngCore};

pub struct EmptyTransaction {}

impl EmptyTransaction {
    // TODO: Need to validate that given index matches with tx pubkey and commitment
    pub fn spend_from(input: OwnedTxOut<'_>, global_output_index: u64) -> Result<InputAdded, MissingOpening> {
        todo!()
    }
}

pub struct MissingOpening; // The opening was missing from the TX

pub struct InputAdded {}

impl InputAdded {
    pub fn with_decoys(self, decoys: [DecoyInput; 10]) -> Result<DecoyOffsetsAdded, DuplicateIndex> {
        todo!()
    }

    pub fn with_random_decoys(
        self,
        rng: &mut impl RngCore,
        client: &(impl FetchDecoyInputs + CalculateKeyOffsetBoundaries),
    ) -> DecoyOffsetsAdded {
        todo!()
    }

    pub fn with_decoys_from_indices(
        self,
        decoy_indices: [u64; 10],
        client: &(impl FetchDecoyInputs),
    ) -> Result<DecoyOffsetsAdded, DuplicateIndex> {
        todo!()
    }
}

pub struct DuplicateIndex; // One of the indices was an evil twin

pub struct DecoyOffsetsAdded {}

impl DecoyOffsetsAdded {
    pub fn add_output(
        self,
        to: Address,
        amount: u64,
        rng: &mut (impl RngCore + CryptoRng),
    ) -> Result<OutputsAdded, InsufficientFunds> {
        todo!()
    }
}

pub struct InsufficientFunds;

pub struct OutputsAdded {}

impl OutputsAdded {
    pub fn add_output(
        self,
        to: Address,
        amount: u64,
        rng: &mut (impl RngCore + CryptoRng),
    ) -> Result<Self, InsufficientFunds> {
        todo!()
    }

    pub fn blind_outputs(self, rng: &mut (impl RngCore + CryptoRng)) -> OutputsBlinded {
        todo!()
    }
}

pub struct OutputsBlinded {}

impl OutputsBlinded {
    pub fn signature_parameters(&self) -> SignatureParameters {
        todo!()
    }

    /// Signs the transaction.
    ///
    /// This function calls the CLSAG sign algorithm with a set of parameters that will work. This however, assumes the caller does not want to have control over these parameters.
    pub fn sign(self, keys: KeyPair, rng: &mut (impl RngCore + CryptoRng)) -> Transaction {
        // TODO: Do we want a sign_recommended API in monero::clsag?

        todo!()
    }

    /// Use the given signature for the internal transaction.
    ///
    /// This function is useful if the caller wants to have full control over certain parameters such as responses, L, R or I.
    /// The provided signature will be validated to make sure it is correct.
    pub fn with_signature(self, sig: Clsag) -> Result<Transaction, InvalidSignature> {
        todo!()
    }
}

pub struct InvalidSignature;

// TODO: We can break the CLSAG fn signature down into two parts:
// 1. What we see below
// 2. What an adaptor sig wants to control (signing key, alpha, L, R & I)
pub struct SignatureParameters {
    message: [u8; 32],
    z: Scalar,
    ring: [EdwardsPoint; 11],
    commitment_ring: [EdwardsPoint; 11],
    // TODO: Can the adaptor sig protocol control the signing key index?
    // Do we even need to control it?
    // We need to know the public key to know the index.
    // signing_key_index: usize,
    pseudo_output_commitment: EdwardsPoint,
}
