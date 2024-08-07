use anyhow::{Context, Result};
use monero::cryptonote::hash::Hash;
use monero::util::ringct;
use monero::PublicKey;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize, Serializer};

#[jsonrpc_client::api(version = "2.0")]
pub trait MonerodRpc {
    async fn generateblocks(&self, amount_of_blocks: u32, wallet_address: String)
        -> GenerateBlocks;
    async fn get_block_header_by_height(&self, height: u32) -> BlockHeader;
    async fn get_block_count(&self) -> BlockCount;
    async fn get_block(&self, height: u32) -> GetBlockResponse;
}

#[jsonrpc_client::implement(MonerodRpc)]
#[derive(Debug, Clone)]
pub struct Client {
    inner: reqwest::Client,
    base_url: reqwest::Url,
    get_o_indexes_bin_url: reqwest::Url,
    get_outs_bin_url: reqwest::Url,
}

impl Client {
    /// New local host monerod RPC client.
    pub fn localhost(port: u16) -> Result<Self> {
        Self::new("127.0.0.1".to_owned(), port)
    }

    fn new(host: String, port: u16) -> Result<Self> {
        Ok(Self {
            inner: reqwest::ClientBuilder::new()
                .connection_verbose(true)
                .build()?,
            base_url: format!("http://{}:{}/json_rpc", host, port)
                .parse()
                .context("url is well formed")?,
            get_o_indexes_bin_url: format!("http://{}:{}/get_o_indexes.bin", host, port)
                .parse()
                .context("url is well formed")?,
            get_outs_bin_url: format!("http://{}:{}/get_outs.bin", host, port)
                .parse()
                .context("url is well formed")?,
        })
    }

    pub async fn get_o_indexes(&self, txid: Hash) -> Result<GetOIndexesResponse> {
        self.binary_request(
            self.get_o_indexes_bin_url.clone(),
            GetOIndexesPayload { txid },
        )
        .await
    }

    pub async fn get_outs(&self, outputs: Vec<GetOutputsOut>) -> Result<GetOutsResponse> {
        self.binary_request(self.get_outs_bin_url.clone(), GetOutsPayload { outputs })
            .await
    }

    async fn binary_request<Req, Res>(&self, url: reqwest::Url, request: Req) -> Result<Res>
    where
        Req: Serialize,
        Res: DeserializeOwned,
    {
        let response = self
            .inner
            .post(url)
            .body(monero_epee_bin_serde::to_bytes(&request)?)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Request failed with status code {}", response.status())
        }

        let body = response.bytes().await?;

        Ok(monero_epee_bin_serde::from_bytes(body)?)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct GenerateBlocks {
    pub blocks: Vec<String>,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct BlockCount {
    pub count: u32,
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

#[derive(Debug, Deserialize)]
pub struct GetBlockResponse {
    #[serde(with = "monero_serde_hex_block")]
    pub blob: monero::Block,
}

#[derive(Debug, Deserialize)]
pub struct GetIndexesResponse {
    pub o_indexes: Vec<u32>,
}

#[derive(Clone, Debug, Serialize)]
struct GetOIndexesPayload {
    #[serde(with = "byte_array")]
    txid: Hash,
}

#[derive(Clone, Debug, Serialize)]
struct GetOutsPayload {
    outputs: Vec<GetOutputsOut>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct GetOutputsOut {
    pub amount: u64,
    pub index: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct GetOutsResponse {
    #[serde(flatten)]
    pub base: BaseResponse,
    pub outs: Vec<OutKey>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct OutKey {
    pub height: u64,
    #[serde(with = "byte_array")]
    pub key: PublicKey,
    #[serde(with = "byte_array")]
    pub mask: ringct::Key,
    #[serde(with = "byte_array")]
    pub txid: Hash,
    pub unlocked: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct BaseResponse {
    pub credits: u64,
    pub status: Status,
    pub top_hash: String,
    pub untrusted: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct GetOIndexesResponse {
    #[serde(flatten)]
    pub base: BaseResponse,
    #[serde(default)]
    pub o_indexes: Vec<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum Status {
    #[serde(rename = "OK")]
    Ok,
    #[serde(rename = "Failed")]
    Failed,
}

mod monero_serde_hex_block {
    use super::*;
    use monero::consensus::Decodable;
    use serde::de::Error;
    use serde::{Deserialize, Deserializer};
    use std::io::Cursor;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<monero::Block, D::Error>
    where
        D: Deserializer<'de>,
    {
        let hex = String::deserialize(deserializer)?;

        let bytes = hex::decode(hex).map_err(D::Error::custom)?;
        let mut cursor = Cursor::new(bytes);

        let block = monero::Block::consensus_decode(&mut cursor).map_err(D::Error::custom)?;

        Ok(block)
    }
}

mod byte_array {
    use super::*;
    use serde::de::Error;
    use serde::Deserializer;
    use std::convert::TryFrom;
    use std::fmt;
    use std::marker::PhantomData;

    pub fn serialize<S, B>(bytes: B, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        B: AsRef<[u8]>,
    {
        serializer.serialize_bytes(bytes.as_ref())
    }

    pub fn deserialize<'de, D, B, const N: usize>(deserializer: D) -> Result<B, D::Error>
    where
        D: Deserializer<'de>,
        B: TryFrom<[u8; N]>,
    {
        struct Visitor<T, const N: usize> {
            phantom: PhantomData<(T, [u8; N])>,
        }

        impl<'de, T, const N: usize> serde::de::Visitor<'de> for Visitor<T, N>
        where
            T: TryFrom<[u8; N]>,
        {
            type Value = T;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a byte buffer")
            }

            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
            where
                E: Error,
            {
                let bytes = <[u8; N]>::try_from(v).map_err(|_| {
                    E::custom(format!("Failed to construct [u8; {}] from buffer", N))
                })?;
                let result = T::try_from(bytes)
                    .map_err(|_| E::custom(format!("Failed to construct T from [u8; {}]", N)))?;

                Ok(result)
            }
        }

        deserializer.deserialize_byte_buf(Visitor {
            phantom: PhantomData,
        })
    }
}
