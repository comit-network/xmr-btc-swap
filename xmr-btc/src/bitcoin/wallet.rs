use crate::bitcoin::{
    BroadcastSignedTransaction, BuildTxLockPsbt, GetRawTransaction, SignTxLock, TxLock,
};
use anyhow::Result;
use async_trait::async_trait;
use bitcoin::{util::psbt::PartiallySignedTransaction, Address, Amount, Transaction, Txid};
use bitcoin_harness::{bitcoind_rpc::PsbtBase64, Bitcoind};
use reqwest::Url;
use std::time::Duration;
use tokio::time;

#[derive(Debug)]
pub struct Wallet(pub bitcoin_harness::Wallet);

impl Wallet {
    pub async fn new(name: &str, url: &Url) -> Result<Self> {
        let wallet = bitcoin_harness::Wallet::new(name, url.clone()).await?;

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

pub async fn make_wallet(
    name: &str,
    bitcoind: &Bitcoind<'_>,
    fund_amount: Amount,
) -> Result<Wallet> {
    let wallet = Wallet::new(name, &bitcoind.node_url).await?;
    let buffer = Amount::from_btc(1.0).unwrap();
    let amount = fund_amount + buffer;

    let address = wallet.0.new_address().await.unwrap();

    bitcoind.mint(address, amount).await.unwrap();

    Ok(wallet)
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

#[async_trait]
impl GetRawTransaction for Wallet {
    async fn get_raw_transaction(&self, txid: Txid) -> Result<Transaction> {
        let tx = self.0.get_raw_transaction(txid).await?;

        Ok(tx)
    }
}
