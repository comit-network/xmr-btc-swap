use crate::bitcoin::wallet::Watchable;
use crate::bitcoin::{
    verify_encsig, verify_sig, Address, Amount, EmptyWitnessStack, EncryptedSignature, NoInputs,
    NotThreeWitnesses, PublicKey, SecretKey, TooManyInputs, Transaction, TxLock,
};
use ::bitcoin::{Sighash, Txid};
use anyhow::{bail, Context, Result};
use bdk::miniscript::Descriptor;
use bitcoin::secp256k1;
use bitcoin::util::sighash::SighashCache;
use bitcoin::{EcdsaSighashType, Script};
use ecdsa_fun::adaptor::{Adaptor, HashTranscript};
use ecdsa_fun::fun::Scalar;
use ecdsa_fun::nonce::Deterministic;
use ecdsa_fun::Signature;
use sha2::Sha256;
use std::collections::HashMap;

use super::extract_ecdsa_sig;

#[derive(Clone, Debug)]
pub struct TxRedeem {
    inner: Transaction,
    digest: Sighash,
    lock_output_descriptor: Descriptor<::bitcoin::PublicKey>,
    watch_script: Script,
}

impl TxRedeem {
    pub fn new(tx_lock: &TxLock, redeem_address: &Address, spending_fee: Amount) -> Self {
        // lock_input is the shared output that is now being used as an input for the
        // redeem transaction
        let tx_redeem = tx_lock.build_spend_transaction(redeem_address, None, spending_fee);

        let digest = SighashCache::new(&tx_redeem)
            .segwit_signature_hash(
                0, // Only one input: lock_input (lock transaction)
                &tx_lock.output_descriptor.script_code().expect("scriptcode"),
                tx_lock.lock_amount().to_sat(),
                EcdsaSighashType::All,
            )
            .expect("sighash");

        Self {
            inner: tx_redeem,
            digest,
            lock_output_descriptor: tx_lock.output_descriptor.clone(),
            watch_script: redeem_address.script_pubkey(),
        }
    }

    pub fn txid(&self) -> Txid {
        self.inner.txid()
    }

    pub fn digest(&self) -> Sighash {
        self.digest
    }

    pub fn complete(
        mut self,
        encrypted_signature: EncryptedSignature,
        a: SecretKey,
        s_a: Scalar,
        B: PublicKey,
    ) -> Result<Transaction> {
        verify_encsig(
            B,
            PublicKey::from(s_a),
            &self.digest(),
            &encrypted_signature,
        )
        .context("Invalid encrypted signature received")?;

        let sig_a = a.sign(self.digest());
        let adaptor = Adaptor::<HashTranscript<Sha256>, Deterministic<Sha256>>::default();
        let sig_b = adaptor.decrypt_signature(&s_a, encrypted_signature);

        let satisfier = {
            let mut satisfier = HashMap::with_capacity(2);

            let A = ::bitcoin::PublicKey {
                compressed: true,
                inner: secp256k1::PublicKey::from_slice(&a.public.to_bytes())?,
            };
            let B = ::bitcoin::PublicKey {
                compressed: true,
                inner: secp256k1::PublicKey::from_slice(&B.0.to_bytes())?,
            };

            let sig_a = secp256k1::ecdsa::Signature::from_compact(&sig_a.to_bytes())?;
            let sig_b = secp256k1::ecdsa::Signature::from_compact(&sig_b.to_bytes())?;
            // The order in which these are inserted doesn't matter
            satisfier.insert(
                A,
                ::bitcoin::EcdsaSig {
                    sig: sig_a,
                    hash_ty: EcdsaSighashType::All,
                },
            );
            satisfier.insert(
                B,
                ::bitcoin::EcdsaSig {
                    sig: sig_b,
                    hash_ty: EcdsaSighashType::All,
                },
            );

            satisfier
        };

        self.lock_output_descriptor
            .satisfy(&mut self.inner.input[0], satisfier)
            .context("Failed to sign Bitcoin redeem transaction")?;

        Ok(self.inner)
    }

    pub fn extract_signature_by_key(
        &self,
        candidate_transaction: Transaction,
        B: PublicKey,
    ) -> Result<Signature> {
        let input = match candidate_transaction.input.as_slice() {
            [input] => input,
            [] => bail!(NoInputs),
            inputs => bail!(TooManyInputs(inputs.len())),
        };

        let sigs = match input.witness.to_vec().as_slice() {
            [sig_1, sig_2, _script] => [sig_1, sig_2]
                .into_iter()
                .map(|sig| extract_ecdsa_sig(sig))
                .collect::<Result<Vec<_>, _>>(),
            [] => bail!(EmptyWitnessStack),
            witnesses => bail!(NotThreeWitnesses(witnesses.len())),
        }?;

        let sig = sigs
            .into_iter()
            .find(|sig| verify_sig(&B, &self.digest(), sig).is_ok())
            .context("Neither signature on witness stack verifies against B")?;

        Ok(sig)
    }

    pub fn weight() -> usize {
        548
    }

    #[cfg(test)]
    pub fn inner(&self) -> Transaction {
        self.inner.clone()
    }
}

impl Watchable for TxRedeem {
    fn id(&self) -> Txid {
        self.txid()
    }

    fn script(&self) -> Script {
        self.watch_script.clone()
    }
}
