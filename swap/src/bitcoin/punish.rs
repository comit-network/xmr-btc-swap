use crate::bitcoin::{Address, PublicKey, PunishTimelock, Transaction, TxCancel};
use ::bitcoin::util::bip143::SigHashCache;
use ::bitcoin::{SigHash, SigHashType};
use anyhow::Result;
use ecdsa_fun::Signature;
use miniscript::{Descriptor, DescriptorTrait};
use std::collections::HashMap;

#[derive(Debug)]
pub struct TxPunish {
    inner: Transaction,
    digest: SigHash,
    cancel_output_descriptor: Descriptor<::bitcoin::PublicKey>,
}

impl TxPunish {
    pub fn new(
        tx_cancel: &TxCancel,
        punish_address: &Address,
        punish_timelock: PunishTimelock,
    ) -> Self {
        let tx_punish = tx_cancel.build_spend_transaction(punish_address, Some(punish_timelock));

        let digest = SigHashCache::new(&tx_punish).signature_hash(
            0, // Only one input: cancel transaction
            &tx_cancel.output_descriptor.script_code(),
            tx_cancel.amount().as_sat(),
            SigHashType::All,
        );

        Self {
            inner: tx_punish,
            digest,
            cancel_output_descriptor: tx_cancel.output_descriptor.clone(),
        }
    }

    pub fn digest(&self) -> SigHash {
        self.digest
    }

    pub fn add_signatures(
        self,
        (A, sig_a): (PublicKey, Signature),
        (B, sig_b): (PublicKey, Signature),
    ) -> Result<Transaction> {
        let satisfier = {
            let mut satisfier = HashMap::with_capacity(2);

            let A = ::bitcoin::PublicKey {
                compressed: true,
                key: A.0.into(),
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

        let mut tx_punish = self.inner;
        self.cancel_output_descriptor
            .satisfy(&mut tx_punish.input[0], satisfier)?;

        Ok(tx_punish)
    }
}
