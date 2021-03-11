use crate::monero::{
    Amount, InsufficientFunds, PrivateViewKey, PublicViewKey, TransferProof, TxHash,
};
use ::monero::{Address, Network, PrivateKey, PublicKey};
use anyhow::{Context, Result};
use monero_rpc::wallet;
use monero_rpc::wallet::{BlockHeight, CheckTxKey, Refreshed};
use std::cmp::min;
use std::future::Future;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Interval;
use tracing::{debug, info};
use url::Url;

#[derive(Debug)]
pub struct Wallet {
    inner: Mutex<wallet::Client>,
    network: Network,
    name: String,
    avg_block_time: Duration,
}

impl Wallet {
    pub fn new(url: Url, network: Network, name: String, avg_block_time: Duration) -> Self {
        Self {
            inner: Mutex::new(wallet::Client::new(url)),
            network,
            name,
            avg_block_time,
        }
    }

    pub fn new_with_client(
        client: wallet::Client,
        network: Network,
        name: String,
        avg_block_time: Duration,
    ) -> Self {
        Self {
            inner: Mutex::new(client),
            network,
            name,
            avg_block_time,
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
        expected: Amount,
        conf_target: u32,
    ) -> Result<(), InsufficientFunds> {
        let txid = transfer_proof.tx_hash();

        tracing::info!(%txid, "Waiting for {} confirmation{} of Monero transaction", conf_target, if conf_target > 1 { "s" } else { "" });

        let address = Address::standard(self.network, public_spend_key, public_view_key.into());

        let check_interval =
            tokio::time::interval(min(self.avg_block_time / 10, Duration::from_secs(1)));
        let key = &transfer_proof.tx_key().to_string();

        wait_for_confirmations(
            txid.0,
            |txid| async move {
                self.inner
                    .lock()
                    .await
                    .check_tx_key(&txid, &key, &address.to_string())
                    .await
            },
            check_interval,
            expected,
            conf_target,
        )
        .await?;

        Ok(())
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

    pub fn static_tx_fee_estimate(&self) -> Amount {
        // Median tx fees on Monero as found here: https://www.monero.how/monero-transaction-fees, 0.000_015 * 2 (to be on the safe side)
        Amount::from_monero(0.000_03f64).expect("static fee to be convertible without problems")
    }
}

async fn wait_for_confirmations<Fut>(
    txid: String,
    fetch_tx: impl Fn(String) -> Fut,
    mut check_interval: Interval,
    expected: Amount,
    conf_target: u32,
) -> Result<(), InsufficientFunds>
where
    Fut: Future<Output = Result<CheckTxKey>>,
{
    let mut seen_confirmations = 0u32;

    while seen_confirmations < conf_target {
        let tx = match fetch_tx(txid.clone()).await {
            Ok(proof) => proof,
            Err(error) => {
                tracing::debug!(%txid, "Failed to retrieve tx from blockchain: {:#}", error);
                continue; // treating every error as transient and retrying
                          // is obviously wrong but the jsonrpc client is
                          // too primitive to differentiate between all the
                          // cases
            }
        };

        let received = Amount::from_piconero(tx.received);

        if received != expected {
            return Err(InsufficientFunds {
                expected,
                actual: received,
            });
        }

        if tx.confirmations > seen_confirmations {
            seen_confirmations = tx.confirmations;
            info!(%txid, "Monero lock tx has {} out of {} confirmations", tx.confirmations, conf_target);
        }

        check_interval.tick().await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use monero_rpc::wallet::CheckTxKey;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn given_exact_confirmations_does_not_fetch_tx_again() {
        let requests = Arc::new(AtomicU32::new(0));

        let result = wait_for_confirmations(
            String::from("TXID"),
            move |_| {
                let requests = requests.clone();

                async move {
                    match requests.fetch_add(1, Ordering::SeqCst) {
                        0 => Ok(CheckTxKey {
                            confirmations: 10,
                            received: 100,
                        }),
                        _ => panic!("should not be called more than once"),
                    }
                }
            },
            tokio::time::interval(Duration::from_millis(10)),
            Amount::from_piconero(100),
            10,
        )
        .await;

        assert!(result.is_ok())
    }

    /// A test that allows us to easily, visually verify if the log output is as
    /// we desire.
    ///
    /// We want the following properties:
    /// - Only print confirmations if they changed i.e. not every time we
    ///   request them
    /// - Also print the last one, i.e. 10 / 10
    #[tokio::test]
    async fn visual_log_check() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        const MAX_REQUESTS: u32 = 20;

        let requests = Arc::new(AtomicU32::new(0));

        let result = wait_for_confirmations(
            String::from("TXID"),
            move |_| {
                let requests = requests.clone();

                async move {
                    match requests.fetch_add(1, Ordering::SeqCst) {
                        requests if requests <= MAX_REQUESTS => {
                            Ok(CheckTxKey {
                                confirmations: requests / 2, /* every 2nd request "yields" a
                                                              * confirmation */
                                received: 100,
                            })
                        }
                        _ => panic!("should not be called more than {} times", MAX_REQUESTS),
                    }
                }
            },
            tokio::time::interval(Duration::from_millis(10)),
            Amount::from_piconero(100),
            10,
        )
        .await;

        assert!(result.is_ok())
    }
}
