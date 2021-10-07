use crate::bitcoin::wallet::Watchable;
use crate::bitcoin::{self, Address, Amount, PunishTimelock, Transaction, TxCancel, Txid};
use ::bitcoin::util::bip143::SigHashCache;
use ::bitcoin::{SigHash, SigHashType};
use anyhow::{Context, Result};
use bdk::bitcoin::Script;
use bdk::miniscript::{Descriptor, DescriptorTrait};
use std::collections::HashMap;

#[derive(Debug)]
pub struct TxPunish {
    inner: Transaction,
    digest: SigHash,
    cancel_output_descriptor: Descriptor<::bitcoin::PublicKey>,
    watch_script: Script,
}

impl TxPunish {
    pub fn new(
        tx_cancel: &TxCancel,
        punish_address: &Address,
        punish_timelock: PunishTimelock,
        spending_fee: Amount,
    ) -> Self {
        let tx_punish =
            tx_cancel.build_spend_transaction(punish_address, Some(punish_timelock), spending_fee);

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
            watch_script: punish_address.script_pubkey(),
        }
    }

    pub fn digest(&self) -> SigHash {
        self.digest
    }

    pub fn complete(
        self,
        tx_punish_sig_bob: bitcoin::Signature,
        a: bitcoin::SecretKey,
        B: bitcoin::PublicKey,
    ) -> Result<Transaction> {
        let sig_a = a.sign(self.digest());
        let sig_b = tx_punish_sig_bob;

        let satisfier = {
            let mut satisfier = HashMap::with_capacity(2);

            let A = a.public().into();
            let B = B.into();

            // The order in which these are inserted doesn't matter
            satisfier.insert(A, (sig_a.into(), ::bitcoin::SigHashType::All));
            satisfier.insert(B, (sig_b.into(), ::bitcoin::SigHashType::All));

            satisfier
        };

        let mut tx_punish = self.inner;
        self.cancel_output_descriptor
            .satisfy(&mut tx_punish.input[0], satisfier)
            .context("Failed to satisfy inputs with given signatures")?;

        Ok(tx_punish)
    }

    pub fn weight() -> usize {
        548
    }
}

impl Watchable for TxPunish {
    fn id(&self) -> Txid {
        self.inner.txid()
    }

    fn script(&self) -> Script {
        self.watch_script.clone()
    }
}
