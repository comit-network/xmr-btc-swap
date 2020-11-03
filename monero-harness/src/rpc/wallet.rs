use crate::rpc::{Request, Response};

use anyhow::Result;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// JSON RPC client for monero-wallet-rpc.
#[derive(Debug)]
pub struct Client {
    pub inner: reqwest::Client,
    pub url: Url,
}

impl Client {
    /// Constructs a monero-wallet-rpc client with localhost endpoint.
    pub fn localhost(port: u16) -> Self {
        let url = format!("http://127.0.0.1:{}/json_rpc", port);
        let url = Url::parse(&url).expect("url is well formed");

        Client::new(url)
    }

    /// Constructs a monero-wallet-rpc client with `url` endpoint.
    pub fn new(url: Url) -> Self {
        Self {
            inner: reqwest::Client::new(),
            url,
        }
    }

    /// Get addresses for account by index.
    pub async fn get_address(&self, account_index: u32) -> Result<GetAddress> {
        let params = GetAddressParams { account_index };
        let request = Request::new("get_address", params);

        let response = self
            .inner
            .post(self.url.clone())
            .json(&request)
            .send()
            .await?
            .text()
            .await?;

        debug!("get address RPC response: {}", response);

        let r: Response<GetAddress> = serde_json::from_str(&response)?;
        Ok(r.result)
    }

    /// Gets the balance of account by index.
    pub async fn get_balance(&self, index: u32) -> Result<u64> {
        let params = GetBalanceParams {
            account_index: index,
        };
        let request = Request::new("get_balance", params);

        let response = self
            .inner
            .post(self.url.clone())
            .json(&request)
            .send()
            .await?
            .text()
            .await?;

        debug!(
            "get balance of account index {} RPC response: {}",
            index, response
        );

        let res: Response<GetBalance> = serde_json::from_str(&response)?;

        let balance = res.result.balance;

        Ok(balance)
    }

    pub async fn create_account(&self, label: &str) -> Result<CreateAccount> {
        let params = LabelParams {
            label: label.to_owned(),
        };
        let request = Request::new("create_account", params);

        let response = self
            .inner
            .post(self.url.clone())
            .json(&request)
            .send()
            .await?
            .text()
            .await?;

        debug!("create account RPC response: {}", response);

        let r: Response<CreateAccount> = serde_json::from_str(&response)?;
        Ok(r.result)
    }

    /// Get accounts, filtered by tag ("" for no filtering).
    pub async fn get_accounts(&self, tag: &str) -> Result<GetAccounts> {
        let params = TagParams {
            tag: tag.to_owned(),
        };
        let request = Request::new("get_accounts", params);

        let response = self
            .inner
            .post(self.url.clone())
            .json(&request)
            .send()
            .await?
            .text()
            .await?;

        debug!("get accounts RPC response: {}", response);

        let r: Response<GetAccounts> = serde_json::from_str(&response)?;

        Ok(r.result)
    }

    /// Creates a wallet using `filename`.
    pub async fn create_wallet(&self, filename: &str) -> Result<()> {
        let params = CreateWalletParams {
            filename: filename.to_owned(),
            language: "English".to_owned(),
        };
        let request = Request::new("create_wallet", params);

        let response = self
            .inner
            .post(self.url.clone())
            .json(&request)
            .send()
            .await?
            .text()
            .await?;

        debug!("create wallet RPC response: {}", response);

        Ok(())
    }

    /// Transfers `amount` moneroj from `account_index` to `address`.
    pub async fn transfer(
        &self,
        account_index: u32,
        amount: u64,
        address: &str,
    ) -> Result<Transfer> {
        let dest = vec![Destination {
            amount,
            address: address.to_owned(),
        }];
        self.multi_transfer(account_index, dest).await
    }

    /// Transfers moneroj from `account_index` to `destinations`.
    pub async fn multi_transfer(
        &self,
        account_index: u32,
        destinations: Vec<Destination>,
    ) -> Result<Transfer> {
        let params = TransferParams {
            account_index,
            destinations,
            get_tx_key: true,
        };
        let request = Request::new("transfer", params);

        let response = self
            .inner
            .post(self.url.clone())
            .json(&request)
            .send()
            .await?
            .text()
            .await?;

        debug!("transfer RPC response: {}", response);

        let r: Response<Transfer> = serde_json::from_str(&response)?;
        Ok(r.result)
    }

