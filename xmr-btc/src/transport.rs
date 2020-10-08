use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait Send<SendMsg> {
    async fn send_message(&mut self, message: SendMsg) -> Result<()>;
}

#[async_trait]
pub trait Receive<RecvMsg> {
    async fn receive_message(&mut self) -> Result<RecvMsg>;
}
