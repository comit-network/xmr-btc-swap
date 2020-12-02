//! An incomplete async bitcoind rpc client that supports multi-wallet features

use crate::bitcoind_rpc_api::{BitcoindRpcApi, PsbtBase64, WalletProcessPsbtResponse};
use ::bitcoin::{hashes::hex::FromHex, Address, Amount, Network, Transaction, Txid};
use bitcoincore_rpc_json::{FinalizePsbtResult, GetAddressInfoResult};
use jsonrpc_client::{JsonRpcError, ResponsePayload, SendRequest};
use reqwest::Url;
use serde::{de::DeserializeOwned, Deserialize};
use std::collections::HashMap;

pub type Result<T> = std::result::Result<T, Error>;

pub const JSONRPC_VERSION: &str = "1.0";

#[jsonrpc_client::implement(BitcoindRpcApi)]
#[derive(Debug, Clone)]
pub struct Client {
    inner: reqwest::Client,
    base_url: reqwest::Url,
}

impl Client {
    pub fn new(url: Url) -> Self {
        Client {
            inner: reqwest::Client::new(),
            base_url: url,
        }
    }

    pub fn with_wallet(&self, wallet_name: &str) -> Result<Self> {
        Ok(Self {
            base_url: self
                .base_url
                .join(format!("/wallet/{}", wallet_name).as_str())?,
            ..self.clone()
        })
    }

    pub async fn network(&self) -> Result<Network> {
        let blockchain_info = self.getblockchaininfo().await?;

        let network = match blockchain_info.chain.as_str() {
            "main" => Network::Bitcoin,
            "test" => Network::Testnet,
            "regtest" => Network::Regtest,
            _ => return Err(Error::UnexpectedResponse),
        };

        Ok(network)
    }

    pub async fn median_time(&self) -> Result<u64> {
        let blockchain_info = self.getblockchaininfo().await?;

        Ok(blockchain_info.median_time)
    }

    pub async fn set_hd_seed(
        &self,
        wallet_name: &str,
        new_key_pool: Option<bool>,
        wif_private_key: Option<String>,
    ) -> Result<()> {
        self.with_wallet(wallet_name)?
            .sethdseed(new_key_pool, wif_private_key)
            .await?;

        Ok(())
    }

    pub async fn send_to_address(
        &self,
        wallet_name: &str,
        address: Address,
        amount: Amount,
    ) -> Result<Txid> {
        let txid = self
            .with_wallet(wallet_name)?
            .sendtoaddress(address, amount.as_btc())
            .await?;
        let txid = Txid::from_hex(&txid)?;

        Ok(txid)
    }

    pub async fn get_raw_transaction(&self, txid: Txid) -> Result<Transaction> {
        let hex: String = self.get_raw_transaction_rpc(txid, false).await?;
        let bytes: Vec<u8> = FromHex::from_hex(&hex)?;
        let transaction = bitcoin::consensus::encode::deserialize(&bytes)?;

        Ok(transaction)
    }

    pub async fn get_raw_transaction_verbose(
        &self,
        txid: Txid,
    ) -> Result<GetRawTransactionVerboseResponse> {
        let res = self.get_raw_transaction_rpc(txid, true).await?;

        Ok(res)
    }

    async fn get_raw_transaction_rpc<R>(&self, txid: Txid, is_verbose: bool) -> Result<R>
    where
        R: std::fmt::Debug + DeserializeOwned,
    {
        let body = jsonrpc_client::Request::new_v2("getrawtransaction")
            .with_argument(txid)?
            .with_argument(is_verbose)?
            .serialize()?;

        let payload: ResponsePayload<R> = self
            .inner
            .send_request::<R>(self.base_url.clone(), body)
            .await
            .map_err(::jsonrpc_client::Error::Client)?
            .payload;
        let response: std::result::Result<R, JsonRpcError> = payload.into();

        Ok(response.map_err(::jsonrpc_client::Error::JsonRpc)?)
    }

