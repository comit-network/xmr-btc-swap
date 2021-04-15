use crate::env::Config;
use crate::monero::{
    Amount, InsufficientFunds, PrivateViewKey, PublicViewKey, TransferProof, TxHash,
};
use ::monero::{Address, Network, PrivateKey, PublicKey};
use anyhow::{Context, Result};
use monero_rpc::wallet;
use monero_rpc::wallet::{BlockHeight, CheckTxKey, MoneroWalletRpc as _, Refreshed};
use std::future::Future;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Interval;
use url::Url;

#[derive(Debug)]
pub struct Wallet {
    inner: Mutex<wallet::Client>,
    network: Network,
    name: String,
    main_address: monero::Address,
    sync_interval: Duration,
}

impl Wallet {
    /// Connect to a wallet RPC and load the given wallet by name.
    pub async fn open_or_create(url: Url, name: String, env_config: Config) -> Result<Self> {
        let client = wallet::Client::new(url);

        let open_wallet_response = client.open_wallet(name.clone()).await;
        if open_wallet_response.is_err() {
            client.create_wallet(name.clone(), "English".to_owned()).await.context(
                "Unable to create Monero wallet, please ensure that the monero-wallet-rpc is available",
            )?;

            tracing::debug!("Created Monero wallet {}", name);
        } else {
            tracing::debug!("Opened Monero wallet {}", name);
        }

        Self::connect(client, name, env_config).await
    }

    /// Connects to a wallet RPC where a wallet is already loaded.
    pub async fn connect(client: wallet::Client, name: String, env_config: Config) -> Result<Self> {
        let main_address =
            monero::Address::from_str(client.get_address(0).await?.address.as_str())?;
        Ok(Self {
            inner: Mutex::new(client),
            network: env_config.monero_network,
            name,
            main_address,
            sync_interval: env_config.monero_sync_interval(),
        })
    }

    /// Re-open the wallet using the internally stored name.
    pub async fn re_open(&self) -> Result<()> {
        self.inner
            .lock()
            .await
            .open_wallet(self.name.clone())
            .await?;
        Ok(())
    }

    pub async fn open(&self, filename: String) -> Result<()> {
        self.inner.lock().await.open_wallet(filename).await?;
        Ok(())
    }

    /// Close the wallet and open (load) another wallet by generating it from
    /// keys. The generated wallet will remain loaded.
    pub async fn create_from_and_load(
        &self,
        file_name: String,
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
        let _ = wallet
            .close_wallet()
            .await
            .context("Failed to close wallet")?;

        let _ = wallet
            .generate_from_keys(
                file_name,
                address.to_string(),
                private_spend_key.to_string(),
                PrivateKey::from(private_view_key).to_string(),
                restore_height.height,
                String::from(""),
                true,
            )
            .await
            .context("Failed to generate new wallet from keys")?;

        Ok(())
    }

