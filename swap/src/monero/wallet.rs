use crate::monero::{
    Amount, InsufficientFunds, PrivateViewKey, PublicViewKey, TransferProof, TxHash,
};
use ::monero::{Address, Network, PrivateKey, PublicKey};
use anyhow::{Context, Result};
use backoff::{backoff::Constant as ConstantBackoff, future::retry};
use bitcoin::hashes::core::sync::atomic::AtomicU32;
use monero_rpc::{
    wallet,
    wallet::{BlockHeight, Refreshed},
};
use std::{str::FromStr, sync::atomic::Ordering, time::Duration};
use tokio::sync::Mutex;
use tracing::{debug, info};
use url::Url;

#[derive(Debug)]
pub struct Wallet {
    inner: Mutex<wallet::Client>,
    network: Network,
    name: String,
}

impl Wallet {
    pub fn new(url: Url, network: Network, name: String) -> Self {
        Self {
            inner: Mutex::new(wallet::Client::new(url)),
            network,
            name,
        }
    }

    pub fn new_with_client(client: wallet::Client, network: Network, name: String) -> Self {
        Self {
            inner: Mutex::new(client),
            network,
            name,
        }
    }

    pub async fn open(&self) -> Result<()> {
        self.inner
            .lock()
            .await
            .open_wallet(self.name.as_str())
            .await?;
        Ok(())
    }

    pub async fn open_or_create(&self) -> Result<()> {
        let open_wallet_response = self.open().await;
        if open_wallet_response.is_err() {
            self.inner.lock().await.create_wallet(self.name.as_str()).await.context(
                "Unable to create Monero wallet, please ensure that the monero-wallet-rpc is available",
            )?;

            debug!("Created Monero wallet {}", self.name);
        } else {
            debug!("Opened Monero wallet {}", self.name);
        }

        Ok(())
    }

    pub async fn create_from_and_load(
        &self,
        private_spend_key: PrivateKey,
        private_view_key: PrivateViewKey,
        restore_height: BlockHeight,
    ) -> Result<()> {
        let public_spend_key = PublicKey::from_private_key(&private_spend_key);
        let public_view_key = PublicKey::from_private_key(&private_view_key.into());

        let address = Address::standard(self.network, public_spend_key, public_view_key);

        let wallet = self.inner.lock().await;

        // Properly close the wallet before generating the other wallet to ensure that
        // it saves its state correctly
        let _ = wallet.close_wallet().await?;

        let _ = wallet
            .generate_from_keys(
                &address.to_string(),
                &private_spend_key.to_string(),
                &PrivateKey::from(private_view_key).to_string(),
                restore_height.height,
            )
            .await?;

        Ok(())
    }

    pub async fn create_from(
        &self,
        private_spend_key: PrivateKey,
        private_view_key: PrivateViewKey,
        restore_height: BlockHeight,
    ) -> Result<()> {
        let public_spend_key = PublicKey::from_private_key(&private_spend_key);
        let public_view_key = PublicKey::from_private_key(&private_view_key.into());

        let address = Address::standard(self.network, public_spend_key, public_view_key);

        let wallet = self.inner.lock().await;

        // Properly close the wallet before generating the other wallet to ensure that
        // it saves its state correctly
        let _ = wallet.close_wallet().await?;

        let _ = wallet
            .generate_from_keys(
                &address.to_string(),
                &private_spend_key.to_string(),
                &PrivateKey::from(private_view_key).to_string(),
                restore_height.height,
            )
            .await?;

        let _ = wallet.open_wallet(self.name.as_str()).await?;

        Ok(())
    }

    /// Get the balance of the primary account.
    pub async fn get_balance(&self) -> Result<Amount> {
        let amount = self.inner.lock().await.get_balance(0).await?;

        Ok(Amount::from_piconero(amount))
    }

    pub async fn block_height(&self) -> Result<BlockHeight> {
        self.inner.lock().await.block_height().await
    }

    pub async fn get_main_address(&self) -> Result<Address> {
        let address = self.inner.lock().await.get_address(0).await?;
        Ok(Address::from_str(address.address.as_str())?)
    }

    pub async fn refresh(&self) -> Result<Refreshed> {
        self.inner.lock().await.refresh().await
    }

    pub async fn sweep_all(&self, address: Address) -> Result<Vec<TxHash>> {
        let sweep_all = self
            .inner
            .lock()
            .await
            .sweep_all(address.to_string().as_str())
            .await?;

        let tx_hashes = sweep_all.tx_hash_list.into_iter().map(TxHash).collect();
        Ok(tx_hashes)
    }

    pub fn static_tx_fee_estimate(&self) -> Amount {
        // Median tx fees on Monero as found here: https://www.monero.how/monero-transaction-fees, 0.000_015 * 2 (to be on the safe side)
        Amount::from_monero(0.000_03f64).expect("static fee to be convertible without problems")
    }

    pub async fn transfer(
        &self,
        public_spend_key: PublicKey,
        public_view_key: PublicViewKey,
        amount: Amount,
    ) -> Result<TransferProof> {
        let destination_address =
            Address::standard(self.network, public_spend_key, public_view_key.into());

        let res = self
            .inner
            .lock()
            .await
            .transfer(0, amount.as_piconero(), &destination_address.to_string())
            .await?;

        tracing::debug!(
            "sent transfer of {} to {} in {}",
            amount,
            public_spend_key,
            res.tx_hash
        );

        Ok(TransferProof::new(
            TxHash(res.tx_hash),
            PrivateKey::from_str(&res.tx_key)?,
        ))
    }

    pub async fn watch_for_transfer(
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

        let confirmations = AtomicU32::new(0u32);

        let res = retry(ConstantBackoff::new(Duration::from_secs(1)), || async {
            // NOTE: Currently, this is conflicting IO errors with the transaction not being
            // in the blockchain yet, or not having enough confirmations on it. All these
            // errors warrant a retry, but the strategy should probably differ per case
            let proof = self
                .inner
                .lock()
                .await
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