    pub async fn fund_psbt(
        &self,
        wallet_name: &str,
        inputs: &[bitcoincore_rpc_json::CreateRawTransactionInput],
        address: Address,
        amount: Amount,
    ) -> Result<String> {
        let mut outputs_converted = HashMap::new();
        outputs_converted.insert(address.to_string(), amount.as_btc());
        let psbt = self
            .with_wallet(wallet_name)?
            .walletcreatefundedpsbt(inputs, outputs_converted)
            .await?;
        Ok(psbt.psbt)
    }

    pub async fn join_psbts(&self, wallet_name: &str, psbts: &[String]) -> Result<PsbtBase64> {
        let psbt = self.with_wallet(wallet_name)?.joinpsbts(psbts).await?;
        Ok(psbt)
    }
    pub async fn wallet_process_psbt(
        &self,
        wallet_name: &str,
        psbt: PsbtBase64,
    ) -> Result<WalletProcessPsbtResponse> {
        let psbt = self
            .with_wallet(wallet_name)?
            .walletprocesspsbt(psbt)
            .await?;
        Ok(psbt)
    }

    pub async fn finalize_psbt(
        &self,
        wallet_name: &str,
        psbt: PsbtBase64,
    ) -> Result<FinalizePsbtResult> {
        let psbt = self.with_wallet(wallet_name)?.finalizepsbt(psbt).await?;
        Ok(psbt)
    }

    pub async fn address_info(
        &self,
        wallet_name: &str,
        address: &Address,
    ) -> Result<GetAddressInfoResult> {
        let address_info = self
            .with_wallet(wallet_name)?
            .getaddressinfo(address)
            .await?;
        Ok(address_info)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("JSON Rpc Client: ")]
    JsonRpcClient(#[from] jsonrpc_client::Error<reqwest::Error>),
    #[error("Serde JSON: ")]
    SerdeJson(#[from] serde_json::Error),
    #[error("Parse amount: ")]
    ParseAmount(#[from] bitcoin::util::amount::ParseAmountError),
    #[error("Hex decode: ")]
    Hex(#[from] bitcoin::hashes::hex::Error),
    #[error("Bitcoin decode: ")]
    BitcoinDecode(#[from] bitcoin::consensus::encode::Error),
    // TODO: add more info to error
    #[error("Unexpected response: ")]
    UnexpectedResponse,
    #[error("Parse url: ")]
    ParseUrl(#[from] url::ParseError),
}

#[derive(Debug, Deserialize)]
struct BlockchainInfo {
    chain: Network,
    mediantime: u32,
}

/// Response to the RPC command `getrawtransaction`, when the second
/// argument is set to `true`.
///
/// It only defines one field, but can be expanded to include all the
/// fields returned by `bitcoind` (see:
/// https://bitcoincore.org/en/doc/0.19.0/rpc/rawtransactions/getrawtransaction/)
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct GetRawTransactionVerboseResponse {
    #[serde(rename = "blockhash")]
    pub block_hash: Option<bitcoin::BlockHash>,
}

/// Response to the RPC command `getblock`.
///
/// It only defines one field, but can be expanded to include all the
/// fields returned by `bitcoind` (see:
/// https://bitcoincore.org/en/doc/0.19.0/rpc/blockchain/getblock/)
#[derive(Copy, Clone, Debug, Deserialize)]
pub struct GetBlockResponse {
    pub height: u32,
}

#[cfg(all(test, feature = "test-docker"))]
mod test {
    use super::*;
    use crate::Bitcoind;
    use testcontainers::clients;

    #[tokio::test]
    async fn get_network_info() {
        let tc_client = clients::Cli::default();
        let (client, _container) = {
            let blockchain = Bitcoind::new(&tc_client, "0.19.1").unwrap();

            (Client::new(blockchain.node_url.clone()), blockchain)
        };

        let network = client.network().await.unwrap();

        assert_eq!(network, Network::Regtest)
    }

    #[tokio::test]
    async fn get_median_time() {
        let tc_client = clients::Cli::default();
        let (client, _container) = {
            let blockchain = Bitcoind::new(&tc_client, "0.19.1").unwrap();

            (Client::new(blockchain.node_url.clone()), blockchain)
        };

        let _mediant_time = client.median_time().await.unwrap();
    }
}
