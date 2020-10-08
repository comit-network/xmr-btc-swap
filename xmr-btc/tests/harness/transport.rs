use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tokio::{
    stream::StreamExt,
    sync::mpsc::{Receiver, Sender},
};
use xmr_btc::transport::{ReceiveMessage, SendMessage};

#[derive(Debug)]
pub struct Transport<SendMsg, RecvMsg> {
    pub sender: Sender<SendMsg>,
    pub receiver: Receiver<RecvMsg>,
}

#[async_trait]
impl<SendMsg, RecvMsg> SendMessage<SendMsg> for Transport<SendMsg, RecvMsg>
where
    SendMsg: Send + Sync,
    RecvMsg: std::marker::Send,
{
    async fn send_message(&mut self, message: SendMsg) -> Result<()> {
        let _ = self
            .sender
            .send(message)
            .await
            .map_err(|_| anyhow!("failed to send message"))?;
        Ok(())
    }
}

#[async_trait]
impl<SendMsg, RecvMsg> ReceiveMessage<RecvMsg> for Transport<SendMsg, RecvMsg>
where
    SendMsg: std::marker::Send,
    RecvMsg: Send + Sync,
{
    async fn receive_message(&mut self) -> Result<RecvMsg> {
        let message = self
            .receiver
            .next()
            .await
            .ok_or_else(|| anyhow!("failed to receive message"))?;
        Ok(message)
    }
}
