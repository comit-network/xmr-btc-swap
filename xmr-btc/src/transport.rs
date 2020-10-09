use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait SendMessage<SendMsg> {
    async fn send_message(&mut self, message: SendMsg) -> Result<()>;
}

#[async_trait]
pub trait ReceiveMessage<RecvMsg> {
    async fn receive_message(&mut self) -> Result<RecvMsg>;
}