    /// Close the wallet and open (load) another wallet by generating it from
    /// keys. The generated wallet will be opened, all funds sweeped to the
    /// main_address and then the wallet will be re-loaded using the internally
    /// stored name.
    pub async fn create_from(
        &self,
        file_name: String,
        private_spend_key: PrivateKey,
        private_view_key: PrivateViewKey,
        restore_height: BlockHeight,
    ) -> Result<()> {
        let public_spend_key = PublicKey::from_private_key(&private_spend_key);
        let public_view_key = PublicKey::from_private_key(&private_view_key.into());

        let temp_wallet_address =
            Address::standard(self.network, public_spend_key, public_view_key);

        let wallet = self.inner.lock().await;

        // Close the default wallet before generating the other wallet to ensure that
        // it saves its state correctly
        let _ = wallet.close_wallet().await?;

        let _ = wallet
            .generate_from_keys(
                file_name,
                temp_wallet_address.to_string(),
                private_spend_key.to_string(),
                PrivateKey::from(private_view_key).to_string(),
                restore_height.height,
                String::from(""),
                true,
            )
            .await?;

        // Try to send all the funds from the generated wallet to the default wallet
        match wallet.refresh().await {
            Ok(_) => match wallet.sweep_all(self.main_address.to_string()).await {
                Ok(sweep_all) => {
                    for tx in sweep_all.tx_hash_list {
                        tracing::info!(%tx, "Monero transferred back to default wallet {}", self.main_address);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Transferring Monero back to default wallet {} failed with {:#}",
                        self.main_address,
                        e
                    );
                }
            },
            Err(e) => {
                tracing::warn!("Refreshing the generated wallet failed with {:#}", e);
            }
        }

        let _ = wallet.open_wallet(self.name.clone()).await?;

        Ok(())
    }

    pub async fn transfer(&self, request: TransferRequest) -> Result<TransferProof> {
        let TransferRequest {
            public_spend_key,
            public_view_key,
            amount,
        } = request;

        let destination_address =
            Address::standard(self.network, public_spend_key, public_view_key.into());

        let res = self
            .inner
            .lock()
            .await
            .transfer_single(0, amount.as_piconero(), &destination_address.to_string())
            .await?;

        tracing::debug!(
            "sent transfer of {} to {} in {}",
            amount,
            public_spend_key,
            res.tx_hash
        );

        Ok(TransferProof::new(
            TxHash(res.tx_hash),
            res.tx_key
                .context("Missing tx_key in `transfer` response")?,
        ))
    }

    pub async fn watch_for_transfer(&self, request: WatchRequest) -> Result<()> {
        let WatchRequest {
            conf_target,
            public_view_key,
            public_spend_key,
            transfer_proof,
            expected,
        } = request;

        let txid = transfer_proof.tx_hash();

        tracing::info!(%txid, "Waiting for {} confirmation{} of Monero transaction", conf_target, if conf_target > 1 { "s" } else { "" });

        let address = Address::standard(self.network, public_spend_key, public_view_key.into());

        let check_interval = tokio::time::interval(self.sync_interval);
        let key = transfer_proof.tx_key().to_string();

        wait_for_confirmations(
            txid.0,
            move |txid| {
                let key = key.clone();
                async move {
                    Ok(self
                        .inner
                        .lock()
                        .await
                        .check_tx_key(txid, key, address.to_string())
                        .await?)
                }
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
            .sweep_all(address.to_string())
            .await?;

        let tx_hashes = sweep_all.tx_hash_list.into_iter().map(TxHash).collect();
        Ok(tx_hashes)
    }

    /// Get the balance of the primary account.
    pub async fn get_balance(&self) -> Result<Amount> {
        let amount = self.inner.lock().await.get_balance(0).await?.balance;

        Ok(Amount::from_piconero(amount))
    }

    pub async fn block_height(&self) -> Result<BlockHeight> {
        Ok(self.inner.lock().await.get_height().await?)
    }

    pub fn get_main_address(&self) -> Address {
        self.main_address
    }

    pub async fn refresh(&self) -> Result<Refreshed> {
        Ok(self.inner.lock().await.refresh().await?)
    }

    pub fn static_tx_fee_estimate(&self) -> Amount {
        // Median tx fees on Monero as found here: https://www.monero.how/monero-transaction-fees, 0.000_015 * 2 (to be on the safe side)
        Amount::from_monero(0.000_03f64).expect("static fee to be convertible without problems")
    }
}

#[derive(Debug)]
pub struct TransferRequest {
    pub public_spend_key: PublicKey,
    pub public_view_key: PublicViewKey,
    pub amount: Amount,
}

#[derive(Debug)]
pub struct WatchRequest {
    pub public_spend_key: PublicKey,
    pub public_view_key: PublicViewKey,
    pub transfer_proof: TransferProof,
    pub conf_target: u64,
    pub expected: Amount,
}

async fn wait_for_confirmations<Fut>(
    txid: String,
    fetch_tx: impl Fn(String) -> Fut,
    mut check_interval: Interval,
    expected: Amount,
    conf_target: u64,
) -> Result<(), InsufficientFunds>
where
    Fut: Future<Output = Result<CheckTxKey>>,
{
    let mut seen_confirmations = 0u64;

    while seen_confirmations < conf_target {
        check_interval.tick().await; // tick() at the beginning of the loop so every `continue` tick()s as well

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
            tracing::info!(%txid, "Monero lock tx has {} out of {} confirmations", tx.confirmations, conf_target);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use monero_rpc::wallet::CheckTxKey;
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
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
        const MAX_REQUESTS: u64 = 20;

        let requests = Arc::new(AtomicU64::new(0));

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
