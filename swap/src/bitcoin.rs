use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use backoff::{backoff::Constant as ConstantBackoff, future::FutureOperation as _};
use bitcoin::{util::psbt::PartiallySignedTransaction, Address, Transaction};
use bitcoin_harness::bitcoind_rpc::PsbtBase64;
use reqwest::Url;
use tokio::time;
use xmr_btc::bitcoin::{
    BlockHeight, BroadcastSignedTransaction, BuildTxLockPsbt, SignTxLock, TransactionBlockHeight,
    WatchForRawTransaction,
};

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
        let txid = self.0.send_raw_transaction(transaction).await?;

        // TODO: Instead of guessing how long it will take for the transaction to be
        // mined we should ask bitcoind for the number of confirmations on `txid`

        // give time for transaction to be mined
        time::delay_for(Duration::from_millis(1100)).await;

        Ok(txid)
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
impl GetRawTransaction for Wallet {
    async fn get_raw_transaction(&self, _txid: Txid) -> Option<Transaction> {
        todo!()
    }
}

#[async_trait]
impl WatchForTransactionFinality for Wallet {
    async fn watch_for_transaction_finality(&self, _txid: Txid) {
        todo!()
    }
}
