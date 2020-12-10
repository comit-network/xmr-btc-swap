use crate::{
    alice::{Behaviour, OutEvent},
    network::{request_response::AliceToBob, transport::SwapTransport, TokioExecutor},
    SwapAmounts,
};
use anyhow::{Context, Result};
use futures::FutureExt;
use libp2p::{
    core::Multiaddr, futures::StreamExt, request_response::ResponseChannel, PeerId, Swarm,
};
use tokio::sync::mpsc::{Receiver, Sender};
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

pub struct EventLoopHandle {
    pub msg0: Receiver<bob::Message0>,
    pub msg1: Receiver<(bob::Message1, ResponseChannel<AliceToBob>)>,
    pub msg2: Receiver<(bob::Message2, ResponseChannel<AliceToBob>)>,
    pub msg3: Receiver<bob::Message3>,
    pub request: Receiver<crate::alice::amounts::OutEvent>,
    pub conn_established: Receiver<PeerId>,
    pub send_amounts: Sender<(ResponseChannel<AliceToBob>, SwapAmounts)>,
    pub send_msg1: Sender<(ResponseChannel<AliceToBob>, alice::Message1)>,
    pub send_msg2: Sender<(ResponseChannel<AliceToBob>, alice::Message2)>,
}

impl EventLoopHandle {
    pub async fn recv_conn_established(&mut self) -> Result<PeerId> {
        self.conn_established
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive connection established from Bob"))
    }

    pub async fn recv_message0(&mut self) -> Result<bob::Message0> {
        self.msg0
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive message 0 from Bob"))
    }

    pub async fn recv_message1(&mut self) -> Result<(bob::Message1, ResponseChannel<AliceToBob>)> {
        self.msg1
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive message 1 from Bob"))
    }

    pub async fn recv_message2(&mut self) -> Result<(bob::Message2, ResponseChannel<AliceToBob>)> {
        self.msg2
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed o receive message 2 from Bob"))
    }

    pub async fn recv_message3(&mut self) -> Result<bob::Message3> {
        self.msg3.recv().await.ok_or_else(|| {
            anyhow::Error::msg("Failed to receive Bitcoin encrypted signature from Bob")
        })
    }

    pub async fn recv_request(&mut self) -> Result<crate::alice::amounts::OutEvent> {
        self.request
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive amounts request from Bob"))
    }

    pub async fn send_amounts(
        &mut self,
        channel: ResponseChannel<AliceToBob>,
        amounts: SwapAmounts,
    ) -> Result<()> {
        let _ = self.send_amounts.send((channel, amounts)).await?;
        Ok(())
    }

    pub async fn send_message1(
        &mut self,
        channel: ResponseChannel<AliceToBob>,
        msg: alice::Message1,
    ) -> Result<()> {
        let _ = self.send_msg1.send((channel, msg)).await?;
        Ok(())
    }

    pub async fn send_message2(
        &mut self,
        channel: ResponseChannel<AliceToBob>,
        msg: alice::Message2,
    ) -> Result<()> {
        let _ = self.send_msg2.send((channel, msg)).await?;
        Ok(())
    }
}

pub struct EventLoop {
    pub swarm: libp2p::Swarm<Behaviour>,
    pub msg0: Sender<bob::Message0>,
    pub msg1: Sender<(bob::Message1, ResponseChannel<AliceToBob>)>,
    pub msg2: Sender<(bob::Message2, ResponseChannel<AliceToBob>)>,
    pub msg3: Sender<bob::Message3>,
    pub request: Sender<crate::alice::amounts::OutEvent>,
    pub conn_established: Sender<PeerId>,
    pub send_amounts: Receiver<(ResponseChannel<AliceToBob>, SwapAmounts)>,
    pub send_msg1: Receiver<(ResponseChannel<AliceToBob>, alice::Message1)>,
    pub send_msg2: Receiver<(ResponseChannel<AliceToBob>, alice::Message2)>,
}

impl EventLoop {
    pub fn new(
        transport: SwapTransport,
        behaviour: Behaviour,
        listen: Multiaddr,
    ) -> Result<(Self, EventLoopHandle)> {
        let local_peer_id = behaviour.peer_id();

        let mut swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, local_peer_id)
            .executor(Box::new(TokioExecutor {
                handle: tokio::runtime::Handle::current(),
            }))
            .build();

        Swarm::listen_on(&mut swarm, listen.clone())
            .with_context(|| format!("Address is not supported: {:#}", listen))?;

        let msg0 = Channels::new();
        let msg1 = Channels::new();
        let msg2 = Channels::new();
        let msg3 = Channels::new();
        let request = Channels::new();
        let conn_established = Channels::new();
        let send_amounts = Channels::new();
        let send_msg1 = Channels::new();
        let send_msg2 = Channels::new();

        let driver = EventLoop {
            swarm,
            msg0: msg0.sender,
            msg1: msg1.sender,
            msg2: msg2.sender,
            msg3: msg3.sender,
            request: request.sender,
            conn_established: conn_established.sender,
            send_amounts: send_amounts.receiver,
            send_msg1: send_msg1.receiver,
            send_msg2: send_msg2.receiver,
        };

        let handle = EventLoopHandle {
            msg0: msg0.receiver,
            msg1: msg1.receiver,
            msg2: msg2.receiver,
            msg3: msg3.receiver,
            request: request.receiver,
            conn_established: conn_established.receiver,
            send_amounts: send_amounts.sender,
            send_msg1: send_msg1.sender,
            send_msg2: send_msg2.sender,
        };

        Ok((driver, handle))
    }

    pub async fn run(&mut self) {
        loop {
            tokio::select! {
                swarm_event = self.swarm.next().fuse() => {
                    match swarm_event {
                        OutEvent::ConnectionEstablished(alice) => {
                            let _ = self.conn_established.send(alice).await;
                        }
                        OutEvent::Message0(msg) => {
                            let _ = self.msg0.send(msg).await;
                        }
                        OutEvent::Message1 { msg, channel } => {
                            let _ = self.msg1.send((msg, channel)).await;
                        }
                        OutEvent::Message2 { msg, channel } => {
                            let _ = self.msg2.send((msg, channel)).await;
                        }
                        OutEvent::Message3(msg) => {
                            let _ = self.msg3.send(msg).await;
                        }
                        OutEvent::Request(event) => {
                            let _ = self.request.send(event).await;
                        }
                    }
                },
                amounts = self.send_amounts.next().fuse() => {
                    if let Some((channel, amounts)) = amounts  {
                        self.swarm.send_amounts(channel, amounts);
                    }
                },
                msg1 = self.send_msg1.next().fuse() => {
                    if let Some((channel, msg)) = msg1  {
                        self.swarm.send_message1(channel, msg);
                    }
                },
                msg2 = self.send_msg2.next().fuse() => {
                    if let Some((channel, msg)) = msg2  {
                        self.swarm.send_message2(channel, msg);
                    }
                },
            }
        }
    }
}
