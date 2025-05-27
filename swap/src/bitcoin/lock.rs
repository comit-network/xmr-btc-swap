use crate::bitcoin::wallet::Watchable;
use crate::bitcoin::{
    build_shared_output_descriptor, Address, Amount, PublicKey, Transaction, Wallet,
};
use ::bitcoin::psbt::Psbt as PartiallySignedTransaction;
use ::bitcoin::{OutPoint, TxIn, TxOut, Txid};
use anyhow::{bail, Context, Result};
use bdk_wallet::miniscript::Descriptor;
use bdk_wallet::psbt::PsbtUtils;
use bitcoin::{locktime::absolute::LockTime as PackedLockTime, ScriptBuf, Sequence};
use serde::{Deserialize, Serialize};

use super::wallet::EstimateFeeRate;

const SCRIPT_SIZE: usize = 34;
const TX_LOCK_WEIGHT: usize = 485;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxLock {
    inner: PartiallySignedTransaction,
    pub(in crate::bitcoin) output_descriptor: Descriptor<::bitcoin::PublicKey>,
}

impl TxLock {
    pub async fn new(
        wallet: &Wallet<
            bdk_wallet::rusqlite::Connection,
            impl EstimateFeeRate + Send + Sync + 'static,
        >,
        amount: Amount,
        spending_fee: Amount,
        A: PublicKey,
        B: PublicKey,
        change: bitcoin::Address,
    ) -> Result<Self> {
        let lock_output_descriptor = build_shared_output_descriptor(A.0, B.0)?;
        let address = lock_output_descriptor
            .address(wallet.network())
            .expect("can derive address from descriptor");

        let psbt = wallet
            .send_to_address(address, amount, spending_fee, Some(change))
            .await?;

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
        let shared_output_candidate = match psbt.unsigned_tx.output.as_slice() {
            [shared_output_candidate, _] if shared_output_candidate.value == btc => {
                shared_output_candidate
            }
            [_, shared_output_candidate] if shared_output_candidate.value == btc => {
                shared_output_candidate
            }
            // A single output is possible if Bob funds without any change necessary
            [shared_output_candidate] if shared_output_candidate.value == btc => {
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

        let descriptor = build_shared_output_descriptor(A.0, B.0)?;
        let legit_shared_output_script = descriptor.script_pubkey();

        if shared_output_candidate.script_pubkey != legit_shared_output_script {
            bail!("Output script is not a shared output")
        }

        Ok(TxLock {
            inner: psbt,
            output_descriptor: descriptor,
        })
    }

    pub fn lock_amount(&self) -> Amount {
        self.inner.clone().extract_tx_unchecked_fee_rate().output[self.lock_output_vout()].value
    }

    pub fn fee(&self) -> Result<Amount> {
        self.inner
            .clone()
            .fee_amount()
            .context("The PSBT is missing a TxOut for an input")
    }

    pub fn txid(&self) -> Txid {
        self.inner
            .clone()
            .extract_tx_unchecked_fee_rate()
            .compute_txid()
    }

    pub fn as_outpoint(&self) -> OutPoint {
        // This is fine because a transaction that has that many outputs is not
        // realistic
        #[allow(clippy::cast_possible_truncation)]
        OutPoint::new(self.txid(), self.lock_output_vout() as u32)
    }

    /// Calculate the size of the script used by this transaction.
    pub fn script_size() -> usize {
        SCRIPT_SIZE
    }

    pub fn script_pubkey(&self) -> ScriptBuf {
        self.output_descriptor.script_pubkey()
    }

    /// Retreive the index of the locked output in the transaction outputs
    /// vector
    fn lock_output_vout(&self) -> usize {
        self.inner
            .clone()
            .extract_tx_unchecked_fee_rate()
            .output
            .iter()
            .position(|output| output.script_pubkey == self.output_descriptor.script_pubkey())
            .expect("transaction contains lock output")
    }

    pub fn build_spend_transaction(
        &self,
        spend_address: &Address,
        sequence: Option<u32>,
        spending_fee: Amount,
    ) -> Transaction {
        let previous_output = self.as_outpoint();

        let sequence = Sequence(sequence.unwrap_or(0xFFFF_FFFF));
        let tx_in = TxIn {
            previous_output,
            script_sig: Default::default(),
            sequence,
            witness: Default::default(),
        };

        let tx_out = TxOut {
            value: self.inner.clone().extract_tx_unchecked_fee_rate().output
                [self.lock_output_vout()]
            .value
                - spending_fee,
            script_pubkey: spend_address.script_pubkey(),
        };

        tracing::debug!(fee=%spending_fee.to_sat(), "Constructed Bitcoin spending transaction");

        Transaction {
            version: bitcoin::transaction::Version(2),
            lock_time: PackedLockTime::from_height(0).expect("0 to be below lock time threshold"),
            input: vec![tx_in],
            output: vec![tx_out],
        }
    }

    pub fn weight() -> ::bitcoin::Weight {
        ::bitcoin::Weight::from_wu(TX_LOCK_WEIGHT as u64)
    }
}

impl From<TxLock> for PartiallySignedTransaction {
    fn from(from: TxLock) -> Self {
        from.inner
    }
}

impl Watchable for TxLock {
    fn id(&self) -> Txid {
        self.txid()
    }

    fn script(&self) -> ScriptBuf {
        self.output_descriptor.script_pubkey()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitcoin::wallet::TestWalletBuilder;
    use crate::bitcoin::Amount;
    use ::bitcoin::psbt::Psbt as PartiallySignedTransaction;

    // Basic setup function for tests
    async fn setup() -> (
        PublicKey,
        PublicKey,
        Wallet<bdk_wallet::rusqlite::Connection, crate::bitcoin::wallet::StaticFeeRate>,
    ) {
        let (A, B) = alice_and_bob();
        let wallet = TestWalletBuilder::new(100_000).build().await;
        (A, B, wallet)
    }

    #[tokio::test]
    async fn given_bob_sends_good_psbt_when_reconstructing_then_succeeeds() {
        let (A, B, wallet) = setup().await;
        let agreed_amount = Amount::from_sat(10000);
        let spending_fee = Amount::from_sat(1000);

        let psbt = bob_make_psbt(A, B, &wallet, agreed_amount, spending_fee).await;
        let result = TxLock::from_psbt(psbt, A, B, agreed_amount);

        result.expect("PSBT to be valid");
    }

    #[tokio::test]
    async fn bob_can_fund_without_a_change_output() {
        let (A, B, _) = setup().await;
        let amount = 10_000;
        let agreed_amount = Amount::from_sat(amount);
        let spending_fee = Amount::from_sat(300);
        let wallet = TestWalletBuilder::new(amount + 300).build().await;

        let psbt = bob_make_psbt(A, B, &wallet, agreed_amount, spending_fee).await;
        assert_eq!(
            psbt.unsigned_tx.output.len(),
            1,
            "Expected no change output"
        );
    }

    #[tokio::test]
    async fn given_bob_is_sending_less_than_agreed_when_reconstructing_txlock_then_fails() {
        let (A, B, wallet) = setup().await;
        let agreed_amount = Amount::from_sat(10000);
        let spending_fee = Amount::from_sat(1000);

        let bad_amount = Amount::from_sat(5000);
        let psbt = bob_make_psbt(A, B, &wallet, bad_amount, spending_fee).await;
        let result = TxLock::from_psbt(psbt, A, B, agreed_amount);

        result.expect_err("PSBT to be invalid");
    }

    #[tokio::test]
    async fn given_bob_is_sending_to_a_bad_output_reconstructing_txlock_then_fails() {
        let (A, B, wallet) = setup().await;
        let agreed_amount = Amount::from_sat(10000);
        let spending_fee = Amount::from_sat(1000);

        let E = eve();
        let psbt = bob_make_psbt(E, B, &wallet, agreed_amount, spending_fee).await;
        let result = TxLock::from_psbt(psbt, A, B, agreed_amount);

        result.expect_err("PSBT to be invalid");
    }

    proptest::proptest! {
        #[test]
        fn estimated_tx_lock_script_size_never_changes(a in crate::proptest::ecdsa_fun::point(), b in crate::proptest::ecdsa_fun::point()) {
            proptest::prop_assume!(a != b);

            let computed_size = build_shared_output_descriptor(a, b).unwrap().script_pubkey().len();

            assert_eq!(computed_size, SCRIPT_SIZE);
        }
    }

    // Helper function for testing PSBT creation by Bob
    async fn bob_make_psbt(
        A: PublicKey,
        B: PublicKey,
        wallet: &Wallet<
            bdk_wallet::rusqlite::Connection,
            impl EstimateFeeRate + Send + Sync + 'static,
        >,
        amount: Amount,
        spending_fee: Amount,
    ) -> PartiallySignedTransaction {
        let change = wallet.new_address().await.unwrap();
        TxLock::new(wallet, amount, spending_fee, A, B, change)
            .await
            .unwrap()
            .into()
    }

    fn alice_and_bob() -> (PublicKey, PublicKey) {
        (PublicKey::random(), PublicKey::random())
    }

    fn eve() -> PublicKey {
        PublicKey::random()
    }
}
