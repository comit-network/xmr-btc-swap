use crate::bitcoin;
use ::bitcoin::sighash::SighashCache;
use ::bitcoin::{secp256k1, ScriptBuf};
use ::bitcoin::{sighash::SegwitV0Sighash as Sighash, EcdsaSighashType, Txid};
use anyhow::{Context, Result};
use bdk_wallet::miniscript::Descriptor;
use bitcoin::{Address, Amount, Transaction};
use std::collections::HashMap;

use super::wallet::Watchable;
use super::TxLock;

const TX_EARLY_REFUND_WEIGHT: usize = 548;

#[derive(Clone)]
pub struct TxEarlyRefund {
    inner: Transaction,
    digest: Sighash,
    lock_output_descriptor: Descriptor<::bitcoin::PublicKey>,
    watch_script: ScriptBuf,
}

impl TxEarlyRefund {
    pub fn new(tx_lock: &TxLock, refund_address: &Address, spending_fee: Amount) -> Self {
        let tx = tx_lock.build_spend_transaction(refund_address, None, spending_fee);

        let digest = SighashCache::new(&tx)
            .p2wsh_signature_hash(
                0,
                &tx_lock
                    .output_descriptor
                    .script_code()
                    .expect("TxLock should have a script code"),
                tx_lock.lock_amount(),
                EcdsaSighashType::All,
            )
            .expect("sighash");

        Self {
            inner: tx,
            digest,
            lock_output_descriptor: tx_lock.output_descriptor.clone(),
            watch_script: refund_address.script_pubkey(),
        }
    }

    pub fn txid(&self) -> Txid {
        self.inner.compute_txid()
    }

    pub fn digest(&self) -> Sighash {
        self.digest
    }

    pub fn complete(
        self,
        tx_early_refund_sig: bitcoin::Signature,
        a: bitcoin::SecretKey,
        B: bitcoin::PublicKey,
    ) -> Result<Transaction> {
        let sig_a = a.sign(self.digest());
        let sig_b = tx_early_refund_sig;

        self.add_signatures((a.public(), sig_a), (B, sig_b))
    }

    fn add_signatures(
        self,
        (A, sig_a): (bitcoin::PublicKey, bitcoin::Signature),
        (B, sig_b): (bitcoin::PublicKey, bitcoin::Signature),
    ) -> Result<Transaction> {
        let satisfier = {
            let mut satisfier = HashMap::with_capacity(2);

            let A = ::bitcoin::PublicKey {
                compressed: true,
                inner: secp256k1::PublicKey::from_slice(&A.0.to_bytes())?,
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

        let mut tx_early_refund = self.inner;
        self.lock_output_descriptor
            .satisfy(&mut tx_early_refund.input[0], satisfier)
            .context("Failed to satisfy inputs with given signatures")?;

        Ok(tx_early_refund)
    }

    pub fn weight() -> usize {
        TX_EARLY_REFUND_WEIGHT
    }
}

impl Watchable for TxEarlyRefund {
    fn id(&self) -> Txid {
        self.txid()
    }

    fn script(&self) -> ScriptBuf {
        self.watch_script.clone()
    }
}
