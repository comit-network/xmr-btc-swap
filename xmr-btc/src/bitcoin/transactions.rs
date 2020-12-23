use crate::bitcoin::{
    build_shared_output_descriptor, verify_sig, BuildTxLockPsbt, Network, OutPoint, PublicKey,
    Txid, TX_FEE,
};
use anyhow::{bail, Context, Result};
use bitcoin::{
    util::{bip143::SigHashCache, psbt::PartiallySignedTransaction},
    Address, Amount, SigHash, SigHashType, Transaction, TxIn, TxOut,
};
use ecdsa_fun::Signature;
use miniscript::{Descriptor, NullCtx};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TxLock {
    inner: Transaction,
    output_descriptor: Descriptor<::bitcoin::PublicKey>,
}

impl TxLock {
    pub async fn new<W>(wallet: &W, amount: Amount, A: PublicKey, B: PublicKey) -> Result<Self>
    where
        W: BuildTxLockPsbt + Network,
    {
        let lock_output_descriptor = build_shared_output_descriptor(A.0, B.0);
        let address = lock_output_descriptor
            .address(wallet.get_network(), NullCtx)
            .expect("can derive address from descriptor");

        // We construct a psbt for convenience
        let psbt = wallet.build_tx_lock_psbt(address, amount).await?;

        // We don't take advantage of psbt functionality yet, instead we convert to a
        // raw transaction
        let inner = psbt.extract_tx();

        Ok(Self {
            inner,
            output_descriptor: lock_output_descriptor,
        })
    }

    pub fn lock_amount(&self) -> Amount {
        Amount::from_sat(self.inner.output[self.lock_output_vout()].value)
    }

    pub fn txid(&self) -> Txid {
        self.inner.txid()
    }

    pub fn as_outpoint(&self) -> OutPoint {
        // This is fine because a transaction that has that many outputs is not
        // realistic
        #[allow(clippy::cast_possible_truncation)]
        OutPoint::new(self.inner.txid(), self.lock_output_vout() as u32)
    }

    /// Retreive the index of the locked output in the transaction outputs
    /// vector
    fn lock_output_vout(&self) -> usize {
        self.inner
            .output
            .iter()
            .position(|output| {
                output.script_pubkey == self.output_descriptor.script_pubkey(NullCtx)
            })
            .expect("transaction contains lock output")
    }

    fn build_spend_transaction(
        &self,
        spend_address: &Address,
        sequence: Option<u32>,
    ) -> Transaction {
        let previous_output = self.as_outpoint();

        let tx_in = TxIn {
            previous_output,
            script_sig: Default::default(),
            sequence: sequence.unwrap_or(0xFFFF_FFFF),
            witness: Vec::new(),
        };

        let tx_out = TxOut {
            value: self.inner.output[self.lock_output_vout()].value - TX_FEE,
            script_pubkey: spend_address.script_pubkey(),
        };

        Transaction {
            version: 2,
            lock_time: 0,
            input: vec![tx_in],
            output: vec![tx_out],
        }
    }
}

impl From<TxLock> for PartiallySignedTransaction {
    fn from(from: TxLock) -> Self {
        PartiallySignedTransaction::from_unsigned_tx(from.inner).expect("to be unsigned")
    }
}

#[derive(Debug, Clone)]
pub struct TxRedeem {
    inner: Transaction,
    digest: SigHash,
}

impl TxRedeem {
    pub fn new(tx_lock: &TxLock, redeem_address: &Address) -> Self {
        // lock_input is the shared output that is now being used as an input for the
        // redeem transaction
        let tx_redeem = tx_lock.build_spend_transaction(redeem_address, None);

        let digest = SigHashCache::new(&tx_redeem).signature_hash(
            0, // Only one input: lock_input (lock transaction)
            &tx_lock.output_descriptor.witness_script(NullCtx),
            tx_lock.lock_amount().as_sat(),
            SigHashType::All,
        );

        Self {
            inner: tx_redeem,
            digest,
        }
    }

    pub fn txid(&self) -> Txid {
        self.inner.txid()
    }

    pub fn digest(&self) -> SigHash {
        self.digest
    }

    pub fn add_signatures(
        self,
        tx_lock: &TxLock,
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

        let mut tx_redeem = self.inner;
        tx_lock
            .output_descriptor
            .satisfy(&mut tx_redeem.input[0], satisfier, NullCtx)?;

        Ok(tx_redeem)
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
            .context("neither signature on witness stack verifies against B")?;

        Ok(sig)
    }
}

#[derive(Clone, Copy, thiserror::Error, Debug)]
#[error("transaction does not spend anything")]
pub struct NoInputs;

#[derive(Clone, Copy, thiserror::Error, Debug)]
#[error("transaction has {0} inputs, expected 1")]
pub struct TooManyInputs(usize);

#[derive(Clone, Copy, thiserror::Error, Debug)]
#[error("empty witness stack")]
pub struct EmptyWitnessStack;

