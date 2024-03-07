use std::fmt;

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize};

#[jsonrpc_client::api(version = "2.0")]
pub trait MoneroWalletRpc {
    async fn get_address(&self, account_index: u32) -> GetAddress;
    async fn get_balance(&self, account_index: u32) -> GetBalance;
    async fn create_account(&self, label: String) -> CreateAccount;
    async fn get_accounts(&self, tag: String) -> GetAccounts;
    async fn open_wallet(&self, filename: String) -> WalletOpened;
    async fn close_wallet(&self) -> WalletClosed;
    async fn create_wallet(&self, filename: String, language: String) -> WalletCreated;
    async fn transfer(
        &self,
        account_index: u32,
        destinations: Vec<Destination>,
        get_tx_key: bool,
    ) -> Transfer;
    async fn get_height(&self) -> BlockHeight;
    async fn check_tx_key(&self, txid: String, tx_key: String, address: String) -> CheckTxKey;
    #[allow(clippy::too_many_arguments)]
    async fn generate_from_keys(
        &self,
        filename: String,
        address: String,
        spendkey: String,
        viewkey: String,
        restore_height: u32,
        password: String,
        autosave_current: bool,
    ) -> GenerateFromKeys;
    async fn refresh(&self) -> Refreshed;
    async fn sweep_all(&self, address: String) -> SweepAll;
    async fn get_version(&self) -> Version;
    async fn store(&self);
}

#[jsonrpc_client::implement(MoneroWalletRpc)]
#[derive(Debug, Clone)]
pub struct Client {
    inner: reqwest::Client,
    base_url: reqwest::Url,
}

impl Client {
    /// Constructs a monero-wallet-rpc client with localhost endpoint.
    pub fn localhost(port: u16) -> Result<Self> {
        Client::new(
            format!("http://127.0.0.1:{}/json_rpc", port)
                .parse()
                .context("url is well formed")?,
        )
    }

    /// Constructs a monero-wallet-rpc client with `url` endpoint.
    pub fn new(url: reqwest::Url) -> Result<Self> {
        Ok(Self {
            inner: reqwest::ClientBuilder::new()
                .connection_verbose(true)
                .build()?,
            base_url: url,
        })
    }

    /// Transfers `amount` monero from `account_index` to `address`.
    pub async fn transfer_single(
        &self,
        account_index: u32,
        amount: u64,
        address: &str,
    ) -> Result<Transfer> {
        let dest = vec![Destination {
            amount,
            address: address.to_owned(),
        }];

        Ok(self.transfer(account_index, dest, true).await?)
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct GetAddress {
    pub address: String,
}

#[derive(Deserialize, Debug, Clone, Copy)]
pub struct GetBalance {
    pub balance: u64,
    pub unlocked_balance: u64,
    pub multisig_import_needed: bool,
    pub blocks_to_unlock: u32,
    pub time_to_unlock: u32,
}

impl fmt::Display for GetBalance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut total = Decimal::from(self.balance);
        total
            .set_scale(12)
            .expect("12 is smaller than max precision of 28");

        let mut unlocked = Decimal::from(self.unlocked_balance);
        unlocked
            .set_scale(12)
            .expect("12 is smaller than max precision of 28");

        write!(
            f,
            "total balance: {}, unlocked balance: {}",
            total, unlocked
        )
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct CreateAccount {
    pub account_index: u32,
    pub address: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct GetAccounts {
    pub subaddress_accounts: Vec<SubAddressAccount>,
    pub total_balance: u64,
    pub total_unlocked_balance: u64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SubAddressAccount {
    pub account_index: u32,
    pub balance: u32,
    pub base_address: String,
    pub label: String,
    pub tag: String,
    pub unlocked_balance: u64,
}

#[derive(Serialize, Debug, Clone)]
pub struct Destination {
    pub amount: u64,
    pub address: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Transfer {
    pub amount: u64,
    pub fee: u64,
    pub multisig_txset: String,
    pub tx_blob: String,
    pub tx_hash: String,
    #[serde(deserialize_with = "opt_key_from_blank")]
    pub tx_key: Option<monero::PrivateKey>,
    pub tx_metadata: String,
    pub unsigned_txset: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct BlockHeight {
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(from = "CheckTxKeyResponse")]
pub struct CheckTxKey {
    pub confirmations: u64,
    pub received: u64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct CheckTxKeyResponse {
    pub confirmations: u64,
    pub received: u64,
}

impl From<CheckTxKeyResponse> for CheckTxKey {
    fn from(response: CheckTxKeyResponse) -> Self {
        // Due to a bug in monerod that causes check_tx_key confirmations
        // to overflow we safeguard the confirmations to avoid unwanted
        // side effects.
        let confirmations = if response.confirmations > u64::MAX - 1000 {
            0
        } else {
            response.confirmations
        };

        CheckTxKey {
            confirmations,
            received: response.received,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct GenerateFromKeys {
    pub address: String,
    pub info: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Refreshed {
    pub blocks_fetched: u32,
    pub received_money: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SweepAll {
    pub tx_hash_list: Vec<String>,
}

#[derive(Debug, Copy, Clone, Deserialize)]
pub struct Version {
    pub version: u32,
}

pub type WalletCreated = Empty;
pub type WalletClosed = Empty;
pub type WalletOpened = Empty;

/// Zero-sized struct to allow serde to deserialize an empty JSON object.
///
/// With `serde`, an empty JSON object (`{ }`) does not deserialize into Rust's
/// `()`. With the adoption of `jsonrpc_client`, we need to be explicit about
/// what the response of every RPC call is. Unfortunately, monerod likes to
/// return empty objects instead of `null`s in certain cases. We use this struct
/// to all the "deserialization" to happily continue.
#[derive(Debug, Copy, Clone, Deserialize)]
pub struct Empty {}

fn opt_key_from_blank<'de, D>(deserializer: D) -> Result<Option<monero::PrivateKey>, D::Error>
where
    D: Deserializer<'de>,
{
    let string = String::deserialize(deserializer)?;

    if string.is_empty() {
        return Ok(None);
    }

    Ok(Some(string.parse().map_err(D::Error::custom)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonrpc_client::Response;

    #[test]
    fn can_deserialize_sweep_all_response() {
        let response = r#"{
          "id": "0",
          "jsonrpc": "2.0",
          "result": {
            "amount_list": [29921410000],
            "fee_list": [78590000],
            "multisig_txset": "",
            "tx_hash_list": ["c1d8cfa87d445c1915a59d67be3e93ba8a29018640cf69b465f07b1840a8f8c8"],
            "unsigned_txset": "",
            "weight_list": [1448]
          }
        }"#;

        let _: Response<SweepAll> = serde_json::from_str(response).unwrap();
    }

    #[test]
    fn can_deserialize_create_wallet() {
        let response = r#"{
          "id": 0,
          "jsonrpc": "2.0",
          "result": {
          }
        }"#;

        let _: Response<WalletCreated> = serde_json::from_str(response).unwrap();
    }
}
