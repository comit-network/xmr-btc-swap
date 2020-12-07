use crate::{
    bob::{Behaviour, OutEvent},
    network::{transport::SwapTransport, TokioExecutor},
    SwapAmounts,
};
use anyhow::Result;
use libp2p::{core::Multiaddr, PeerId};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::info;
use xmr_btc::{alice, bitcoin::EncryptedSignature, bob};

pub struct Channels<T> {
    sender: Sender<T>,
    receiver: Receiver<T>,
}

impl<T> Channels<T> {
    pub fn new() -> Channels<T> {
        let (sender, receiver) = tokio::sync::mpsc::channel(100);
        Channels { sender, receiver }
    }
}

impl<T> Default for Channels<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SwarmDriver {
    pub swarm: libp2p::Swarm<Behaviour>,
    pub amounts: Channels<SwapAmounts>,
    pub msg0: Channels<alice::Message0>,
    pub msg1: Channels<alice::Message1>,
    pub msg2: Channels<alice::Message2>,
    pub conn_established: Channels<PeerId>,
}

impl SwarmDriver {
    pub fn new(transport: SwapTransport, behaviour: Behaviour) -> Self {
        let local_peer_id = behaviour.peer_id();

        let swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, local_peer_id)
            .executor(Box::new(TokioExecutor {
                handle: tokio::runtime::Handle::current(),
            }))
            .build();

        SwarmDriver {
            swarm,
            amounts: Channels::new(),
            msg0: Channels::new(),
            msg1: Channels::new(),
            msg2: Channels::new(),
            conn_established: Channels::new(),
        }
    }

    pub async fn poll_swarm(mut self) {
        loop {
            match self.swarm.next().await {
                OutEvent::ConnectionEstablished(alice) => {
                    let _ = self.conn_established.sender.send(alice).await;
                }
                OutEvent::Amounts(amounts) => {
                    let _ = self.amounts.sender.send(amounts).await;
                }
                OutEvent::Message0(msg) => {
                    let _ = self.msg0.sender.send(msg).await;
                }
                OutEvent::Message1(msg) => {
                    let _ = self.msg1.sender.send(msg).await;
                }
                OutEvent::Message2(msg) => {
                    let _ = self.msg2.sender.send(msg).await;
                }
                OutEvent::Message3 => info!("Alice acknowledged message 3 received"),
            };
        }
    }

    // todo: Remove this
    pub fn request_amounts(&mut self, alice_peer_id: PeerId) {
        self.swarm.request_amounts(alice_peer_id, 0);
    }

    pub fn dial_alice(&mut self, addr: Multiaddr) -> Result<()> {
        let _ = libp2p::Swarm::dial_addr(&mut self.swarm, addr)?;
        Ok(())
    }

    pub fn send_message0(&mut self, peer_id: PeerId, msg: bob::Message0) {
        self.swarm.send_message0(peer_id, msg);
    }

    pub fn send_message1(&mut self, peer_id: PeerId, msg: bob::Message1) {
        self.swarm.send_message1(peer_id, msg);
    }

    pub fn send_message2(&mut self, peer_id: PeerId, msg: bob::Message2) {
        self.swarm.send_message2(peer_id, msg);
    }

    pub fn send_message3(&mut self, peer_id: PeerId, tx_redeem_encsig: EncryptedSignature) {
        self.swarm.send_message3(peer_id, tx_redeem_encsig);
    }

    pub async fn recv_conn_established(&mut self) -> Result<PeerId> {
        self.conn_established.receiver.recv().await.ok_or_else(|| {
            anyhow::Error::msg("Failed to receive connection established from Alice")
        })
    }

    pub async fn recv_amounts(&mut self) -> Result<SwapAmounts> {
        self.amounts
            .receiver
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive amounts from Alice"))
    }

    pub async fn recv_message0(&mut self) -> Result<alice::Message0> {
        self.msg0
            .receiver
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive message 0 from Alice"))
    }

    pub async fn recv_message1(&mut self) -> Result<alice::Message1> {
        self.msg1
            .receiver
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive message 1 from Alice"))
    }

    pub async fn recv_message2(&mut self) -> Result<alice::Message2> {
        self.msg2
            .receiver
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive message 2 from Alice"))
    }
}
