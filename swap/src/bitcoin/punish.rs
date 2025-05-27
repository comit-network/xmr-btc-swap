use crate::bitcoin::wallet::Watchable;
use crate::bitcoin::{self, Address, Amount, PunishTimelock, Transaction, TxCancel, Txid};
use ::bitcoin::sighash::SighashCache;
use ::bitcoin::{secp256k1, sighash::SegwitV0Sighash as Sighash, EcdsaSighashType};
use ::bitcoin::{ScriptBuf, Weight};
use anyhow::{Context, Result};
use bdk_wallet::miniscript::Descriptor;
use std::collections::HashMap;

#[derive(Debug)]
pub struct TxPunish {
    inner: Transaction,
    digest: Sighash,
    cancel_output_descriptor: Descriptor<::bitcoin::PublicKey>,
    watch_script: ScriptBuf,
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

        let digest = SighashCache::new(&tx_punish)
            .p2wsh_signature_hash(
                0, // Only one input: cancel transaction
                &tx_cancel
                    .output_descriptor
                    .script_code()
                    .expect("scriptcode"),
                tx_cancel.amount(),
                EcdsaSighashType::All,
            )
            .expect("sighash");

        Self {
            inner: tx_punish,
            digest,
            cancel_output_descriptor: tx_cancel.output_descriptor.clone(),
            watch_script: punish_address.script_pubkey(),
        }
    }

    pub fn digest(&self) -> Sighash {
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

            let A = a.public().try_into()?;
            let B = B.try_into()?;

            let sig_a = secp256k1::ecdsa::Signature::from_compact(&sig_a.to_bytes())?;
            let sig_b = secp256k1::ecdsa::Signature::from_compact(&sig_b.to_bytes())?;
            // The order in which these are inserted doesn't matter
            satisfier.insert(
                A,
                ::bitcoin::ecdsa::Signature {
                    signature: sig_a,
                    sighash_type: EcdsaSighashType::All,
                },
            );
            satisfier.insert(
                B,
                ::bitcoin::ecdsa::Signature {
                    signature: sig_b,
                    sighash_type: EcdsaSighashType::All,
                },
            );

            satisfier
        };

        let mut tx_punish = self.inner;
        self.cancel_output_descriptor
            .satisfy(&mut tx_punish.input[0], satisfier)
            .context("Failed to satisfy inputs with given signatures")?;

        Ok(tx_punish)
    }

    pub fn weight() -> Weight {
        Weight::from_wu(548)
    }
}

impl Watchable for TxPunish {
    fn id(&self) -> Txid {
        self.inner.compute_txid()
    }

    fn script(&self) -> ScriptBuf {
        self.watch_script.clone()
    }
}
