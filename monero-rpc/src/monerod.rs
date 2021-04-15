use serde::{Deserialize, Serialize};

#[jsonrpc_client::api(version = "2.0")]
pub trait MonerodRpc {
    async fn generateblocks(&self, amount_of_blocks: u32, wallet_address: String)
        -> GenerateBlocks;
    async fn get_block_header_by_height(&self, height: u32) -> BlockHeader;
    async fn get_block_count(&self) -> u32;
}

#[jsonrpc_client::implement(MonerodRpc)]
#[derive(Debug, Clone)]
pub struct Client {
    inner: reqwest::Client,
    base_url: reqwest::Url,
}

impl Client {
    /// New local host monerod RPC client.
    pub fn localhost(port: u16) -> Self {
        Self {
            inner: reqwest::Client::new(),
            base_url: format!("http://127.0.0.1:{}/json_rpc", port)
                .parse()
                .expect("url is well formed"),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct GenerateBlocksParams {
    amount_of_blocks: u32,
    wallet_address: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GenerateBlocks {
    pub blocks: Vec<String>,
    pub height: u32,
    pub status: String,
}

#[derive(Clone, Debug, Serialize)]
struct GetBlockHeaderByHeightParams {
    height: u32,
}

#[derive(Clone, Debug, Deserialize)]
struct GetBlockHeaderByHeight {
    block_header: BlockHeader,
    status: String,
    untrusted: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct BlockCount {
    count: u32,
    status: String,
}

// We should be able to use monero-rs for this but it does not include all
// the fields.
#[derive(Clone, Debug, Deserialize)]
pub struct BlockHeader {
    pub block_size: u32,
    pub depth: u32,
    pub difficulty: u32,
    pub hash: String,
    pub height: u32,
    pub major_version: u32,
    pub minor_version: u32,
    pub nonce: u32,
    pub num_txes: u32,
    pub orphan_status: bool,
    pub prev_hash: String,
    pub reward: u64,
    pub timestamp: u32,
}