#[derive(Clone, Copy, thiserror::Error, Debug)]
#[error("input has {0} witnesses, expected 3")]
pub struct NotThreeWitnesses(usize);

#[derive(Debug, Clone)]
pub struct TxCancel {
    inner: Transaction,
    digest: SigHash,
    output_descriptor: Descriptor<::bitcoin::PublicKey>,
}

impl TxCancel {
    pub fn new(tx_lock: &TxLock, cancel_timelock: u32, A: PublicKey, B: PublicKey) -> Self {
        let cancel_output_descriptor = build_shared_output_descriptor(A.0, B.0);

        let tx_in = TxIn {
            previous_output: tx_lock.as_outpoint(),
            script_sig: Default::default(),
            sequence: cancel_timelock,
            witness: Vec::new(),
        };

        let tx_out = TxOut {
            value: tx_lock.lock_amount().as_sat() - TX_FEE,
            script_pubkey: cancel_output_descriptor.script_pubkey(NullCtx),
        };

        let transaction = Transaction {
            version: 2,
            lock_time: 0,
            input: vec![tx_in],
            output: vec![tx_out],
        };

        let digest = SigHashCache::new(&transaction).signature_hash(
            0, // Only one input: lock_input (lock transaction)
            &tx_lock.output_descriptor.witness_script(NullCtx),
            tx_lock.lock_amount().as_sat(),
            SigHashType::All,
        );

        Self {
            inner: transaction,
            digest,
            output_descriptor: cancel_output_descriptor,
        }
    }

    pub fn txid(&self) -> Txid {
        self.inner.txid()
    }

    pub fn digest(&self) -> SigHash {
        self.digest
    }

    fn amount(&self) -> Amount {
        Amount::from_sat(self.inner.output[0].value)
    }

    pub fn as_outpoint(&self) -> OutPoint {
        OutPoint::new(self.inner.txid(), 0)
    }

    pub fn add_signatures(
        self,
        tx_lock: &TxLock,
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

        let mut tx_cancel = self.inner;
        tx_lock
            .output_descriptor
            .satisfy(&mut tx_cancel.input[0], satisfier, NullCtx)?;

        Ok(tx_cancel)
    }

    fn build_spend_transaction(
        &self,
        spend_address: &Address,
        sequence: Option<u32>,
    ) -> Transaction {
        let previous_output = self.as_outpoint();

        let tx_in = TxIn {
            previous_output,
            script_sig: Default::default(),
            sequence: sequence.unwrap_or(0xFFFF_FFFF),
            witness: Vec::new(),
        };

        let tx_out = TxOut {
            value: self.amount().as_sat() - TX_FEE,
            script_pubkey: spend_address.script_pubkey(),
        };

        Transaction {
            version: 2,
            lock_time: 0,
            input: vec![tx_in],
            output: vec![tx_out],
        }
    }
}

#[derive(Debug)]
pub struct TxRefund {
    inner: Transaction,
    digest: SigHash,
}

impl TxRefund {
    pub fn new(tx_cancel: &TxCancel, refund_address: &Address) -> Self {
        let tx_punish = tx_cancel.build_spend_transaction(refund_address, None);

        let digest = SigHashCache::new(&tx_punish).signature_hash(
            0, // Only one input: cancel transaction
            &tx_cancel.output_descriptor.witness_script(NullCtx),
            tx_cancel.amount().as_sat(),
            SigHashType::All,
        );

        Self {
            inner: tx_punish,
            digest,
        }
    }

    pub fn txid(&self) -> Txid {
        self.inner.txid()
    }

    pub fn digest(&self) -> SigHash {
        self.digest
    }

    pub fn add_signatures(
        self,
        tx_cancel: &TxCancel,
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

        let mut tx_refund = self.inner;
        tx_cancel
            .output_descriptor
            .satisfy(&mut tx_refund.input[0], satisfier, NullCtx)?;

        Ok(tx_refund)
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
            .context("neither signature on witness stack verifies against B")?;

        Ok(sig)
    }
}

#[derive(Debug)]
pub struct TxPunish {
    inner: Transaction,
    digest: SigHash,
}

impl TxPunish {
    pub fn new(tx_cancel: &TxCancel, punish_address: &Address, punish_timelock: u32) -> Self {
        let tx_punish = tx_cancel.build_spend_transaction(punish_address, Some(punish_timelock));

        let digest = SigHashCache::new(&tx_punish).signature_hash(
            0, // Only one input: cancel transaction
            &tx_cancel.output_descriptor.witness_script(NullCtx),
            tx_cancel.amount().as_sat(),
            SigHashType::All,
        );

        Self {
            inner: tx_punish,
            digest,
        }
    }

    pub fn txid(&self) -> Txid {
        self.inner.txid()
    }

    pub fn digest(&self) -> SigHash {
        self.digest
    }

    pub fn add_signatures(
        self,
        tx_cancel: &TxCancel,
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
        tx_cancel
            .output_descriptor
            .satisfy(&mut tx_punish.input[0], satisfier, NullCtx)?;

        Ok(tx_punish)
    }
}
