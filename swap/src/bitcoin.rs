use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use backoff::{backoff::Constant as ConstantBackoff, future::FutureOperation as _};
use bitcoin::util::psbt::PartiallySignedTransaction;
use bitcoin_harness::bitcoind_rpc::PsbtBase64;
use reqwest::Url;
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
pub struct Wallet(pub bitcoin_harness::Wallet);

impl Wallet {
    pub async fn new(name: &str, url: Url) -> Result<Self> {
        let wallet = bitcoin_harness::Wallet::new(name, url).await?;

        Ok(Self(wallet))
    }

    pub async fn balance(&self) -> Result<Amount> {
        let balance = self.0.balance().await?;
        Ok(balance)
    }

    pub async fn new_address(&self) -> Result<Address> {
        self.0.new_address().await.map_err(Into::into)
    }

    pub async fn transaction_fee(&self, txid: Txid) -> Result<Amount> {
        let fee = self
            .0
            .get_wallet_transaction(txid)
            .await
            .map(|res| bitcoin::Amount::from_btc(-res.fee))??;

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
        let psbt = self.0.fund_psbt(output_address, output_amount).await?;
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

        let psbt = self.0.wallet_process_psbt(PsbtBase64(as_base64)).await?;
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
        Ok(self.0.send_raw_transaction(transaction).await?)
    }
}

// TODO: For retry, use `backoff::ExponentialBackoff` in production as opposed
// to `ConstantBackoff`.
#[async_trait]
impl WatchForRawTransaction for Wallet {
    async fn watch_for_raw_transaction(&self, txid: Txid) -> Transaction {
        (|| async { Ok(self.0.get_raw_transaction(txid).await?) })
            .retry(ConstantBackoff::new(Duration::from_secs(1)))
            .await
            .expect("transient errors to be retried")
    }
}

#[async_trait]
impl GetRawTransaction for Wallet {
    // todo: potentially replace with option
    async fn get_raw_transaction(&self, txid: Txid) -> Result<Transaction> {
        Ok(self.0.get_raw_transaction(txid).await?)
    }
}

#[async_trait]
impl BlockHeight for Wallet {
    async fn block_height(&self) -> u32 {
        (|| async { Ok(self.0.block_height().await?) })
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
                .0
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
            let tx = self.0.client.get_raw_transaction_verbose(txid).await?;
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
