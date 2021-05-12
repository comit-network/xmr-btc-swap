use crate::bitcoin::wallet::Watchable;
use crate::bitcoin::{
    build_shared_output_descriptor, Address, Amount, PartiallySignedTransaction, PublicKey,
    Transaction, Txid, Wallet, TX_FEE,
};
use anyhow::{bail, Result};
use bdk::bitcoin::{OutPoint, Script, TxIn, TxOut};
use bdk::database::BatchDatabase;
use bdk::descriptor::Descriptor;
use ecdsa_fun::fun::Point;
use miniscript::DescriptorTrait;
use rand::thread_rng;

#[derive(Debug, Clone, PartialEq)]
pub struct BtcLock {
    inner: PartiallySignedTransaction,
    pub(crate) output_descriptor: Descriptor<::bitcoin::PublicKey>,
}

impl BtcLock {
    pub async fn new<B, D, C>(
        wallet: &Wallet<B, D, C>,
        amount: Amount,
        A: PublicKey,
        B: PublicKey,
    ) -> Result<Self>
    where
        D: BatchDatabase,
    {
        let lock_output_descriptor = build_shared_output_descriptor(A.into(), B.into());
        let address = lock_output_descriptor
            .address(wallet.get_network())
            .expect("can derive address from descriptor");

        let psbt = wallet.send_to_address(address, amount).await?;

        Ok(Self {
            inner: psbt,
            output_descriptor: lock_output_descriptor,
        })
    }

    /// Creates an instance of `TxLock` from a PSBT, the public keys of the
    /// parties and the specified amount.
    ///
    /// This function validates that the given PSBT does indeed pay that
    /// specified amount to a shared output.
    pub fn from_psbt(
        psbt: PartiallySignedTransaction,
        A: PublicKey,
        B: PublicKey,
        btc: Amount,
    ) -> Result<Self> {
        let shared_output_candidate = match psbt.global.unsigned_tx.output.as_slice() {
            [shared_output_candidate, _] if shared_output_candidate.value == btc.as_sat() => {
                shared_output_candidate
            }
            [_, shared_output_candidate] if shared_output_candidate.value == btc.as_sat() => {
                shared_output_candidate
            }
            // A single output is possible if Bob funds without any change necessary
            [shared_output_candidate] if shared_output_candidate.value == btc.as_sat() => {
                shared_output_candidate
            }
            [_, _] => {
                bail!("Neither of the two provided outputs pays the right amount!");
            }
            [_] => {
                bail!("The provided output does not pay the right amount!");
            }
            other => {
                let num_outputs = other.len();
                bail!(
                    "PSBT has {} outputs, expected one or two. Something is fishy!",
                    num_outputs
                );
            }
        };

        let descriptor = build_shared_output_descriptor(A.into(), B.into());
        let legit_shared_output_script = descriptor.script_pubkey();

        if shared_output_candidate.script_pubkey != legit_shared_output_script {
            bail!("Output script is not a shared output")
        }

        Ok(BtcLock {
            inner: psbt,
            output_descriptor: descriptor,
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

    pub fn script_pubkey(&self) -> Script {
        self.output_descriptor.script_pubkey()
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

impl From<BtcLock> for PartiallySignedTransaction {
    fn from(from: BtcLock) -> Self {
        from.inner
    }
}

impl Watchable for BtcLock {
    fn id(&self) -> Txid {
        self.txid()
    }

    fn script(&self) -> Script {
        self.output_descriptor.script_pubkey()
    }
}
