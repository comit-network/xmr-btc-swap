use crate::{
    bitcoind_rpc::{Client, Result},
    bitcoind_rpc_api::{Account, BitcoindRpcApi, PsbtBase64, WalletProcessPsbtResponse},
};
use bitcoin::{hashes::hex::FromHex, Address, Amount, Transaction, Txid};
use bitcoincore_rpc_json::{
    FinalizePsbtResult, GetAddressInfoResult, GetTransactionResult, GetWalletInfoResult,
    ListUnspentResultEntry,
};
use std::convert::TryFrom;
use url::Url;

/// A wrapper to bitcoind wallet
#[derive(Debug)]
pub struct Wallet {
    name: String,
    bitcoind_client: Client,
}

impl Wallet {
    /// Create a wallet on the bitcoind instance or use the wallet with the same
    /// name if it exists.
    pub async fn new(name: &str, url: Url) -> Result<Self> {
        let bitcoind_client = Client::new(url);

        let wallet = Self {
            name: name.to_string(),
            bitcoind_client,
        };

        wallet.init().await?;

        Ok(wallet)
    }

    async fn init(&self) -> Result<()> {
        match self.info().await {
            Err(_) => {
                self.bitcoind_client
                    .createwallet(&self.name, None, None, None, None)
                    .await?;
                Ok(())
            }
            Ok(_) => Ok(()),
        }
    }

    pub async fn info(&self) -> Result<GetWalletInfoResult> {
        Ok(self
            .bitcoind_client
            .with_wallet(&self.name)?
            .getwalletinfo()
            .await?)
    }

    pub async fn median_time(&self) -> Result<u64> {
        Ok(self.bitcoind_client.median_time().await?)
    }

    pub async fn block_height(&self) -> Result<u32> {
        Ok(self.bitcoind_client.getblockcount().await?)
    }

    pub async fn new_address(&self) -> Result<Address> {
        Ok(self
            .bitcoind_client
            .with_wallet(&self.name)?
            .getnewaddress(None, Some("bech32".into()))
            .await?)
    }

    pub async fn balance(&self) -> Result<Amount> {
        let response = self
            .bitcoind_client
            .with_wallet(&self.name)?
            .getbalance(Account, None, None, None)
            .await?;
        let amount = Amount::from_btc(response)?;
        Ok(amount)
    }

    pub async fn send_to_address(&self, address: Address, amount: Amount) -> Result<Txid> {
        let txid = self
            .bitcoind_client
            .with_wallet(&self.name)?
            .sendtoaddress(address, amount.as_btc())
            .await?;
        let txid = Txid::from_hex(&txid)?;

        Ok(txid)
    }

    pub async fn send_raw_transaction(&self, transaction: Transaction) -> Result<Txid> {
        let txid = self
            .bitcoind_client
            .with_wallet(&self.name)?
            .sendrawtransaction(transaction.into())
            .await?;
        let txid = Txid::from_hex(&txid)?;
        Ok(txid)
    }

    pub async fn get_raw_transaction(&self, txid: Txid) -> Result<Transaction> {
        self.bitcoind_client.get_raw_transaction(txid).await
    }

    pub async fn get_wallet_transaction(&self, txid: Txid) -> Result<GetTransactionResult> {
        let res = self
            .bitcoind_client
            .with_wallet(&self.name)?
            .gettransaction(txid)
            .await?;

        Ok(res)
    }

    pub async fn address_info(&self, address: &Address) -> Result<GetAddressInfoResult> {
        self.bitcoind_client.address_info(&self.name, address).await
    }

    pub async fn list_unspent(&self) -> Result<Vec<ListUnspentResultEntry>> {
        let unspents = self
            .bitcoind_client
            .with_wallet(&self.name)?
            .listunspent(None, None, None, None)
            .await?;
        Ok(unspents)
    }

    pub async fn fund_psbt(&self, address: Address, amount: Amount) -> Result<String> {
        self.bitcoind_client
            .fund_psbt(&self.name, &[], address, amount)
            .await
    }

    pub async fn join_psbts(&self, psbts: &[String]) -> Result<PsbtBase64> {
        self.bitcoind_client.join_psbts(&self.name, psbts).await
    }

    pub async fn wallet_process_psbt(&self, psbt: PsbtBase64) -> Result<WalletProcessPsbtResponse> {
        self.bitcoind_client
            .wallet_process_psbt(&self.name, psbt)
            .await
    }

    pub async fn finalize_psbt(&self, psbt: PsbtBase64) -> Result<FinalizePsbtResult> {
        self.bitcoind_client.finalize_psbt(&self.name, psbt).await
    }

