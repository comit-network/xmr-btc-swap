use serde::{Deserialize, Serialize};

#[jsonrpc_client::api(version = "2.0")]
pub trait MoneroWalletRpc {
    async fn get_address(&self, account_index: u32) -> GetAddress;
    async fn get_balance(&self, account_index: u32) -> u64;
    async fn create_account(&self, label: String) -> CreateAccount;
    async fn get_accounts(&self, tag: String) -> GetAccounts;
    async fn open_wallet(&self, filename: String);
    async fn close_wallet(&self);
    async fn create_wallet(&self, filename: String, language: String);
    async fn transfer(&self, account_index: u32, destinations: Vec<Destination>) -> Transfer;
    async fn get_height(&self) -> BlockHeight;
    async fn check_tx_key(&self, tx_id: String, tx_key: String, address: String) -> CheckTxKey;
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
}

#[jsonrpc_client::implement(MoneroWalletRpc)]
#[derive(Debug, Clone)]
pub struct Client {
    inner: reqwest::Client,
    base_url: reqwest::Url,
}

impl Client {
    /// Constructs a monero-wallet-rpc client with localhost endpoint.
    pub fn localhost(port: u16) -> Self {
        Client::new(
            format!("http://127.0.0.1:{}/json_rpc", port)
                .parse()
                .expect("url is well formed"),
        )
    }

    /// Constructs a monero-wallet-rpc client with `url` endpoint.
    pub fn new(url: reqwest::Url) -> Self {
        Self {
            inner: reqwest::Client::new(),
            base_url: url,
        }
    }

    /// Transfers `amount` monero from `account_index` to `address`.
    pub async fn transfer_single(
        &self,
        account_index: u32,
        amount: u64,
        address: &str,
    ) -> Result<Transfer, jsonrpc_client::Error<reqwest::Error>> {
        let dest = vec![Destination {
            amount,
            address: address.to_owned(),
        }];
        self.transfer(account_index, dest).await
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
struct OpenWalletParams {
    filename: String,
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize)]
pub struct SweepAllParams {
    pub address: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SweepAll {
    amount_list: Vec<u64>,
    fee_list: Vec<u64>,
    multisig_txset: String,
    pub tx_hash_list: Vec<String>,
    unsigned_txset: String,
    weight_list: Vec<u32>,
}

#[derive(Debug, Copy, Clone, Deserialize)]
pub struct Version {
    version: u32,
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

        let _: Response<SweepAll> = serde_json::from_str(&response).unwrap();
    }
}
