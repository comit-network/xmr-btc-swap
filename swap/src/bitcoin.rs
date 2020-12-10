use anyhow::{Context, Result};
use async_trait::async_trait;
use backoff::{backoff::Constant as ConstantBackoff, future::FutureOperation as _};
use bitcoin::util::psbt::PartiallySignedTransaction;
use bitcoin_harness::{bitcoind_rpc::PsbtBase64, BitcoindRpcApi};
use reqwest::Url;
use std::time::Duration;
use tokio::time::interval;
use xmr_btc::{
    bitcoin::{
        BlockHeight, BroadcastSignedTransaction, BuildTxLockPsbt, SignTxLock,
        TransactionBlockHeight, WatchForRawTransaction,
    },
    config::Config,
};

pub use ::bitcoin::{Address, Transaction};
pub use xmr_btc::bitcoin::*;

pub const TX_LOCK_MINE_TIMEOUT: u64 = 3600;

#[derive(Debug)]
pub struct Wallet {
    pub inner: bitcoin_harness::Wallet,
    pub network: bitcoin::Network,
}

impl Wallet {
    pub async fn new(name: &str, url: Url, network: bitcoin::Network) -> Result<Self> {
        let wallet = bitcoin_harness::Wallet::new(name, url).await?;

        Ok(Self {
            inner: wallet,
            network,
        })
    }

    pub async fn balance(&self) -> Result<Amount> {
        let balance = self.inner.balance().await?;
        Ok(balance)
    }

    pub async fn new_address(&self) -> Result<Address> {
        self.inner.new_address().await.map_err(Into::into)
    }

    pub async fn transaction_fee(&self, txid: Txid) -> Result<Amount> {
        let fee = self
            .inner
            .get_wallet_transaction(txid)
            .await
            .map(|res| {
                res.fee.map(|signed_amount| {
                    signed_amount
                        .abs()
                        .to_unsigned()
                        .expect("Absolute value is always positive")
                })
            })?
            .context("Rpc response did not contain a fee")?;

        Ok(fee)
    }
}

#[async_trait]
impl BuildTxLockPsbt for Wallet {
    async fn build_tx_lock_psbt(
        &self,
        output_address: Address,
        output_amount: Amount,
    ) -> Result<PartiallySignedTransaction> {
        let psbt = self.inner.fund_psbt(output_address, output_amount).await?;
        let as_hex = base64::decode(psbt)?;

        let psbt = bitcoin::consensus::deserialize(&as_hex)?;

        Ok(psbt)
    }
}

#[async_trait]
impl SignTxLock for Wallet {
    async fn sign_tx_lock(&self, tx_lock: TxLock) -> Result<Transaction> {
        let psbt = PartiallySignedTransaction::from(tx_lock);

        let psbt = bitcoin::consensus::serialize(&psbt);
        let as_base64 = base64::encode(psbt);

        let psbt = self
            .inner
            .wallet_process_psbt(PsbtBase64(as_base64))
            .await?;
        let PsbtBase64(signed_psbt) = PsbtBase64::from(psbt);

        let as_hex = base64::decode(signed_psbt)?;
        let psbt: PartiallySignedTransaction = bitcoin::consensus::deserialize(&as_hex)?;

        let tx = psbt.extract_tx();

        Ok(tx)
    }
}

#[async_trait]
impl BroadcastSignedTransaction for Wallet {
    async fn broadcast_signed_transaction(&self, transaction: Transaction) -> Result<Txid> {
        let txid = self.inner.send_raw_transaction(transaction).await?;
        tracing::debug!("Bitcoin tx broadcasted! TXID = {}", txid);
        Ok(txid)
    }
}

// TODO: For retry, use `backoff::ExponentialBackoff` in production as opposed
// to `ConstantBackoff`.
#[async_trait]
impl WatchForRawTransaction for Wallet {
    async fn watch_for_raw_transaction(&self, txid: Txid) -> Transaction {
        (|| async { Ok(self.inner.get_raw_transaction(txid).await?) })
            .retry(ConstantBackoff::new(Duration::from_secs(1)))
            .await
            .expect("transient errors to be retried")
    }
}

#[async_trait]
impl GetRawTransaction for Wallet {
    // todo: potentially replace with option
    async fn get_raw_transaction(&self, txid: Txid) -> Result<Transaction> {
        Ok(self.inner.get_raw_transaction(txid).await?)
    }
}

#[async_trait]
impl BlockHeight for Wallet {
    async fn block_height(&self) -> u32 {
        (|| async { Ok(self.inner.client.getblockcount().await?) })
            .retry(ConstantBackoff::new(Duration::from_secs(1)))
            .await
            .expect("transient errors to be retried")
    }
}

#[async_trait]
impl TransactionBlockHeight for Wallet {
    async fn transaction_block_height(&self, txid: Txid) -> u32 {
        #[derive(Debug)]
        enum Error {
            Io,
            NotYetMined,
        }

        (|| async {
            let block_height = self
                .inner
                .transaction_block_height(txid)
                .await
                .map_err(|_| backoff::Error::Transient(Error::Io))?;

            let block_height =
                block_height.ok_or_else(|| backoff::Error::Transient(Error::NotYetMined))?;

            Result::<_, backoff::Error<Error>>::Ok(block_height)
        })
        .retry(ConstantBackoff::new(Duration::from_secs(1)))
        .await
        .expect("transient errors to be retried")
    }
}

#[async_trait]
impl WaitForTransactionFinality for Wallet {
    async fn wait_for_transaction_finality(&self, txid: Txid, config: Config) -> Result<()> {
        // TODO(Franck): This assumes that bitcoind runs with txindex=1

        // Divide by 4 to not check too often yet still be aware of the new block early
        // on.
        let mut interval = interval(config.bitcoin_avg_block_time / 4);

        loop {
            let tx = self.inner.client.get_raw_transaction_verbose(txid).await?;
            if let Some(confirmations) = tx.confirmations {
                if confirmations >= config.bitcoin_finality_confirmations {
                    break;
                }
            }
            interval.tick().await;
        }

        Ok(())
    }
}

impl Network for Wallet {
    fn get_network(&self) -> bitcoin::Network {
        self.network
    }
}
