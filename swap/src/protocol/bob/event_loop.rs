use crate::{
    bitcoin::EncryptedSignature,
    network::{transport::SwapTransport, TokioExecutor},
    protocol::{
        alice,
        alice::SwapResponse,
        bob::{self, Behaviour, OutEvent, SwapRequest},
    },
};
use anyhow::{anyhow, Result};
use futures::FutureExt;
use libp2p::{core::Multiaddr, PeerId};
use tokio::{
    stream::StreamExt,
    sync::mpsc::{Receiver, Sender},
};
use tracing::{debug, error, info};

#[derive(Debug)]
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
    swap_response: Receiver<SwapResponse>,
    msg0: Receiver<alice::Message0>,
    msg1: Receiver<alice::Message1>,
    msg2: Receiver<alice::Message2>,
    conn_established: Receiver<PeerId>,
    dial_alice: Sender<()>,
    send_swap_request: Sender<SwapRequest>,
    send_msg0: Sender<bob::Message0>,
    send_msg1: Sender<bob::Message1>,
    send_msg2: Sender<bob::Message2>,
    send_msg3: Sender<EncryptedSignature>,
}

impl EventLoopHandle {
    pub async fn recv_swap_response(&mut self) -> Result<SwapResponse> {
        self.swap_response
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive swap response from Alice"))
    }

    pub async fn recv_message0(&mut self) -> Result<alice::Message0> {
        self.msg0
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive message 0 from Alice"))
    }

    pub async fn recv_message1(&mut self) -> Result<alice::Message1> {
        self.msg1
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive message 1 from Alice"))
    }

    pub async fn recv_message2(&mut self) -> Result<alice::Message2> {
        self.msg2
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed o receive message 2 from Alice"))
    }

    /// Dials other party and wait for the connection to be established.
    /// Do nothing if we are already connected
    pub async fn dial(&mut self) -> Result<()> {
        debug!("Attempt to dial Alice");
        let _ = self.dial_alice.send(()).await?;

        self.conn_established
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive connection established from Alice"))?;

        Ok(())
    }

    pub async fn send_swap_request(&mut self, swap_request: SwapRequest) -> Result<()> {
        let _ = self.send_swap_request.send(swap_request).await?;
        Ok(())
    }

    pub async fn send_message0(&mut self, msg: bob::Message0) -> Result<()> {
        let _ = self.send_msg0.send(msg).await?;
        Ok(())
    }

    pub async fn send_message1(&mut self, msg: bob::Message1) -> Result<()> {
        let _ = self.send_msg1.send(msg).await?;
        Ok(())
    }

    pub async fn send_message2(&mut self, msg: bob::Message2) -> Result<()> {
        let _ = self.send_msg2.send(msg).await?;
        Ok(())
    }

    pub async fn send_message3(&mut self, tx_redeem_encsig: EncryptedSignature) -> Result<()> {
        let _ = self.send_msg3.send(tx_redeem_encsig).await?;
        Ok(())
    }
}

#[allow(missing_debug_implementations)]
pub struct EventLoop {
    swarm: libp2p::Swarm<Behaviour>,
    alice_peer_id: PeerId,
    swap_response: Sender<SwapResponse>,
    msg0: Sender<alice::Message0>,
    msg1: Sender<alice::Message1>,
    msg2: Sender<alice::Message2>,
    conn_established: Sender<PeerId>,
    dial_alice: Receiver<()>,
    send_swap_request: Receiver<SwapRequest>,
    send_msg0: Receiver<bob::Message0>,
    send_msg1: Receiver<bob::Message1>,
    send_msg2: Receiver<bob::Message2>,
    send_msg3: Receiver<EncryptedSignature>,
}

impl EventLoop {
    pub fn new(
        transport: SwapTransport,
        behaviour: Behaviour,
        peer_id: PeerId,
        alice_peer_id: PeerId,
        alice_addr: Multiaddr,
    ) -> Result<(Self, EventLoopHandle)> {
        let mut swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, peer_id)
            .executor(Box::new(TokioExecutor {
                handle: tokio::runtime::Handle::current(),
            }))
            .build();

