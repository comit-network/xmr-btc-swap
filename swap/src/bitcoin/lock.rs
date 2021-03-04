use crate::bitcoin::{
    build_shared_output_descriptor, Address, Amount, PublicKey, Transaction, Wallet, TX_FEE,
};
use ::bitcoin::util::psbt::PartiallySignedTransaction;
use ::bitcoin::{OutPoint, TxIn, TxOut, Txid};
use anyhow::Result;
use ecdsa_fun::fun::Point;
use miniscript::{Descriptor, DescriptorTrait};
use rand::thread_rng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TxLock {
    inner: PartiallySignedTransaction,
    pub(in crate::bitcoin) output_descriptor: Descriptor<::bitcoin::PublicKey>,
}

impl TxLock {
    pub async fn new(wallet: &Wallet, amount: Amount, A: PublicKey, B: PublicKey) -> Result<Self> {
        let lock_output_descriptor = build_shared_output_descriptor(A.0, B.0);
        let address = lock_output_descriptor
            .address(wallet.get_network().await)
            .expect("can derive address from descriptor");

        let psbt = wallet.send_to_address(address, amount).await?;

        Ok(Self {
            inner: psbt,
            output_descriptor: lock_output_descriptor,
        })
    }

    pub fn lock_amount(&self) -> Amount {
        Amount::from_sat(self.inner.clone().extract_tx().output[self.lock_output_vout()].value)
    }

    pub fn txid(&self) -> Txid {
        self.inner.clone().extract_tx().txid()
    }

    pub fn as_outpoint(&self) -> OutPoint {
        // This is fine because a transaction that has that many outputs is not
        // realistic
        #[allow(clippy::cast_possible_truncation)]
        OutPoint::new(self.txid(), self.lock_output_vout() as u32)
    }

    /// Calculate the size of the script used by this transaction.
    pub fn script_size() -> usize {
        build_shared_output_descriptor(
            Point::random(&mut thread_rng()),
            Point::random(&mut thread_rng()),
        )
        .script_pubkey()
        .len()
    }

    /// Retreive the index of the locked output in the transaction outputs
    /// vector
    fn lock_output_vout(&self) -> usize {
        self.inner
            .clone()
            .extract_tx()
            .output
            .iter()
            .position(|output| output.script_pubkey == self.output_descriptor.script_pubkey())
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
            value: self.inner.clone().extract_tx().output[self.lock_output_vout()].value - TX_FEE,
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
        from.inner
    }
}
