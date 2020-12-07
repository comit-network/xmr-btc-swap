use crate::{
    alice::{Behaviour, OutEvent},
    network::{request_response::AliceToBob, transport::SwapTransport, TokioExecutor},
    SwapAmounts,
};
use anyhow::{Context, Result};
use libp2p::{core::Multiaddr, request_response::ResponseChannel, PeerId, Swarm};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::info;
use xmr_btc::{alice, bob};

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
    pub msg0: Channels<bob::Message0>,
    pub msg1: Channels<(bob::Message1, ResponseChannel<AliceToBob>)>,
    pub msg2: Channels<(bob::Message2, ResponseChannel<AliceToBob>)>,
    pub msg3: Channels<bob::Message3>,
    pub request: Channels<crate::alice::amounts::OutEvent>,
    pub conn_established: Channels<PeerId>,
}

impl SwarmDriver {
    pub fn new(transport: SwapTransport, behaviour: Behaviour, listen: Multiaddr) -> Result<Self> {
        let local_peer_id = behaviour.peer_id();

        let mut swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, local_peer_id)
            .executor(Box::new(TokioExecutor {
                handle: tokio::runtime::Handle::current(),
            }))
            .build();

        Swarm::listen_on(&mut swarm, listen.clone())
            .with_context(|| format!("Address is not supported: {:#}", listen))?;

        Ok(SwarmDriver {
            swarm,
            msg0: Channels::new(),
            msg1: Channels::new(),
            msg2: Channels::new(),
            msg3: Channels::new(),
            request: Channels::new(),
            conn_established: Channels::new(),
        })
    }

    pub async fn poll_swarm(mut self) {
        loop {
            match self.swarm.next().await {
                OutEvent::ConnectionEstablished(alice) => {
                    let _ = self.conn_established.sender.send(alice).await;
                }
                OutEvent::Message0(msg) => {
                    let _ = self.msg0.sender.send(msg).await;
                }
                OutEvent::Message1 { msg, channel } => {
                    let _ = self.msg1.sender.send((msg, channel)).await;
                }
                OutEvent::Message2 { msg, channel } => {
                    let _ = self.msg2.sender.send((msg, channel)).await;
                }
                OutEvent::Message3(msg) => {
                    let _ = self.msg3.sender.send(msg).await;
                }
                OutEvent::Request(event) => {
                    let _ = self.request.sender.send(event).await;
                }
            };
        }
    }

    pub fn send_amounts(&mut self, channel: ResponseChannel<AliceToBob>, amounts: SwapAmounts) {
        let msg = AliceToBob::Amounts(amounts);
        self.swarm.amounts.send(channel, msg);
        info!("Sent amounts response");
    }

    pub fn send_message1(&mut self, channel: ResponseChannel<AliceToBob>, msg: alice::Message1) {
        self.swarm.send_message1(channel, msg);
    }

    pub fn send_message2(&mut self, channel: ResponseChannel<AliceToBob>, msg: alice::Message2) {
        self.swarm.send_message2(channel, msg);
    }

    pub async fn recv_conn_established(&mut self) -> Result<PeerId> {
        self.conn_established
            .receiver
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive connection established from Bob"))
    }

    pub async fn recv_message0(&mut self) -> Result<bob::Message0> {
        self.msg0
            .receiver
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive message 0 from Bob"))
    }

    pub async fn recv_message1(&mut self) -> Result<(bob::Message1, ResponseChannel<AliceToBob>)> {
        self.msg1
            .receiver
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive message 1 from Bob"))
    }

    pub async fn recv_message2(&mut self) -> Result<(bob::Message2, ResponseChannel<AliceToBob>)> {
        self.msg2
            .receiver
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed o receive message 2 from Bob"))
    }

    pub async fn recv_message3(&mut self) -> Result<bob::Message3> {
        self.msg3.receiver.recv().await.ok_or_else(|| {
            anyhow::Error::msg("Failed to receive Bitcoin encrypted signature from Bob")
        })
    }

    pub async fn recv_request(&mut self) -> Result<crate::alice::amounts::OutEvent> {
        self.request
            .receiver
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive amounts request from Bob"))
    }
}
