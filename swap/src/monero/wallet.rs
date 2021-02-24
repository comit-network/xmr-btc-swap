use crate::monero::{
    Amount, CreateWallet, CreateWalletForOutput, CreateWalletForOutputThenLoadDefaultWallet,
    GetAddress, InsufficientFunds, OpenWallet, PrivateViewKey, PublicViewKey, Refresh, Transfer,
    TransferProof, TxHash, WalletBlockHeight, WatchForTransfer,
};
use ::monero::{Address, Network, PrivateKey, PublicKey};
use anyhow::Result;
use async_trait::async_trait;
use backoff::{backoff::Constant as ConstantBackoff, future::retry};
use bitcoin::hashes::core::sync::atomic::AtomicU32;
use monero_rpc::{
    wallet,
    wallet::{BlockHeight, Refreshed},
};
use std::{
    str::FromStr,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::sync::Mutex;
use tracing::info;
use url::Url;

#[derive(Debug)]
pub struct Wallet {
    inner: Mutex<wallet::Client>,
    network: Network,
    default_wallet_name: String,
}

impl Wallet {
    pub fn new(url: Url, network: Network, default_wallet_name: String) -> Self {
        Self {
            inner: Mutex::new(wallet::Client::new(url)),
            network,
            default_wallet_name,
        }
    }

    pub fn new_with_client(
        client: wallet::Client,
        network: Network,
        default_wallet_name: String,
    ) -> Self {
        Self {
            inner: Mutex::new(client),
            network,
            default_wallet_name,
        }
    }

    /// Get the balance of the primary account.
    pub async fn get_balance(&self) -> Result<Amount> {
        let amount = self.inner.lock().await.get_balance(0).await?;

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
            .lock()
            .await
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
        restore_height: BlockHeight,
    ) -> Result<()> {
        let public_spend_key = PublicKey::from_private_key(&private_spend_key);
        let public_view_key = PublicKey::from_private_key(&private_view_key.into());

        let address = Address::standard(self.network, public_spend_key, public_view_key);

        let _ = self
            .inner
            .lock()
            .await
            .generate_from_keys(
                &address.to_string(),
                &private_spend_key.to_string(),
                &PrivateKey::from(private_view_key).to_string(),
                restore_height.height,
            )
            .await?;

        Ok(())
    }
}

#[async_trait]
impl CreateWalletForOutputThenLoadDefaultWallet for Wallet {
    async fn create_and_load_wallet_for_output_then_load_default_wallet(
        &self,
        private_spend_key: PrivateKey,
        private_view_key: PrivateViewKey,
        restore_height: BlockHeight,
    ) -> Result<()> {
        let public_spend_key = PublicKey::from_private_key(&private_spend_key);
        let public_view_key = PublicKey::from_private_key(&private_view_key.into());

        let address = Address::standard(self.network, public_spend_key, public_view_key);

        let wallet = self.inner.lock().await;

        let _ = wallet
            .generate_from_keys(
                &address.to_string(),
                &private_spend_key.to_string(),
                &PrivateKey::from(private_view_key).to_string(),
                restore_height.height,
            )
            .await?;

        let _ = wallet
            .open_wallet(self.default_wallet_name.as_str())
            .await?;

        Ok(())
    }
}

#[async_trait]
impl OpenWallet for Wallet {
    async fn open_wallet(&self, file_name: &str) -> Result<()> {
        self.inner.lock().await.open_wallet(file_name).await?;
        Ok(())
    }
}

#[async_trait]
impl CreateWallet for Wallet {
    async fn create_wallet(&self, file_name: &str) -> Result<()> {
        self.inner.lock().await.create_wallet(file_name).await?;
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

        let confirmations = Arc::new(AtomicU32::new(0u32));

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

#[async_trait]
impl WalletBlockHeight for Wallet {
    async fn block_height(&self) -> Result<BlockHeight> {
        self.inner.lock().await.block_height().await
    }
}

#[async_trait]
impl GetAddress for Wallet {
    async fn get_main_address(&self) -> Result<Address> {
        let address = self.inner.lock().await.get_address(0).await?;
        Ok(Address::from_str(address.address.as_str())?)
    }
}

#[async_trait]
impl Refresh for Wallet {
    async fn refresh(&self) -> Result<Refreshed> {
        self.inner.lock().await.refresh().await
    }
}
