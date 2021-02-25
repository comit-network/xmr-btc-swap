use crate::{
    bitcoin::{
        timelocks::BlockHeight, Address, Amount, BroadcastSignedTransaction, GetBlockHeight,
        GetRawTransaction, SignTxLock, Transaction, TransactionBlockHeight, TxLock,
        WaitForTransactionFinality, WatchForRawTransaction,
    },
    execution_params::ExecutionParams,
};
use ::bitcoin::{util::psbt::PartiallySignedTransaction, Txid};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use backoff::{backoff::Constant as ConstantBackoff, future::retry};
use bdk::{
    blockchain::{noop_progress, Blockchain, ElectrumBlockchain},
    electrum_client::{self, Client, ElectrumApi},
    miniscript::bitcoin::PrivateKey,
    FeeRate,
};
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::interval};

const SLED_TREE_NAME: &str = "default_tree";

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("Sending the request failed")]
    Io(reqwest::Error),
    #[error("Conversion to Integer failed")]
    Parse(std::num::ParseIntError),
    #[error("The transaction is not minded yet")]
    NotYetMined,
    #[error("Deserialization failed")]
    JsonDeserialization(reqwest::Error),
    #[error("Electrum client error")]
    ElectrumClient(electrum_client::Error),
}

pub struct Wallet {
    inner: Arc<Mutex<bdk::Wallet<ElectrumBlockchain, bdk::sled::Tree>>>,
    http_url: Url,
    rpc_url: Url,
}

impl Wallet {
    pub async fn new(
        electrum_rpc_url: Url,
        electrum_http_url: Url,
        network: bitcoin::Network,
        wallet_dir: &Path,
        private_key: PrivateKey,
    ) -> Result<Self> {
        // Workaround for https://github.com/bitcoindevkit/rust-electrum-client/issues/47.
        let config = electrum_client::ConfigBuilder::default().retry(2).build();

        let client = Client::from_config(electrum_rpc_url.as_str(), config)
            .map_err(|e| anyhow!("Failed to init electrum rpc client: {:?}", e))?;

        let db = bdk::sled::open(wallet_dir)?.open_tree(SLED_TREE_NAME)?;

        let bdk_wallet = bdk::Wallet::new(
            bdk::template::P2WPKH(private_key),
            None,
            network,
            db,
            ElectrumBlockchain::from(client),
        )?;

        Ok(Self {
            inner: Arc::new(Mutex::new(bdk_wallet)),
            http_url: electrum_http_url,
            rpc_url: electrum_rpc_url,
        })
    }

    pub async fn balance(&self) -> Result<Amount> {
        let balance = self.inner.lock().await.get_balance()?;
        Ok(Amount::from_sat(balance))
    }

    pub async fn new_address(&self) -> Result<Address> {
        let address = self.inner.lock().await.get_new_address()?;

        Ok(address)
    }

    pub async fn get_tx(&self, txid: Txid) -> Result<Option<Transaction>> {
        let tx = self.inner.lock().await.client().get_tx(&txid)?;
        Ok(tx)
    }

    pub async fn transaction_fee(&self, txid: Txid) -> Result<Amount> {
        let fees = self
            .inner
            .lock()
            .await
            .list_transactions(true)?
            .iter()
            .find(|tx| tx.txid == txid)
            .ok_or_else(|| {
                anyhow!("Could not find tx in bdk wallet when trying to determine fees")
            })?
            .fees;

        Ok(Amount::from_sat(fees))
    }

    pub async fn sync_wallet(&self) -> Result<()> {
        self.inner.lock().await.sync(noop_progress(), None)?;
        Ok(())
    }

    pub async fn send_to_address(
        &self,
        address: Address,
        amount: Amount,
    ) -> Result<PartiallySignedTransaction> {
        let wallet = self.inner.lock().await;

        let mut tx_builder = wallet.build_tx();
        tx_builder.add_recipient(address.script_pubkey(), amount.as_sat());
        tx_builder.fee_rate(FeeRate::from_sat_per_vb(5.0)); // todo: make dynamic
        let (psbt, _details) = tx_builder.finish()?;

        Ok(psbt)
    }

    pub async fn get_network(&self) -> bitcoin::Network {
        self.inner.lock().await.network()
    }
}

#[async_trait]
impl SignTxLock for Wallet {
    async fn sign_tx_lock(&self, tx_lock: TxLock) -> Result<Transaction> {
        let txid = tx_lock.txid();
        tracing::debug!("signing tx lock: {}", txid);
        let psbt = PartiallySignedTransaction::from(tx_lock);
        let (signed_psbt, finalized) = self.inner.lock().await.sign(psbt, None)?;
        if !finalized {
            bail!("Could not finalize TxLock psbt")
        }
        let tx = signed_psbt.extract_tx();
        tracing::debug!("signed tx lock: {}", txid);
        Ok(tx)
    }
}

#[async_trait]
impl BroadcastSignedTransaction for Wallet {
    async fn broadcast_signed_transaction(&self, transaction: Transaction) -> Result<Txid> {
        tracing::debug!("attempting to broadcast tx: {}", transaction.txid());
        self.inner.lock().await.broadcast(transaction.clone())?;
        tracing::info!("Bitcoin tx broadcasted! TXID = {}", transaction.txid());
        Ok(transaction.txid())
    }
}

