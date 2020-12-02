use async_trait::async_trait;
use futures::{channel::mpsc::Receiver, StreamExt};
use xmr_btc::{
    alice::ReceiveBitcoinRedeemEncsig, bitcoin::EncryptedSignature, bob::ReceiveTransferProof,
    monero::TransferProof,
};

pub mod harness;

type AliceNetwork = Network<EncryptedSignature>;
type BobNetwork = Network<TransferProof>;

#[derive(Debug)]
struct Network<M> {
    // TODO: It is weird to use mpsc's in a situation where only one message is expected, but the
    // ownership rules of Rust are making this painful
    pub receiver: Receiver<M>,
}

#[async_trait]
impl ReceiveTransferProof for BobNetwork {
    async fn receive_transfer_proof(&mut self) -> TransferProof {
        self.receiver.next().await.unwrap()
    }
}

#[async_trait]
impl ReceiveBitcoinRedeemEncsig for AliceNetwork {
    async fn receive_bitcoin_redeem_encsig(&mut self) -> EncryptedSignature {
        self.receiver.next().await.unwrap()
    }
}
