use crate::{
    network::{request_response::AliceToBob, transport::SwapTransport, TokioExecutor},
    protocol::{
        alice,
        alice::{Behaviour, Message4, OutEvent, SwapResponse},
        bob,
        bob::Message5,
    },
};
use anyhow::{anyhow, Context, Result};
use futures::FutureExt;
use libp2p::{
    core::Multiaddr, futures::StreamExt, request_response::ResponseChannel, PeerId, Swarm,
};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::trace;

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
    msg2: Receiver<bob::Message2>,
    msg5: Receiver<Message5>,
    request: Receiver<crate::protocol::alice::swap_response::OutEvent>,
    conn_established: Receiver<PeerId>,
    send_swap_response: Sender<(ResponseChannel<AliceToBob>, SwapResponse)>,
    send_msg0: Sender<(ResponseChannel<AliceToBob>, alice::Message0)>,
    send_msg1: Sender<(ResponseChannel<AliceToBob>, alice::Message1)>,
    send_msg4: Sender<(PeerId, Message4)>,
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

    pub async fn recv_message2(&mut self) -> Result<bob::Message2> {
        self.msg2
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive message 2 from Bob"))
    }

    pub async fn recv_message5(&mut self) -> Result<Message5> {
        self.msg5
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

    pub async fn send_message4(&mut self, bob: PeerId, msg: Message4) -> Result<()> {
        let _ = self.send_msg4.send((bob, msg)).await?;
        Ok(())
    }
}

#[allow(missing_debug_implementations)]
pub struct EventLoop {
    swarm: libp2p::Swarm<Behaviour>,
    msg0: Sender<(bob::Message0, ResponseChannel<AliceToBob>)>,
    msg1: Sender<(bob::Message1, ResponseChannel<AliceToBob>)>,
    msg2: Sender<bob::Message2>,
    msg5: Sender<Message5>,
    request: Sender<crate::protocol::alice::swap_response::OutEvent>,
    conn_established: Sender<PeerId>,
    send_swap_response: Receiver<(ResponseChannel<AliceToBob>, SwapResponse)>,
    send_msg0: Receiver<(ResponseChannel<AliceToBob>, alice::Message0)>,
    send_msg1: Receiver<(ResponseChannel<AliceToBob>, alice::Message1)>,
    send_msg4: Receiver<(PeerId, Message4)>,
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
        let msg5 = Channels::new();
        let request = Channels::new();
        let conn_established = Channels::new();
        let send_swap_response = Channels::new();
        let send_msg0 = Channels::new();
        let send_msg1 = Channels::new();
        let send_msg4 = Channels::new();

        let driver = EventLoop {
            swarm,
            msg0: msg0.sender,
            msg1: msg1.sender,
            msg2: msg2.sender,
            msg5: msg5.sender,
            request: request.sender,
            conn_established: conn_established.sender,
            send_swap_response: send_swap_response.receiver,
            send_msg0: send_msg0.receiver,
            send_msg1: send_msg1.receiver,
            send_msg4: send_msg4.receiver,
        };

        let handle = EventLoopHandle {
            msg0: msg0.receiver,
            msg1: msg1.receiver,
            msg2: msg2.receiver,
            msg5: msg5.receiver,
            request: request.receiver,
            conn_established: conn_established.receiver,
            send_swap_response: send_swap_response.sender,
            send_msg0: send_msg0.sender,
            send_msg1: send_msg1.sender,
            send_msg4: send_msg4.sender,
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
                        OutEvent::Message2 { msg, bob_peer_id : _} => {
                            let _ = self.msg2.send(*msg).await;
                        }
                        OutEvent::Message4 => trace!("Bob ack'd message 4"),
                        OutEvent::Message5(msg) => {
                            let _ = self.msg5.send(msg).await;
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
                msg4 = self.send_msg4.next().fuse() => {
                    if let Some((bob_peer_id, msg)) = msg4  {
                        self.swarm.send_message4(bob_peer_id, msg);
                    }
                },
            }
        }
    }
}
