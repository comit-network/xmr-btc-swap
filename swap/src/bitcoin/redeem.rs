use crate::bitcoin::wallet::Watchable;
use crate::bitcoin::{
    verify_encsig, verify_sig, Address, EmptyWitnessStack, EncryptedSignature, NoInputs,
    NotThreeWitnesses, PublicKey, SecretKey, TooManyInputs, Transaction, TxLock,
};
use ::bitcoin::util::bip143::SigHashCache;
use ::bitcoin::{SigHash, SigHashType, Txid};
use anyhow::{bail, Context, Result};
use bitcoin::Script;
use ecdsa_fun::adaptor::{Adaptor, HashTranscript};
use ecdsa_fun::fun::Scalar;
use ecdsa_fun::nonce::Deterministic;
use ecdsa_fun::Signature;
use miniscript::{Descriptor, DescriptorTrait};
use sha2::Sha256;
use std::collections::HashMap;

#[derive(Debug)]
pub struct TxRedeem {
    inner: Transaction,
    digest: SigHash,
    lock_output_descriptor: Descriptor<::bitcoin::PublicKey>,
    watch_script: Script,
}

impl TxRedeem {
    pub fn new(tx_lock: &TxLock, redeem_address: &Address) -> Self {
        // lock_input is the shared output that is now being used as an input for the
        // redeem transaction
        let tx_redeem = tx_lock.build_spend_transaction(redeem_address, None);

        let digest = SigHashCache::new(&tx_redeem).signature_hash(
            0, // Only one input: lock_input (lock transaction)
            &tx_lock.output_descriptor.script_code(),
            tx_lock.lock_amount().as_sat(),
            SigHashType::All,
        );

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

    pub fn digest(&self) -> SigHash {
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
            PublicKey::from(s_a.clone()),
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
                key: a.public.into(),
            };
            let B = ::bitcoin::PublicKey {
                compressed: true,
                key: B.0.into(),
            };

            // The order in which these are inserted doesn't matter
            satisfier.insert(A, (sig_a.into(), ::bitcoin::SigHashType::All));
            satisfier.insert(B, (sig_b.into(), ::bitcoin::SigHashType::All));

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
            [inputs @ ..] => bail!(TooManyInputs(inputs.len())),
        };

        let sigs = match input
            .witness
            .iter()
            .map(|vec| vec.as_slice())
            .collect::<Vec<_>>()
            .as_slice()
        {
            [sig_1, sig_2, _script] => [sig_1, sig_2]
                .iter()
                .map(|sig| {
                    bitcoin::secp256k1::Signature::from_der(&sig[..sig.len() - 1])
                        .map(Signature::from)
                })
                .collect::<std::result::Result<Vec<_>, _>>(),
            [] => bail!(EmptyWitnessStack),
            [witnesses @ ..] => bail!(NotThreeWitnesses(witnesses.len())),
        }?;

        let sig = sigs
            .into_iter()
            .find(|sig| verify_sig(&B, &self.digest(), &sig).is_ok())
            .context("Neither signature on witness stack verifies against B")?;

        Ok(sig)
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