        swarm.add_address(alice_peer_id.clone(), alice_addr);

        let swap_response = Channels::new();
        let msg0 = Channels::new();
        let msg1 = Channels::new();
        let msg2 = Channels::new();
        let conn_established = Channels::new();
        let dial_alice = Channels::new();
        let send_swap_request = Channels::new();
        let send_msg0 = Channels::new();
        let send_msg1 = Channels::new();
        let send_msg2 = Channels::new();
        let send_msg3 = Channels::new();

        let event_loop = EventLoop {
            swarm,
            alice_peer_id,
            swap_response: swap_response.sender,
            msg0: msg0.sender,
            msg1: msg1.sender,
            msg2: msg2.sender,
            conn_established: conn_established.sender,
            dial_alice: dial_alice.receiver,
            send_swap_request: send_swap_request.receiver,
            send_msg0: send_msg0.receiver,
            send_msg1: send_msg1.receiver,
            send_msg2: send_msg2.receiver,
            send_msg3: send_msg3.receiver,
        };

        let handle = EventLoopHandle {
            swap_response: swap_response.receiver,
            msg0: msg0.receiver,
            msg1: msg1.receiver,
            msg2: msg2.receiver,
            conn_established: conn_established.receiver,
            dial_alice: dial_alice.sender,
            send_swap_request: send_swap_request.sender,
            send_msg0: send_msg0.sender,
            send_msg1: send_msg1.sender,
            send_msg2: send_msg2.sender,
            send_msg3: send_msg3.sender,
        };

        Ok((event_loop, handle))
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                swarm_event = self.swarm.next().fuse() => {
                    match swarm_event {
                        OutEvent::ConnectionEstablished(peer_id) => {
                            let _ = self.conn_established.send(peer_id).await;
                        }
                        OutEvent::SwapResponse(msg) => {
                            let _ = self.swap_response.send(msg).await;
                        },
                        OutEvent::Message0(msg) => {
                            let _ = self.msg0.send(*msg).await;
                        }
                        OutEvent::Message1(msg) => {
                            let _ = self.msg1.send(*msg).await;
                        }
                        OutEvent::Message2(msg) => {
                            let _ = self.msg2.send(msg).await;
                        }
                        OutEvent::Message3 => info!("Alice acknowledged message 3 received"),
                    }
                },
                option = self.dial_alice.next().fuse() => {
                    if option.is_some() {
                           let peer_id = self.alice_peer_id.clone();
                        if self.swarm.pt.is_connected(&peer_id) {
                            debug!("Already connected to Alice: {}", peer_id);
                            let _ = self.conn_established.send(peer_id).await;
                        } else {
                            info!("dialing alice: {}", peer_id);
                            if let Err(err) = libp2p::Swarm::dial(&mut self.swarm, &peer_id) {
                                error!("Could not dial alice: {}", err);
                                // TODO(Franck): If Dial fails then we should report it.
                            }

                        }
                    }
                },
                swap_request = self.send_swap_request.next().fuse() =>  {
                    if let Some(swap_request) = swap_request {
                        self.swarm.send_swap_request(self.alice_peer_id.clone(), swap_request);
                    }
                },

                msg0 = self.send_msg0.next().fuse() => {
                    if let Some(msg) = msg0 {
                        self.swarm.send_message0(self.alice_peer_id.clone(), msg);
                    }
                }

                msg1 = self.send_msg1.next().fuse() => {
                    if let Some(msg) = msg1 {
                        self.swarm.send_message1(self.alice_peer_id.clone(), msg);
                    }
                },
                msg2 = self.send_msg2.next().fuse() => {
                    if let Some(msg) = msg2 {
                        self.swarm.send_message2(self.alice_peer_id.clone(), msg);
                    }
                },
                msg3 = self.send_msg3.next().fuse() => {
                    if let Some(tx_redeem_encsig) = msg3 {
                        self.swarm.send_message3(self.alice_peer_id.clone(), tx_redeem_encsig);
                    }
                }
            }
        }
    }
}
