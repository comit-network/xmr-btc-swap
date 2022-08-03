use crate::env::Config;
use crate::monero::{
    Amount, InsufficientFunds, PrivateViewKey, PublicViewKey, TransferProof, TxHash,
};
use ::monero::{Address, Network, PrivateKey, PublicKey};
use anyhow::{Context, Result};
use monero_rpc::wallet::{BlockHeight, MoneroWalletRpc as _, Refreshed};
use monero_rpc::{jsonrpc, wallet};
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
        let client = wallet::Client::new(url)?;

        let open_wallet_response = client.open_wallet(name.clone()).await;
        if open_wallet_response.is_err() {
            client.create_wallet(name.clone(), "English".to_owned()).await.context(
                "Unable to create Monero wallet, please ensure that the monero-wallet-rpc is available",
            )?;

            tracing::debug!(monero_wallet_name = %name, "Created Monero wallet");
        } else {
            tracing::debug!(monero_wallet_name = %name, "Opened Monero wallet");
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
                        tracing::info!(
                            %tx,
                            monero_address = %self.main_address,
                            "Monero transferred back to default wallet");
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        address = %self.main_address,
                        "Failed to transfer Monero to default wallet: {:#}", error
                    );
                }
            },
            Err(error) => {
                tracing::warn!("Failed to refresh generated wallet: {:#}", error);
            }
        }

        let _ = wallet.open_wallet(self.name.clone()).await?;

        Ok(())
    }

    pub async fn transfer(&self, request: TransferRequest) -> Result<TransferProof> {
        let inner = self.inner.lock().await;

        inner
            .open_wallet(self.name.clone())
            .await
            .with_context(|| format!("Failed to open wallet {}", self.name))?;

        let TransferRequest {
            public_spend_key,
            public_view_key,
            amount,
        } = request;

        let destination_address =
            Address::standard(self.network, public_spend_key, public_view_key.into());

        let res = inner
            .transfer_single(0, amount.as_piconero(), &destination_address.to_string())
            .await?;

        tracing::debug!(
            %amount,
            to = %public_spend_key,
            tx_id = %res.tx_hash,
            "Successfully initiated Monero transfer"
        );

        Ok(TransferProof::new(
            TxHash(res.tx_hash),
            res.tx_key
                .context("Missing tx_key in `transfer` response")?,
        ))
    }

    pub async fn watch_for_transfer(&self, request: WatchRequest) -> Result<(), InsufficientFunds> {
        let WatchRequest {
            conf_target,
            public_view_key,
            public_spend_key,
            transfer_proof,
            expected,
        } = request;

        let txid = transfer_proof.tx_hash();

        tracing::info!(
            %txid,
            target_confirmations = %conf_target,
            "Waiting for Monero transaction finality"
        );

        let address = Address::standard(self.network, public_spend_key, public_view_key.into());

        let check_interval = tokio::time::interval(self.sync_interval);

        wait_for_confirmations(
            &self.inner,
            transfer_proof,
            address,
            expected,
            conf_target,
            check_interval,
            self.name.clone(),
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

async fn wait_for_confirmations<C: monero_rpc::wallet::MoneroWalletRpc<reqwest::Client> + Sync>(
    client: &Mutex<C>,
    transfer_proof: TransferProof,
    to_address: Address,
    expected: Amount,
    conf_target: u64,
    mut check_interval: Interval,
    wallet_name: String,
) -> Result<(), InsufficientFunds> {
    let mut seen_confirmations = 0u64;

    while seen_confirmations < conf_target {
        check_interval.tick().await; // tick() at the beginning of the loop so every `continue` tick()s as well

        let txid = transfer_proof.tx_hash().to_string();
        let client = client.lock().await;

        let tx = match client
            .check_tx_key(
                txid.clone(),
                transfer_proof.tx_key.to_string(),
                to_address.to_string(),
            )
            .await
        {
            Ok(proof) => proof,
            Err(jsonrpc::Error::JsonRpc(jsonrpc::JsonRpcError {
                code: -1,
                message,
                data,
            })) => {
                tracing::debug!(message, ?data);
                tracing::warn!(%txid, message, "`monero-wallet-rpc` failed to fetch transaction, may need to be restarted");
                continue;
            }
            // TODO: Implement this using a generic proxy for each function call once https://github.com/thomaseizinger/rust-jsonrpc-client/issues/47 is fixed.
            Err(jsonrpc::Error::JsonRpc(jsonrpc::JsonRpcError { code: -13, .. })) => {
                tracing::debug!(
                    "Opening wallet `{}` because no wallet is loaded",
                    wallet_name
                );
                let _ = client.open_wallet(wallet_name.clone()).await;
                continue;
            }
            Err(other) => {
                tracing::debug!(
                    %txid,
                    "Failed to retrieve tx from blockchain: {:#}", other
                );
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
            tracing::info!(
                %txid,
                %seen_confirmations,
                needed_confirmations = %conf_target,
                "Received new confirmation for Monero lock tx"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracing_ext::capture_logs;
    use monero_rpc::wallet::CheckTxKey;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tracing::metadata::LevelFilter;

    #[tokio::test]
    async fn given_exact_confirmations_does_not_fetch_tx_again() {
        let client = Mutex::new(DummyClient::new(vec![Ok(CheckTxKey {
            confirmations: 10,
            received: 100,
        })]));

        let result = wait_for_confirmations(
            &client,
            TransferProof::new(TxHash("<FOO>".to_owned()), PrivateKey {
                scalar: crate::monero::Scalar::random(&mut rand::thread_rng())
            }),
            "53H3QthYLckeCXh9u38vohb2gZ4QgEG3FMWHNxccR6MqV1LdDVYwF1FKsRJPj4tTupWLf9JtGPBcn2MVN6c9oR7p5Uf7JdJ".parse().unwrap(),
            Amount::from_piconero(100),
            10,
            tokio::time::interval(Duration::from_millis(10)),
            "foo-wallet".to_owned()
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(
            client
                .lock()
                .await
                .check_tx_key_invocations
                .load(Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn visual_log_check() {
        let writer = capture_logs(LevelFilter::INFO);

        let client = Mutex::new(DummyClient::new(vec![
            Ok(CheckTxKey {
                confirmations: 1,
                received: 100,
            }),
            Ok(CheckTxKey {
                confirmations: 1,
                received: 100,
            }),
            Ok(CheckTxKey {
                confirmations: 1,
                received: 100,
            }),
            Ok(CheckTxKey {
                confirmations: 3,
                received: 100,
            }),
            Ok(CheckTxKey {
                confirmations: 5,
                received: 100,
            }),
        ]));

        wait_for_confirmations(
            &client,
            TransferProof::new(TxHash("<FOO>".to_owned()), PrivateKey {
                scalar: crate::monero::Scalar::random(&mut rand::thread_rng())
            }),
            "53H3QthYLckeCXh9u38vohb2gZ4QgEG3FMWHNxccR6MqV1LdDVYwF1FKsRJPj4tTupWLf9JtGPBcn2MVN6c9oR7p5Uf7JdJ".parse().unwrap(),
            Amount::from_piconero(100),
            5,
            tokio::time::interval(Duration::from_millis(10)),
            "foo-wallet".to_owned()
        )
        .await
        .unwrap();

        assert_eq!(
            writer.captured(),
            r" INFO swap::monero::wallet: Received new confirmation for Monero lock tx txid=<FOO> seen_confirmations=1 needed_confirmations=5
 INFO swap::monero::wallet: Received new confirmation for Monero lock tx txid=<FOO> seen_confirmations=3 needed_confirmations=5
 INFO swap::monero::wallet: Received new confirmation for Monero lock tx txid=<FOO> seen_confirmations=5 needed_confirmations=5
"
        );
    }

    #[tokio::test]
    async fn reopens_wallet_in_case_not_available() {
        let writer = capture_logs(LevelFilter::DEBUG);

        let client = Mutex::new(DummyClient::new(vec![
            Ok(CheckTxKey {
                confirmations: 1,
                received: 100,
            }),
            Ok(CheckTxKey {
                confirmations: 1,
                received: 100,
            }),
            Err((-13, "No wallet file".to_owned())),
            Ok(CheckTxKey {
                confirmations: 3,
                received: 100,
            }),
            Ok(CheckTxKey {
                confirmations: 5,
                received: 100,
            }),
        ]));

        wait_for_confirmations(
            &client,
            TransferProof::new(TxHash("<FOO>".to_owned()), PrivateKey {
                scalar: crate::monero::Scalar::random(&mut rand::thread_rng())
            }),
            "53H3QthYLckeCXh9u38vohb2gZ4QgEG3FMWHNxccR6MqV1LdDVYwF1FKsRJPj4tTupWLf9JtGPBcn2MVN6c9oR7p5Uf7JdJ".parse().unwrap(),
            Amount::from_piconero(100),
            5,
            tokio::time::interval(Duration::from_millis(10)),
            "foo-wallet".to_owned()
        )
        .await
        .unwrap();

        assert_eq!(
            writer.captured(),
            r" INFO swap::monero::wallet: Received new confirmation for Monero lock tx txid=<FOO> seen_confirmations=1 needed_confirmations=5
DEBUG swap::monero::wallet: Opening wallet `foo-wallet` because no wallet is loaded
 INFO swap::monero::wallet: Received new confirmation for Monero lock tx txid=<FOO> seen_confirmations=3 needed_confirmations=5
 INFO swap::monero::wallet: Received new confirmation for Monero lock tx txid=<FOO> seen_confirmations=5 needed_confirmations=5
"
        );
        assert_eq!(
            client
                .lock()
                .await
                .open_wallet_invocations
                .load(Ordering::SeqCst),
            1
        );
    }

    type ErrorCode = i64;
    type ErrorMessage = String;

    struct DummyClient {
        check_tx_key_responses: Vec<Result<wallet::CheckTxKey, (ErrorCode, ErrorMessage)>>,

        check_tx_key_invocations: AtomicU32,
        open_wallet_invocations: AtomicU32,
    }

    impl DummyClient {
        fn new(
            check_tx_key_responses: Vec<Result<wallet::CheckTxKey, (ErrorCode, ErrorMessage)>>,
        ) -> Self {
            Self {
                check_tx_key_responses,
                check_tx_key_invocations: Default::default(),
                open_wallet_invocations: Default::default(),
            }
        }
    }

    #[async_trait::async_trait]
    impl monero_rpc::wallet::MoneroWalletRpc<reqwest::Client> for DummyClient {
        async fn open_wallet(
            &self,
            _: String,
        ) -> Result<wallet::WalletOpened, monero_rpc::jsonrpc::Error<reqwest::Error>> {
            self.open_wallet_invocations.fetch_add(1, Ordering::SeqCst);

            Ok(monero_rpc::wallet::Empty {})
        }

        async fn check_tx_key(
            &self,
            _: String,
            _: String,
            _: String,
        ) -> Result<wallet::CheckTxKey, monero_rpc::jsonrpc::Error<reqwest::Error>> {
            let index = self.check_tx_key_invocations.fetch_add(1, Ordering::SeqCst);

            self.check_tx_key_responses[index as usize]
                .clone()
                .map_err(|(code, message)| {
                    monero_rpc::jsonrpc::Error::JsonRpc(monero_rpc::jsonrpc::JsonRpcError {
                        code,
                        message,
                        data: None,
                    })
                })
        }

        async fn send_request<P>(
            &self,
            _: String,
        ) -> Result<monero_rpc::jsonrpc::Response<P>, reqwest::Error>
        where
            P: serde::de::DeserializeOwned,
        {
            todo!()
        }
    }
}