    /// Get wallet block height, this might be behind monerod height.
    pub(crate) async fn block_height(&self) -> Result<BlockHeight> {
        let request = Request::new("get_height", "");

        let response = self
            .inner
            .post(self.url.clone())
            .json(&request)
            .send()
            .await?
            .text()
            .await?;

        debug!("wallet height RPC response: {}", response);

        let r: Response<BlockHeight> = serde_json::from_str(&response)?;
        Ok(r.result)
    }

    /// Check a transaction in the blockchain with its secret key.
    pub async fn check_tx_key(
        &self,
        tx_id: &str,
        tx_key: &str,
        address: &str,
    ) -> Result<CheckTxKey> {
        let params = CheckTxKeyParams {
            tx_id: tx_id.to_owned(),
            tx_key: tx_key.to_owned(),
            address: address.to_owned(),
        };
        let request = Request::new("check_tx_key", params);

        let response = self
            .inner
            .post(self.url.clone())
            .json(&request)
            .send()
            .await?
            .text()
            .await?;

        debug!("transfer RPC response: {}", response);

        let r: Response<CheckTxKey> = serde_json::from_str(&response)?;
        Ok(r.result)
    }

    pub async fn generate_from_keys(
        &self,
        address: &str,
        spend_key: &str,
        view_key: &str,
    ) -> Result<GenerateFromKeys> {
        let params = GenerateFromKeysParams {
            restore_height: 0,
            filename: view_key.into(),
            address: address.into(),
            spendkey: spend_key.into(),
            viewkey: view_key.into(),
            password: "".into(),
            autosave_current: true,
        };
        let request = Request::new("generate_from_keys", params);

        let response = self
            .inner
            .post(self.url.clone())
            .json(&request)
            .send()
            .await?
            .text()
            .await?;

        debug!("generate_from_keys RPC response: {}", response);

        let r: Response<GenerateFromKeys> = serde_json::from_str(&response)?;
        Ok(r.result)
    }

    pub async fn refresh(&self) -> Result<Refreshed> {
        let request = Request::new("refresh", "");

        let response = self
            .inner
            .post(self.url.clone())
            .json(&request)
            .send()
            .await?
            .text()
            .await?;

        debug!("refresh RPC response: {}", response);

        let r: Response<Refreshed> = serde_json::from_str(&response)?;
        Ok(r.result)
    }
}

#[derive(Serialize, Debug, Clone)]
struct GetAddressParams {
    account_index: u32,
}

#[derive(Deserialize, Debug, Clone)]
pub struct GetAddress {
    pub address: String,
}

#[derive(Serialize, Debug, Clone)]
struct GetBalanceParams {
    account_index: u32,
}

#[derive(Deserialize, Debug, Clone)]
struct GetBalance {
    balance: u64,
    blocks_to_unlock: u32,
    multisig_import_needed: bool,
    time_to_unlock: u32,
    unlocked_balance: u64,
}

#[derive(Serialize, Debug, Clone)]
struct LabelParams {
    label: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CreateAccount {
    pub account_index: u32,
    pub address: String,
}

#[derive(Serialize, Debug, Clone)]
struct TagParams {
    tag: String,
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
struct CreateWalletParams {
    filename: String,
    language: String,
}

#[derive(Serialize, Debug, Clone)]
struct TransferParams {
    // Transfer from this account.
    account_index: u32,
    // Destinations to receive XMR:
    destinations: Vec<Destination>,
    // Return the transaction key after sending.
    get_tx_key: bool,
}

#[derive(Serialize, Debug, Clone)]
pub struct Destination {
    amount: u64,
    address: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Transfer {
    pub amount: u64,
    pub fee: u64,
    pub multisig_txset: String,
    pub tx_blob: String,
    pub tx_hash: String,
    pub tx_key: String,
    pub tx_metadata: String,
    pub unsigned_txset: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct BlockHeight {
    pub height: u32,
}

#[derive(Serialize, Debug, Clone)]
struct CheckTxKeyParams {
    #[serde(rename = "txid")]
    tx_id: String,
    tx_key: String,
    address: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct CheckTxKey {
    pub confirmations: u32,
    pub in_pool: bool,
    pub received: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct GenerateFromKeysParams {
    pub restore_height: u32,
    pub filename: String,
    pub address: String,
    pub spendkey: String,
    pub viewkey: String,
    pub password: String,
    pub autosave_current: bool,
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