#[async_trait]
impl WatchForRawTransaction for Wallet {
    async fn watch_for_raw_transaction(&self, txid: Txid) -> Result<Transaction> {
        tracing::debug!("watching for tx: {}", txid);
        let tx = retry(ConstantBackoff::new(Duration::from_secs(1)), || async {
            let client = Client::new(self.rpc_url.as_ref())
                .map_err(|err| backoff::Error::Permanent(Error::ElectrumClient(err)))?;

            let tx = client.transaction_get(&txid).map_err(|err| match err {
                electrum_client::Error::Protocol(err) => {
                    tracing::debug!("Received protocol error {} from Electrum, retrying...", err);
                    backoff::Error::Transient(Error::NotYetMined)
                }
                err => backoff::Error::Permanent(Error::ElectrumClient(err)),
            })?;

            Result::<_, backoff::Error<Error>>::Ok(tx)
        })
        .await
        .context("transient errors to be retried")?;

        Ok(tx)
    }
}

#[async_trait]
impl GetRawTransaction for Wallet {
    async fn get_raw_transaction(&self, txid: Txid) -> Result<Transaction> {
        self.get_tx(txid)
            .await?
            .ok_or_else(|| anyhow!("Could not get raw tx with id: {}", txid))
    }
}

#[async_trait]
impl GetBlockHeight for Wallet {
    async fn get_block_height(&self) -> Result<BlockHeight> {
        let url = blocks_tip_height_url(&self.http_url)?;
        let height = retry(ConstantBackoff::new(Duration::from_secs(1)), || async {
            let height = reqwest::Client::new()
                .request(Method::GET, url.clone())
                .send()
                .await
                .map_err(Error::Io)?
                .text()
                .await
                .map_err(Error::Io)?
                .parse::<u32>()
                .map_err(|err| backoff::Error::Permanent(Error::Parse(err)))?;
            Result::<_, backoff::Error<Error>>::Ok(height)
        })
        .await
        .context("transient errors to be retried")?;

        Ok(BlockHeight::new(height))
    }
}

#[async_trait]
impl TransactionBlockHeight for Wallet {
    async fn transaction_block_height(&self, txid: Txid) -> Result<BlockHeight> {
        let url = tx_status_url(txid, &self.http_url)?;
        #[derive(Serialize, Deserialize, Debug, Clone)]
        struct TransactionStatus {
            block_height: Option<u32>,
            confirmed: bool,
        }
        let height = retry(ConstantBackoff::new(Duration::from_secs(1)), || async {
            let resp = reqwest::Client::new()
                .request(Method::GET, url.clone())
                .send()
                .await
                .map_err(|err| backoff::Error::Transient(Error::Io(err)))?;

            let tx_status: TransactionStatus = resp
                .json()
                .await
                .map_err(|err| backoff::Error::Permanent(Error::JsonDeserialization(err)))?;

            let block_height = tx_status
                .block_height
                .ok_or(backoff::Error::Transient(Error::NotYetMined))?;

            Result::<_, backoff::Error<Error>>::Ok(block_height)
        })
        .await
        .context("transient errors to be retried")?;

        Ok(BlockHeight::new(height))
    }
}

#[async_trait]
impl WaitForTransactionFinality for Wallet {
    async fn wait_for_transaction_finality(
        &self,
        txid: Txid,
        execution_params: ExecutionParams,
    ) -> Result<()> {
        tracing::debug!("waiting for tx finality: {}", txid);
        // Divide by 4 to not check too often yet still be aware of the new block early
        // on.
        let mut interval = interval(execution_params.bitcoin_avg_block_time / 4);

        loop {
            let tx_block_height = self.transaction_block_height(txid).await?;
            tracing::debug!("tx_block_height: {:?}", tx_block_height);
            let block_height = self.get_block_height().await?;
            tracing::debug!("latest_block_height: {:?}", block_height);
            if let Some(confirmations) = block_height.checked_sub(
                tx_block_height
                    .checked_sub(BlockHeight::new(1))
                    .expect("transaction must be included in block with height >= 1"),
            ) {
                tracing::debug!("confirmations: {:?}", confirmations);
                if u32::from(confirmations) >= execution_params.bitcoin_finality_confirmations {
                    break;
                }
            }
            interval.tick().await;
        }

        Ok(())
    }
}

fn tx_status_url(txid: Txid, base_url: &Url) -> Result<Url> {
    let url = base_url.join(&format!("tx/{}/status", txid))?;
    Ok(url)
}

fn blocks_tip_height_url(base_url: &Url) -> Result<Url> {
    let url = base_url.join("blocks/tip/height")?;
    Ok(url)
}

#[cfg(test)]
mod tests {
    use crate::{
        bitcoin::{
            wallet::{blocks_tip_height_url, tx_status_url},
            Txid,
        },
        cli::config::DEFAULT_ELECTRUM_HTTP_URL,
    };
    use reqwest::Url;

    #[test]
    fn create_tx_status_url_from_default_base_url_success() {
        let txid: Txid = Txid::default();
        let base_url = Url::parse(DEFAULT_ELECTRUM_HTTP_URL).expect("Could not parse url");
        let url = tx_status_url(txid, &base_url).expect("Could not create url");
        let expected = format!("https://blockstream.info/testnet/api/tx/{}/status", txid);
        assert_eq!(url.as_str(), expected);
    }

    #[test]
    fn create_block_tip_height_url_from_default_base_url_success() {
        let base_url = Url::parse(DEFAULT_ELECTRUM_HTTP_URL).expect("Could not parse url");
        let url = blocks_tip_height_url(&base_url).expect("Could not create url");
        let expected = "https://blockstream.info/testnet/api/blocks/tip/height";
        assert_eq!(url.as_str(), expected);
    }
}
