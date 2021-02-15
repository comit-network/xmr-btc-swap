use crate::bitcoin::{
    build_shared_output_descriptor, Address, Amount, BuildTxLockPsbt, GetNetwork, PublicKey,
    Transaction, TX_FEE,
};
use ::bitcoin::{util::psbt::PartiallySignedTransaction, OutPoint, TxIn, TxOut, Txid};
use anyhow::Result;
use miniscript::{Descriptor, NullCtx};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TxLock {
    inner: Transaction,
    pub(in crate::bitcoin) output_descriptor: Descriptor<::bitcoin::PublicKey>,
}

impl TxLock {
    pub async fn new<W>(wallet: &W, amount: Amount, A: PublicKey, B: PublicKey) -> Result<Self>
    where
        W: BuildTxLockPsbt + GetNetwork,
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

    pub fn build_spend_transaction(
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
