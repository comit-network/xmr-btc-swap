use crate::{
    bob::{Behaviour, OutEvent},
    network::{transport::SwapTransport, TokioExecutor},
};
use anyhow::Result;
use futures::FutureExt;
use libp2p::{core::Multiaddr, PeerId};
use tokio::{
    stream::StreamExt,
    sync::mpsc::{Receiver, Sender},
};
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

pub struct SwarmDriverHandle {
    pub msg0: Receiver<alice::Message0>,
    pub msg1: Receiver<alice::Message1>,
    pub msg2: Receiver<alice::Message2>,
    pub request_amounts: Sender<(PeerId, ::bitcoin::Amount)>,
    pub conn_established: Receiver<PeerId>,
    pub dial_alice: Sender<Multiaddr>,
    pub send_msg0: Sender<(PeerId, bob::Message0)>,
    pub send_msg1: Sender<(PeerId, bob::Message1)>,
    pub send_msg2: Sender<(PeerId, bob::Message2)>,
    pub send_msg3: Sender<(PeerId, EncryptedSignature)>,
}

impl SwarmDriverHandle {
    pub async fn recv_conn_established(&mut self) -> Result<PeerId> {
        self.conn_established
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive connection established from Bob"))
    }

    pub async fn recv_message0(&mut self) -> Result<alice::Message0> {
        self.msg0
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive message 0 from Bob"))
    }

    pub async fn recv_message1(&mut self) -> Result<alice::Message1> {
        self.msg1
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed to receive message 1 from Bob"))
    }

    pub async fn recv_message2(&mut self) -> Result<alice::Message2> {
        self.msg2
            .recv()
            .await
            .ok_or_else(|| anyhow::Error::msg("Failed o receive message 2 from Bob"))
    }

    pub async fn dial_alice(&mut self, addr: Multiaddr) -> Result<()> {
        info!("sending msg to ourselves to dial alice: {}", addr);
        let _ = self.dial_alice.send(addr).await?;
        Ok(())
    }

    pub async fn request_amounts(
        &mut self,
        peer_id: PeerId,
        btc_amount: ::bitcoin::Amount,
    ) -> Result<()> {
        let _ = self.request_amounts.send((peer_id, btc_amount)).await?;
        Ok(())
    }

    pub async fn send_message0(&mut self, peer_id: PeerId, msg: bob::Message0) -> Result<()> {
        let _ = self.send_msg0.send((peer_id, msg)).await?;
        Ok(())
    }

    pub async fn send_message1(&mut self, peer_id: PeerId, msg: bob::Message1) -> Result<()> {
        let _ = self.send_msg1.send((peer_id, msg)).await?;
        Ok(())
    }

    pub async fn send_message2(&mut self, peer_id: PeerId, msg: bob::Message2) -> Result<()> {
        let _ = self.send_msg2.send((peer_id, msg)).await?;
        Ok(())
    }

    pub async fn send_message3(
        &mut self,
        peer_id: PeerId,
        tx_redeem_encsig: EncryptedSignature,
    ) -> Result<()> {
        let _ = self.send_msg3.send((peer_id, tx_redeem_encsig)).await?;
        Ok(())
    }
}

pub struct SwarmDriver {
    pub swarm: libp2p::Swarm<Behaviour>,
    pub msg0: Sender<alice::Message0>,
    pub msg1: Sender<alice::Message1>,
    pub msg2: Sender<alice::Message2>,
    pub conn_established: Sender<PeerId>,
    pub request_amounts: Receiver<(PeerId, ::bitcoin::Amount)>,
    pub dial_alice: Receiver<Multiaddr>,
    pub send_msg0: Receiver<(PeerId, bob::Message0)>,
    pub send_msg1: Receiver<(PeerId, bob::Message1)>,
    pub send_msg2: Receiver<(PeerId, bob::Message2)>,
    pub send_msg3: Receiver<(PeerId, EncryptedSignature)>,
}

impl SwarmDriver {
    pub fn new(
        transport: SwapTransport,
        behaviour: Behaviour,
    ) -> Result<(Self, SwarmDriverHandle)> {
        let local_peer_id = behaviour.peer_id();

        let swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, local_peer_id)
            .executor(Box::new(TokioExecutor {
                handle: tokio::runtime::Handle::current(),
            }))
            .build();

        let amounts = Channels::new();
        let msg0 = Channels::new();
        let msg1 = Channels::new();
        let msg2 = Channels::new();
        let conn_established = Channels::new();
        let dial_alice = Channels::new();
        let send_msg0 = Channels::new();
        let send_msg1 = Channels::new();
        let send_msg2 = Channels::new();
        let send_msg3 = Channels::new();

        let driver = SwarmDriver {
            swarm,
            request_amounts: amounts.receiver,
            msg0: msg0.sender,
            msg1: msg1.sender,
            msg2: msg2.sender,
            conn_established: conn_established.sender,
            dial_alice: dial_alice.receiver,
            send_msg0: send_msg0.receiver,
            send_msg1: send_msg1.receiver,
            send_msg2: send_msg2.receiver,
            send_msg3: send_msg3.receiver,
        };

        let handle = SwarmDriverHandle {
            request_amounts: amounts.sender,
            msg0: msg0.receiver,
            msg1: msg1.receiver,
            msg2: msg2.receiver,
            conn_established: conn_established.receiver,
            dial_alice: dial_alice.sender,
            send_msg0: send_msg0.sender,
            send_msg1: send_msg1.sender,
            send_msg2: send_msg2.sender,
            send_msg3: send_msg3.sender,
        };

        Ok((driver, handle))
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                swarm_event = self.swarm.next().fuse() => {
                    match swarm_event {
                        OutEvent::ConnectionEstablished(alice) => {
                            let _ = self.conn_established.send(alice).await;
                        }
                        OutEvent::Amounts(_amounts) => info!("Amounts received from Alice"),
                        OutEvent::Message0(msg) => {
                            let _ = self.msg0.send(msg).await;
                        }
                        OutEvent::Message1(msg) => {
                            let _ = self.msg1.send(msg).await;
                        }
                        OutEvent::Message2(msg) => {
                            let _ = self.msg2.send(msg).await;
                        }
                        OutEvent::Message3 => info!("Alice acknowledged message 3 received"),
                    }
                },
                addr = self.dial_alice.next().fuse() => {
                    if let Some(addr) = addr {
                        info!("dialing alice: {}", addr);
                        libp2p::Swarm::dial_addr(&mut self.swarm, addr).expect("Could not dial alice");
                    }
                },
                amounts = self.request_amounts.next().fuse() =>  {
                    if let Some((peer_id, btc_amount)) = amounts {
                        self.swarm.request_amounts(peer_id, btc_amount.as_sat());
                    }
                },

                msg0 = self.send_msg0.next().fuse() => {
                    if let Some((peer_id, msg)) = msg0 {
                        self.swarm.send_message0(peer_id, msg);
                    }
                }

                msg1 = self.send_msg1.next().fuse() => {
                    if let Some((peer_id, msg)) = msg1 {
                        self.swarm.send_message1(peer_id, msg);
                    }
                },
                msg2 = self.send_msg2.next().fuse() => {
                    if let Some((peer_id, msg)) = msg2 {
                        self.swarm.send_message2(peer_id, msg);
                    }
                },
                msg3 = self.send_msg3.next().fuse() => {
                    if let Some((peer_id, tx_redeem_encsig)) = msg3 {
                        self.swarm.send_message3(peer_id, tx_redeem_encsig);
                    }
                }
            }
        }
    }
}
