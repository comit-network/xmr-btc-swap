use crate::rpc::monerod::{BlockHeader, GenerateBlocks};

#[jsonrpc_client::api(version = "1.0")]
#[async_trait::async_trait]
pub trait MonerodRpcApi {
    async fn generateblocks(&self, amount_of_blocks: u32, wallet_address: &str) -> GenerateBlocks;

    async fn get_block_header_by_height(&self, height: u32) -> BlockHeader;

    async fn get_block_count(&self) -> u32;
}
