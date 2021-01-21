use anyhow::{anyhow, Context, Result};
use futures::FutureExt;
use libp2p::{
    core::Multiaddr, futures::StreamExt, request_response::ResponseChannel, PeerId, Swarm,
};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::{
    network::{request_response::AliceToBob, transport::SwapTransport, TokioExecutor},
    protocol::{
        alice,
        alice::{Behaviour, OutEvent, SwapResponse},
        bob,
    },
};

#[allow(missing_debug_implementations)]
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

#[derive(Debug)]
pub struct EventLoopHandle {
    msg0: Receiver<(bob::Message0, ResponseChannel<AliceToBob>)>,
    msg1: Receiver<(bob::Message1, ResponseChannel<AliceToBob>)>,
    msg2: Receiver<(bob::Message2, ResponseChannel<AliceToBob>)>,
    msg3: Receiver<bob::Message3>,
    request: Receiver<crate::protocol::alice::swap_response::OutEvent>,
    conn_established: Receiver<PeerId>,
    send_swap_response: Sender<(ResponseChannel<AliceToBob>, SwapResponse)>,
    send_msg0: Sender<(ResponseChannel<AliceToBob>, alice::Message0)>,
    send_msg1: Sender<(ResponseChannel<AliceToBob>, alice::Message1)>,
    send_msg2: Sender<(ResponseChannel<AliceToBob>, alice::Message2)>,
}

impl EventLoopHandle {
    pub async fn recv_conn_established(&mut self) -> Result<PeerId> {
        self.conn_established
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive connection established from Bob"))
    }

    pub async fn recv_message0(&mut self) -> Result<(bob::Message0, ResponseChannel<AliceToBob>)> {
        self.msg0
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive message 0 from Bob"))
    }

    pub async fn recv_message1(&mut self) -> Result<(bob::Message1, ResponseChannel<AliceToBob>)> {
        self.msg1
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive message 1 from Bob"))
    }

    pub async fn recv_message2(&mut self) -> Result<(bob::Message2, ResponseChannel<AliceToBob>)> {
        self.msg2
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed o receive message 2 from Bob"))
    }

    pub async fn recv_message3(&mut self) -> Result<bob::Message3> {
        self.msg3
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive Bitcoin encrypted signature from Bob"))
    }

    pub async fn recv_request(
        &mut self,
    ) -> Result<crate::protocol::alice::swap_response::OutEvent> {
        self.request
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive amounts request from Bob"))
    }

    pub async fn send_swap_response(
        &mut self,
        channel: ResponseChannel<AliceToBob>,
        swap_response: SwapResponse,
    ) -> Result<()> {
        let _ = self
            .send_swap_response
            .send((channel, swap_response))
            .await?;
        Ok(())
    }

    pub async fn send_message0(
        &mut self,
        channel: ResponseChannel<AliceToBob>,
        msg: alice::Message0,
    ) -> Result<()> {
        let _ = self.send_msg0.send((channel, msg)).await?;
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

#[allow(missing_debug_implementations)]
pub struct EventLoop {
    swarm: libp2p::Swarm<Behaviour>,
    msg0: Sender<(bob::Message0, ResponseChannel<AliceToBob>)>,
    msg1: Sender<(bob::Message1, ResponseChannel<AliceToBob>)>,
    msg2: Sender<(bob::Message2, ResponseChannel<AliceToBob>)>,
    msg3: Sender<bob::Message3>,
    request: Sender<crate::protocol::alice::swap_response::OutEvent>,
    conn_established: Sender<PeerId>,
    send_swap_response: Receiver<(ResponseChannel<AliceToBob>, SwapResponse)>,
    send_msg0: Receiver<(ResponseChannel<AliceToBob>, alice::Message0)>,
    send_msg1: Receiver<(ResponseChannel<AliceToBob>, alice::Message1)>,
    send_msg2: Receiver<(ResponseChannel<AliceToBob>, alice::Message2)>,
}

impl EventLoop {
    pub fn new(
        transport: SwapTransport,
        behaviour: Behaviour,
        listen: Multiaddr,
        peer_id: PeerId,
    ) -> Result<(Self, EventLoopHandle)> {
        let mut swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, peer_id)
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
        let send_swap_response = Channels::new();
        let send_msg0 = Channels::new();
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
            send_swap_response: send_swap_response.receiver,
            send_msg0: send_msg0.receiver,
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
            send_swap_response: send_swap_response.sender,
            send_msg0: send_msg0.sender,
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
                        OutEvent::Message0 { msg, channel } => {
                            let _ = self.msg0.send((*msg, channel)).await;
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
                            let _ = self.request.send(*event).await;
                        }
                    }
                },
                swap_response = self.send_swap_response.next().fuse() => {
                    if let Some((channel, swap_response)) = swap_response  {
                        self.swarm.send_swap_response(channel, swap_response);
                    }
                },
                msg0 = self.send_msg0.next().fuse() => {
                    if let Some((channel, msg)) = msg0  {
                        self.swarm.send_message0(channel, msg);
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
