use crate::bitcoin::wallet::Watchable;
use crate::bitcoin::{
    verify_encsig, verify_sig, Address, EmptyWitnessStack, EncryptedSignature, NoInputs,
    NotThreeWitnesses, PublicKey, SecretKey, TooManyInputs, Transaction,
};
use crate::xmr_first_protocol::transactions::btc_lock::BtcLock;
use crate::xmr_first_protocol::transactions::btc_redeem::BtcRedeem;
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

#[derive(Clone, Debug)]
pub struct BtcEmergencyRefund {
    inner: Transaction,
    digest: SigHash,
    lock_output_descriptor: Descriptor<::bitcoin::PublicKey>,
    watch_script: Script,
}

impl BtcEmergencyRefund {
    pub fn new(tx_redeem: &BtcRedeem, redeem_address: &Address) -> Self {
        let tx_refund = tx_redeem.build_take_transaction(redeem_address, None);

        let digest = SigHashCache::new(&tx_refund).signature_hash(
            0, // Only one input: lock_input (lock transaction)
            &tx_refund.output_descriptor.script_code(),
            tx_refund.lock_amount().as_sat(),
            SigHashType::All,
        );

        Self {
            inner: tx_refund,
            digest,
            lock_output_descriptor: tx_refund.output_descriptor.clone(),
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
        a: SecretKey,
        s_a: Scalar,
        B: PublicKey,
        encrypted_signature: EncryptedSignature,
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
}

impl Watchable for BtcEmergencyRefund {
    fn id(&self) -> Txid {
        self.txid()
    }

    fn script(&self) -> Script {
        self.watch_script.clone()
    }
}