    pub async fn transaction_block_height(&self, txid: Txid) -> Result<Option<u32>> {
        let res = self
            .bitcoind_client
            .get_raw_transaction_verbose(txid)
            .await?;

        let block_hash = match res.block_hash {
            Some(block_hash) => block_hash,
            None => return Ok(None),
        };

        let res = self.bitcoind_client.getblock(&block_hash).await?;

        // TODO: This was changed to u32 because down the road we needed it as u32 (and
        // the height should be sufficient as u32)
        Ok(Some(
            u32::try_from(res.height).expect("can cast block-height to u32"),
        ))
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use crate::{Bitcoind, Wallet};
    use bitcoin::{util::psbt::PartiallySignedTransaction, Amount, Transaction, TxOut};
    use tokio::time::delay_for;

    #[tokio::test]
    async fn get_wallet_transaction() {
        let tc_client = testcontainers::clients::Cli::default();
        let bitcoind = Bitcoind::new(&tc_client, "0.19.1").unwrap();
        bitcoind.init(5).await.unwrap();

        let wallet = Wallet::new("wallet", bitcoind.node_url.clone())
            .await
            .unwrap();
        let mint_address = wallet.new_address().await.unwrap();
        let mint_amount = bitcoin::Amount::from_btc(3.0).unwrap();
        bitcoind.mint(mint_address, mint_amount).await.unwrap();

        let pay_address = wallet.new_address().await.unwrap();
        let pay_amount = bitcoin::Amount::from_btc(1.0).unwrap();
        let txid = wallet
            .send_to_address(pay_address, pay_amount)
            .await
            .unwrap();

        let _res = wallet.get_wallet_transaction(txid).await.unwrap();
    }

    #[tokio::test]
    async fn two_party_psbt_test() {
        let tc_client = testcontainers::clients::Cli::default();
        let bitcoind = Bitcoind::new(&tc_client, "0.19.1").unwrap();
        bitcoind.init(5).await.unwrap();

        let alice = Wallet::new("alice", bitcoind.node_url.clone())
            .await
            .unwrap();
        let address = alice.new_address().await.unwrap();
        let amount = bitcoin::Amount::from_btc(3.0).unwrap();
        bitcoind.mint(address, amount).await.unwrap();
        let joined_address = alice.new_address().await.unwrap();
        let alice_result = alice
            .fund_psbt(joined_address.clone(), Amount::from_btc(1.0).unwrap())
            .await
            .unwrap();

        let bob = Wallet::new("bob", bitcoind.node_url.clone()).await.unwrap();
        let address = bob.new_address().await.unwrap();
        let amount = bitcoin::Amount::from_btc(3.0).unwrap();
        bitcoind.mint(address, amount).await.unwrap();
        let bob_psbt = bob
            .fund_psbt(joined_address.clone(), Amount::from_btc(1.0).unwrap())
            .await
            .unwrap();

        let joined_psbts = alice
            .join_psbts(&[alice_result.clone(), bob_psbt.clone()])
            .await
            .unwrap();

        let partial_signed_bitcoin_transaction: PartiallySignedTransaction = {
            let as_hex = base64::decode(joined_psbts.0).unwrap();
            bitcoin::consensus::deserialize(&as_hex).unwrap()
        };

        let transaction = partial_signed_bitcoin_transaction.extract_tx();
        let mut outputs = vec![];

        transaction.output.iter().for_each(|output| {
            // filter out shared output
            if output.script_pubkey != joined_address.clone().script_pubkey() {
                outputs.push(output.clone());
            }
        });
        // add shared output with twice the btc to fit change addresses
        outputs.push(TxOut {
            value: Amount::from_btc(2.0).unwrap().as_sat(),
            script_pubkey: joined_address.clone().script_pubkey(),
        });

        let transaction = Transaction {
            output: outputs,
            ..transaction
        };

        assert_eq!(
            transaction.input.len(),
            2,
            "We expect 2 inputs, one from alice, one from bob"
        );
        assert_eq!(
            transaction.output.len(),
            3,
            "We expect 3 outputs, change for alice, change for bob and shared address"
        );

        let psbt = {
            let partial_signed_bitcoin_transaction =
                PartiallySignedTransaction::from_unsigned_tx(transaction).unwrap();
            let hex_vec = bitcoin::consensus::serialize(&partial_signed_bitcoin_transaction);
            base64::encode(hex_vec).into()
        };

        let alice_signed_psbt = alice.wallet_process_psbt(psbt).await.unwrap();
        let bob_signed_psbt = bob
            .wallet_process_psbt(alice_signed_psbt.into())
            .await
            .unwrap();

        let alice_finalized_psbt = alice.finalize_psbt(bob_signed_psbt.into()).await.unwrap();

        let transaction = alice_finalized_psbt.transaction().unwrap().unwrap();
        let txid = alice.send_raw_transaction(transaction).await.unwrap();
        println!("Final tx_id: {:?}", txid);
    }

    #[tokio::test]
    async fn block_height() {
        let tc_client = testcontainers::clients::Cli::default();
        let bitcoind = Bitcoind::new(&tc_client, "0.19.1").unwrap();
        bitcoind.init(5).await.unwrap();

        let wallet = Wallet::new("wallet", bitcoind.node_url.clone())
            .await
            .unwrap();

        let height_0 = wallet.block_height().await.unwrap();
        delay_for(Duration::from_secs(2)).await;

        let height_1 = wallet.block_height().await.unwrap();

        assert!(height_1 > height_0)
    }

    #[tokio::test]
    async fn transaction_block_height() {
        let tc_client = testcontainers::clients::Cli::default();
        let bitcoind = Bitcoind::new(&tc_client, "0.19.1").unwrap();
        bitcoind.init(5).await.unwrap();

        let wallet = Wallet::new("wallet", bitcoind.node_url.clone())
            .await
            .unwrap();
        let mint_address = wallet.new_address().await.unwrap();
        let mint_amount = bitcoin::Amount::from_btc(3.0).unwrap();
        bitcoind.mint(mint_address, mint_amount).await.unwrap();

        let pay_address = wallet.new_address().await.unwrap();
        let pay_amount = bitcoin::Amount::from_btc(1.0).unwrap();
        let txid = wallet
            .send_to_address(pay_address, pay_amount)
            .await
            .unwrap();

        // wait for the transaction to be included in a block, so that
        // it has a block height field assigned to it when calling
        // `getrawtransaction`
        delay_for(Duration::from_secs(2)).await;

        let _res = wallet.transaction_block_height(txid).await.unwrap();
    }
}
