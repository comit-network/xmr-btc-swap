use crate::monero::{
    Amount, CreateWalletForOutput, InsufficientFunds, PrivateViewKey, PublicViewKey, Transfer,
    TransferProof, TxHash, WatchForTransfer,
};
use ::monero::{Address, Network, PrivateKey, PublicKey};
use anyhow::Result;
use async_trait::async_trait;
use backoff::{backoff::Constant as ConstantBackoff, tokio::retry};
use bitcoin::hashes::core::sync::atomic::AtomicU32;
use monero_harness::rpc::wallet;
use std::{
    str::FromStr,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tracing::info;
use url::Url;

#[derive(Debug)]
pub struct Wallet {
    pub inner: wallet::Client,
    pub network: Network,
}

impl Wallet {
    pub fn new(url: Url, network: Network) -> Self {
        Self {
            inner: wallet::Client::new(url),
            network,
        }
    }

    /// Get the balance of the primary account.
    pub async fn get_balance(&self) -> Result<Amount> {
        let amount = self.inner.get_balance(0).await?;

        Ok(Amount::from_piconero(amount))
    }
}

#[async_trait]
impl Transfer for Wallet {
    async fn transfer(
        &self,
        public_spend_key: PublicKey,
        public_view_key: PublicViewKey,
        amount: Amount,
    ) -> Result<(TransferProof, Amount)> {
        let destination_address =
            Address::standard(self.network, public_spend_key, public_view_key.into());

        let res = self
            .inner
            .transfer(0, amount.as_piconero(), &destination_address.to_string())
            .await?;

        let tx_hash = TxHash(res.tx_hash);
        tracing::info!("Monero tx broadcasted!, tx hash: {:?}", tx_hash);
        let tx_key = PrivateKey::from_str(&res.tx_key)?;

        let fee = Amount::from_piconero(res.fee);

        let transfer_proof = TransferProof::new(tx_hash, tx_key);
        tracing::debug!("  Transfer proof: {:?}", transfer_proof);

        Ok((transfer_proof, fee))
    }
}

#[async_trait]
impl CreateWalletForOutput for Wallet {
    async fn create_and_load_wallet_for_output(
        &self,
        private_spend_key: PrivateKey,
        private_view_key: PrivateViewKey,
        restore_height: Option<u32>,
    ) -> Result<()> {
        let public_spend_key = PublicKey::from_private_key(&private_spend_key);
        let public_view_key = PublicKey::from_private_key(&private_view_key.into());

        let address = Address::standard(self.network, public_spend_key, public_view_key);

        let _ = self
            .inner
            .generate_from_keys(
                &address.to_string(),
                &private_spend_key.to_string(),
                &PrivateKey::from(private_view_key).to_string(),
                restore_height,
            )
            .await?;

        Ok(())
    }
}

// TODO: For retry, use `backoff::ExponentialBackoff` in production as opposed
// to `ConstantBackoff`.
#[async_trait]
impl WatchForTransfer for Wallet {
    async fn watch_for_transfer(
        &self,
        public_spend_key: PublicKey,
        public_view_key: PublicViewKey,
        transfer_proof: TransferProof,
        expected_amount: Amount,
        expected_confirmations: u32,
    ) -> Result<(), InsufficientFunds> {
        enum Error {
            TxNotFound,
            InsufficientConfirmations,
            InsufficientFunds { expected: Amount, actual: Amount },
        }

        let address = Address::standard(self.network, public_spend_key, public_view_key.into());
        let wallet = self.inner.clone();

        let confirmations = Arc::new(AtomicU32::new(0u32));

        let res = retry(ConstantBackoff::new(Duration::from_secs(1)), || async {
            // NOTE: Currently, this is conflicting IO errors with the transaction not being
            // in the blockchain yet, or not having enough confirmations on it. All these
            // errors warrant a retry, but the strategy should probably differ per case
            let proof = wallet
                .check_tx_key(
                    &String::from(transfer_proof.tx_hash()),
                    &transfer_proof.tx_key().to_string(),
                    &address.to_string(),
                )
                .await
                .map_err(|_| backoff::Error::Transient(Error::TxNotFound))?;

            if proof.received != expected_amount.as_piconero() {
                return Err(backoff::Error::Permanent(Error::InsufficientFunds {
                    expected: expected_amount,
                    actual: Amount::from_piconero(proof.received),
                }));
            }

            if proof.confirmations > confirmations.load(Ordering::SeqCst) {
                confirmations.store(proof.confirmations, Ordering::SeqCst);
                info!(
                    "Monero lock tx received {} out of {} confirmations",
                    proof.confirmations, expected_confirmations
                );
            }

            if proof.confirmations < expected_confirmations {
                return Err(backoff::Error::Transient(Error::InsufficientConfirmations));
            }

            Ok(proof)
        })
        .await;

        if let Err(Error::InsufficientFunds { expected, actual }) = res {
            return Err(InsufficientFunds { expected, actual });
        };

        Ok(())
    }
}
