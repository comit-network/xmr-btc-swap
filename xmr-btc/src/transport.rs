use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait SendReceive<SendMsg, RecvMsg> {
    async fn send_message(&mut self, message: SendMsg) -> Result<()>;
    async fn receive_message(&mut self) -> Result<RecvMsg>;
}
