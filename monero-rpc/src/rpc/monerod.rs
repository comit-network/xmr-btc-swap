use crate::rpc::{Request, Response};
use anyhow::Result;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// RPC client for monerod and monero-wallet-rpc.
#[derive(Debug, Clone)]
pub struct Client {
    pub inner: reqwest::Client,
    pub url: Url,
}

impl Client {
    /// New local host monerod RPC client.
    pub fn localhost(port: u16) -> Self {
        let url = format!("http://127.0.0.1:{}/json_rpc", port);
        let url = Url::parse(&url).expect("url is well formed");

        Self {
            inner: reqwest::Client::new(),
            url,
        }
    }

    pub async fn generate_blocks(
        &self,
        amount_of_blocks: u32,
        wallet_address: &str,
    ) -> Result<GenerateBlocks> {
        let params = GenerateBlocksParams {
            amount_of_blocks,
            wallet_address: wallet_address.to_owned(),
        };
        let url = self.url.clone();
        // // Step 1:  Get the auth header
        // let res = self.inner.get(url.clone()).send().await?;
        // let headers = res.headers();
        // let wwwauth = headers["www-authenticate"].to_str()?;
        //
        // // Step 2:  Given the auth header, sign the digest for the real req.
        // let tmp_url = url.clone();
        // let context = AuthContext::new("username", "password", tmp_url.path());
        // let mut prompt = digest_auth::parse(wwwauth)?;
        // let answer = prompt.respond(&context)?.to_header_string();

        let request = Request::new("generateblocks", params);

        let response = self
            .inner
            .post(url)
            .json(&request)
            .send()
            .await?
            .text()
            .await?;

        debug!("generate blocks response: {}", response);

        let res: Response<GenerateBlocks> = serde_json::from_str(&response)?;

        Ok(res.result)
    }

    // $ curl http://127.0.0.1:18081/json_rpc -d '{"jsonrpc":"2.0","id":"0","method":"get_block_header_by_height","params":{"height":1}}' -H 'Content-Type: application/json'
    pub async fn get_block_header_by_height(&self, height: u32) -> Result<BlockHeader> {
        let params = GetBlockHeaderByHeightParams { height };
        let request = Request::new("get_block_header_by_height", params);

        let response = self
            .inner
            .post(self.url.clone())
            .json(&request)
            .send()
            .await?
            .text()
            .await?;

        debug!("get block header by height response: {}", response);

        let res: Response<GetBlockHeaderByHeight> = serde_json::from_str(&response)?;

        Ok(res.result.block_header)
    }

    pub async fn get_block_count(&self) -> Result<u32> {
        let request = Request::new("get_block_count", "");

        let response = self
            .inner
            .post(self.url.clone())
            .json(&request)
            .send()
            .await?
            .text()
            .await?;

        debug!("get block count response: {}", response);

        let res: Response<BlockCount> = serde_json::from_str(&response)?;

        Ok(res.result.count)
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
