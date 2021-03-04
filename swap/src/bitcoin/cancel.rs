use crate::bitcoin::{
    build_shared_output_descriptor, Address, Amount, BlockHeight, PublicKey, Transaction, TxLock,
    TX_FEE,
};
use ::bitcoin::util::bip143::SigHashCache;
use ::bitcoin::{OutPoint, SigHash, SigHashType, TxIn, TxOut, Txid};
use anyhow::Result;
use ecdsa_fun::Signature;
use miniscript::{Descriptor, DescriptorTrait};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Add;

/// Represent a timelock, expressed in relative block height as defined in
/// [BIP68](https://github.com/bitcoin/bips/blob/master/bip-0068.mediawiki).
/// E.g. The timelock expires 10 blocks after the reference transaction is
/// mined.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(transparent)]
pub struct CancelTimelock(u32);

impl CancelTimelock {
    pub const fn new(number_of_blocks: u32) -> Self {
        Self(number_of_blocks)
    }
}

impl Add<CancelTimelock> for BlockHeight {
    type Output = BlockHeight;

    fn add(self, rhs: CancelTimelock) -> Self::Output {
        self + rhs.0
    }
}

/// Represent a timelock, expressed in relative block height as defined in
/// [BIP68](https://github.com/bitcoin/bips/blob/master/bip-0068.mediawiki).
/// E.g. The timelock expires 10 blocks after the reference transaction is
/// mined.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(transparent)]
pub struct PunishTimelock(u32);

impl PunishTimelock {
    pub const fn new(number_of_blocks: u32) -> Self {
        Self(number_of_blocks)
    }
}

impl Add<PunishTimelock> for BlockHeight {
    type Output = BlockHeight;

    fn add(self, rhs: PunishTimelock) -> Self::Output {
        self + rhs.0
    }
}

#[derive(Debug, Clone)]
pub struct TxCancel {
    inner: Transaction,
    digest: SigHash,
    pub(in crate::bitcoin) output_descriptor: Descriptor<::bitcoin::PublicKey>,
    lock_output_descriptor: Descriptor<::bitcoin::PublicKey>,
}

impl TxCancel {
    pub fn new(
        tx_lock: &TxLock,
        cancel_timelock: CancelTimelock,
        A: PublicKey,
        B: PublicKey,
    ) -> Self {
        let cancel_output_descriptor = build_shared_output_descriptor(A.0, B.0);

        let tx_in = TxIn {
            previous_output: tx_lock.as_outpoint(),
            script_sig: Default::default(),
            sequence: cancel_timelock.0,
            witness: Vec::new(),
        };

        let tx_out = TxOut {
            value: tx_lock.lock_amount().as_sat() - TX_FEE,
            script_pubkey: cancel_output_descriptor.script_pubkey(),
        };

        let transaction = Transaction {
            version: 2,
            lock_time: 0,
            input: vec![tx_in],
            output: vec![tx_out],
        };

        let digest = SigHashCache::new(&transaction).signature_hash(
            0, // Only one input: lock_input (lock transaction)
            &tx_lock.output_descriptor.script_code(),
            tx_lock.lock_amount().as_sat(),
            SigHashType::All,
        );

        Self {
            inner: transaction,
            digest,
            output_descriptor: cancel_output_descriptor,
            lock_output_descriptor: tx_lock.output_descriptor.clone(),
        }
    }

    pub fn txid(&self) -> Txid {
        self.inner.txid()
    }

    pub fn digest(&self) -> SigHash {
        self.digest
    }

    pub fn amount(&self) -> Amount {
        Amount::from_sat(self.inner.output[0].value)
    }

    pub fn as_outpoint(&self) -> OutPoint {
        OutPoint::new(self.inner.txid(), 0)
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

        let mut tx_cancel = self.inner;
        self.lock_output_descriptor
            .satisfy(&mut tx_cancel.input[0], satisfier)?;

        Ok(tx_cancel)
    }

    pub fn build_spend_transaction(
        &self,
        spend_address: &Address,
        sequence: Option<PunishTimelock>,
    ) -> Transaction {
        let previous_output = self.as_outpoint();

        let tx_in = TxIn {
            previous_output,
            script_sig: Default::default(),
            sequence: sequence.map(|seq| seq.0).unwrap_or(0xFFFF_FFFF),
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
