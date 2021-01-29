use crate::{
    network::{request_response::AliceToBob, transport::SwapTransport, TokioExecutor},
    protocol::{
        alice,
        alice::{Behaviour, OutEvent, SwapResponse, TransferProof},
        bob,
        bob::EncryptedSignature,
    },
};
use anyhow::{anyhow, Context, Result};
use libp2p::{
    core::Multiaddr, futures::FutureExt, request_response::ResponseChannel, PeerId, Swarm,
};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{error, trace};

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
    recv_message0: Receiver<(bob::Message0, ResponseChannel<AliceToBob>)>,
    recv_message1: Receiver<(bob::Message1, ResponseChannel<AliceToBob>)>,
    recv_message2: Receiver<bob::Message2>,
    recv_encrypted_signature: Receiver<EncryptedSignature>,
    request: Receiver<crate::protocol::alice::swap_response::OutEvent>,
    conn_established: Receiver<PeerId>,
    send_swap_response: Sender<(ResponseChannel<AliceToBob>, SwapResponse)>,
    send_message0: Sender<(ResponseChannel<AliceToBob>, alice::Message0)>,
    send_message1: Sender<(ResponseChannel<AliceToBob>, alice::Message1)>,
    send_transfer_proof: Sender<(PeerId, TransferProof)>,
    recv_transfer_proof_ack: Receiver<()>,
}

impl EventLoopHandle {
    pub async fn recv_conn_established(&mut self) -> Result<PeerId> {
        self.conn_established
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive connection established from Bob"))
    }

    pub async fn recv_message0(&mut self) -> Result<(bob::Message0, ResponseChannel<AliceToBob>)> {
        self.recv_message0
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive message 0 from Bob"))
    }

    pub async fn recv_message1(&mut self) -> Result<(bob::Message1, ResponseChannel<AliceToBob>)> {
        self.recv_message1
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive message 1 from Bob"))
    }

    pub async fn recv_message2(&mut self) -> Result<bob::Message2> {
        self.recv_message2
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive message 2 from Bob"))
    }

    pub async fn recv_encrypted_signature(&mut self) -> Result<EncryptedSignature> {
        self.recv_encrypted_signature
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
        let _ = self.send_message0.send((channel, msg)).await?;
        Ok(())
    }

    pub async fn send_message1(
        &mut self,
        channel: ResponseChannel<AliceToBob>,
        msg: alice::Message1,
    ) -> Result<()> {
        let _ = self.send_message1.send((channel, msg)).await?;
        Ok(())
    }

    pub async fn send_transfer_proof(&mut self, bob: PeerId, msg: TransferProof) -> Result<()> {
        let _ = self.send_transfer_proof.send((bob, msg)).await?;

        self.recv_transfer_proof_ack
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive transfer proof ack from Bob"))?;
        Ok(())
    }
}

#[allow(missing_debug_implementations)]
pub struct EventLoop {
    swarm: libp2p::Swarm<Behaviour>,
    recv_message0: Sender<(bob::Message0, ResponseChannel<AliceToBob>)>,
    recv_message1: Sender<(bob::Message1, ResponseChannel<AliceToBob>)>,
    recv_message2: Sender<bob::Message2>,
    recv_encrypted_signature: Sender<EncryptedSignature>,
    request: Sender<crate::protocol::alice::swap_response::OutEvent>,
    conn_established: Sender<PeerId>,
    send_swap_response: Receiver<(ResponseChannel<AliceToBob>, SwapResponse)>,
    send_message0: Receiver<(ResponseChannel<AliceToBob>, alice::Message0)>,
    send_message1: Receiver<(ResponseChannel<AliceToBob>, alice::Message1)>,
    send_transfer_proof: Receiver<(PeerId, TransferProof)>,
    recv_transfer_proof_ack: Sender<()>,
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

        let recv_message0 = Channels::new();
        let recv_message1 = Channels::new();
        let recv_message2 = Channels::new();
        let recv_encrypted_signature = Channels::new();
        let request = Channels::new();
        let conn_established = Channels::new();
        let send_swap_response = Channels::new();
        let send_message0 = Channels::new();
        let send_message1 = Channels::new();
        let send_transfer_proof = Channels::new();
        let recv_transfer_proof_ack = Channels::new();

        let driver = EventLoop {
            swarm,
            recv_message0: recv_message0.sender,
            recv_message1: recv_message1.sender,
            recv_message2: recv_message2.sender,
            recv_encrypted_signature: recv_encrypted_signature.sender,
            request: request.sender,
            conn_established: conn_established.sender,
            send_swap_response: send_swap_response.receiver,
            send_message0: send_message0.receiver,
            send_message1: send_message1.receiver,
            send_transfer_proof: send_transfer_proof.receiver,
            recv_transfer_proof_ack: recv_transfer_proof_ack.sender,
        };

        let handle = EventLoopHandle {
            recv_message0: recv_message0.receiver,
            recv_message1: recv_message1.receiver,
            recv_message2: recv_message2.receiver,
            recv_encrypted_signature: recv_encrypted_signature.receiver,
            request: request.receiver,
            conn_established: conn_established.receiver,
            send_swap_response: send_swap_response.sender,
            send_message0: send_message0.sender,
            send_message1: send_message1.sender,
            send_transfer_proof: send_transfer_proof.sender,
            recv_transfer_proof_ack: recv_transfer_proof_ack.receiver,
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
                            let _ = self.recv_message0.send((*msg, channel)).await;
                        }
                        OutEvent::Message1 { msg, channel } => {
                            let _ = self.recv_message1.send((msg, channel)).await;
                        }
                        OutEvent::Message2 { msg, bob_peer_id : _} => {
                            let _ = self.recv_message2.send(*msg).await;
                        }
                        OutEvent::TransferProofAcknowledged => {
                            trace!("Bob acknowledged transfer proof");
                            let _ = self.recv_transfer_proof_ack.send(()).await;
                        }
                        OutEvent::EncryptedSignature(msg) => {
                            let _ = self.recv_encrypted_signature.send(msg).await;
                        }
                        OutEvent::Request(event) => {
                            let _ = self.request.send(*event).await;
                        }
                    }
                },
                swap_response = self.send_swap_response.recv().fuse() => {
                    if let Some((channel, swap_response)) = swap_response  {
                        let _ = self
                            .swarm
                            .send_swap_response(channel, swap_response)
                            .map_err(|err|error!("Failed to send swap response: {:#}", err));
                    }
                },
                msg0 = self.send_message0.recv().fuse() => {
                    if let Some((channel, msg)) = msg0  {
                        let _ = self
                            .swarm
                            .send_message0(channel, msg)
                            .map_err(|err|error!("Failed to send message0: {:#}", err));
                    }
                },
                msg1 = self.send_message1.recv().fuse() => {
                    if let Some((channel, msg)) = msg1  {
                        let _ = self
                            .swarm
                            .send_message1(channel, msg)
                            .map_err(|err|error!("Failed to send message1: {:#}", err));
                    }
                },
                transfer_proof = self.send_transfer_proof.recv().fuse() => {
                    if let Some((bob_peer_id, msg)) = transfer_proof  {
                      self.swarm.send_transfer_proof(bob_peer_id, msg)
                    }
                },
            }
        }
    }
}
