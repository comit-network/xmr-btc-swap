use crate::{
    bitcoin,
    bitcoin::EncryptedSignature,
    network::{transport::SwapTransport, TokioExecutor},
    protocol::{
        alice,
        alice::{SwapResponse, TransferProof},
        bob::{self, Behaviour, OutEvent, State0, State2, SwapRequest},
    },
};
use anyhow::{anyhow, Result};
use futures::FutureExt;
use libp2p::{core::Multiaddr, PeerId};
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
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
    recv_swap_response: Receiver<SwapResponse>,
    recv_message0: Receiver<alice::Message0>,
    recv_message1: Receiver<alice::Message1>,
    start_execution_setup: Sender<State0>,
    done_execution_setup: Receiver<Result<State2>>,
    recv_transfer_proof: Receiver<TransferProof>,
    conn_established: Receiver<PeerId>,
    dial_alice: Sender<()>,
    send_swap_request: Sender<SwapRequest>,
    send_message0: Sender<bob::Message0>,
    send_message1: Sender<bob::Message1>,
    send_message2: Sender<bob::Message2>,
    send_encrypted_signature: Sender<EncryptedSignature>,
    recv_encrypted_signature_ack: Receiver<()>,
}

impl EventLoopHandle {
    pub async fn recv_swap_response(&mut self) -> Result<SwapResponse> {
        self.recv_swap_response
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive swap response from Alice"))
    }

    pub async fn recv_message0(&mut self) -> Result<alice::Message0> {
        self.recv_message0
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive message 0 from Alice"))
    }

    pub async fn recv_message1(&mut self) -> Result<alice::Message1> {
        self.recv_message1
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive message 1 from Alice"))
    }

    pub async fn execution_setup(&mut self, state0: State0) -> Result<State2> {
        let _ = self.start_execution_setup.send(state0).await?;

        self.done_execution_setup
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to setup execution with Alice"))?
    }

    pub async fn recv_transfer_proof(&mut self) -> Result<TransferProof> {
        self.recv_transfer_proof
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive transfer proof from Alice"))
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
        let _ = self.send_message0.send(msg).await?;
        Ok(())
    }

    pub async fn send_message1(&mut self, msg: bob::Message1) -> Result<()> {
        let _ = self.send_message1.send(msg).await?;
        Ok(())
    }

    pub async fn send_message2(&mut self, msg: bob::Message2) -> Result<()> {
        let _ = self.send_message2.send(msg).await?;
        Ok(())
    }

    pub async fn send_encrypted_signature(
        &mut self,
        tx_redeem_encsig: EncryptedSignature,
    ) -> Result<()> {
        self.send_encrypted_signature.send(tx_redeem_encsig).await?;

        self.recv_encrypted_signature_ack
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive encrypted signature ack from Alice"))?;
        Ok(())
    }
}

#[allow(missing_debug_implementations)]
pub struct EventLoop {
    swarm: libp2p::Swarm<Behaviour>,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    alice_peer_id: PeerId,
    recv_swap_response: Sender<SwapResponse>,
    recv_message0: Sender<alice::Message0>,
    recv_message1: Sender<alice::Message1>,
    start_execution_setup: Receiver<State0>,
    done_execution_setup: Sender<Result<State2>>,
    recv_transfer_proof: Sender<TransferProof>,
    dial_alice: Receiver<()>,
    conn_established: Sender<PeerId>,
    send_swap_request: Receiver<SwapRequest>,
    send_message0: Receiver<bob::Message0>,
    send_message1: Receiver<bob::Message1>,
    send_message2: Receiver<bob::Message2>,
    send_encrypted_signature: Receiver<EncryptedSignature>,
    recv_encrypted_signature_ack: Sender<()>,
}

impl EventLoop {
    pub fn new(
        transport: SwapTransport,
        behaviour: Behaviour,
        peer_id: PeerId,
        alice_peer_id: PeerId,
        alice_addr: Multiaddr,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
    ) -> Result<(Self, EventLoopHandle)> {
        let mut swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, peer_id)
            .executor(Box::new(TokioExecutor {
                handle: tokio::runtime::Handle::current(),
            }))
            .build();

        swarm.add_address(alice_peer_id, alice_addr);

        let swap_response = Channels::new();
        let recv_message0 = Channels::new();
        let recv_message1 = Channels::new();
        let start_execution_setup = Channels::new();
        let done_execution_setup = Channels::new();
        let recv_transfer_proof = Channels::new();
        let dial_alice = Channels::new();
        let conn_established = Channels::new();
        let send_swap_request = Channels::new();
        let send_message0 = Channels::new();
        let send_message1 = Channels::new();
        let send_message2 = Channels::new();
        let send_encrypted_signature = Channels::new();
        let recv_encrypted_signature_ack = Channels::new();

        let event_loop = EventLoop {
            swarm,
            alice_peer_id,
            bitcoin_wallet,
            recv_swap_response: swap_response.sender,
            recv_message0: recv_message0.sender,
            recv_message1: recv_message1.sender,
            start_execution_setup: start_execution_setup.receiver,
            done_execution_setup: done_execution_setup.sender,
            recv_transfer_proof: recv_transfer_proof.sender,
            conn_established: conn_established.sender,
            dial_alice: dial_alice.receiver,
            send_swap_request: send_swap_request.receiver,
            send_message0: send_message0.receiver,
            send_message1: send_message1.receiver,
            send_message2: send_message2.receiver,
            send_encrypted_signature: send_encrypted_signature.receiver,
            recv_encrypted_signature_ack: recv_encrypted_signature_ack.sender,
        };

        let handle = EventLoopHandle {
            recv_swap_response: swap_response.receiver,
            recv_message0: recv_message0.receiver,
            recv_message1: recv_message1.receiver,
            start_execution_setup: start_execution_setup.sender,
            done_execution_setup: done_execution_setup.receiver,
            recv_transfer_proof: recv_transfer_proof.receiver,
            conn_established: conn_established.receiver,
            dial_alice: dial_alice.sender,
            send_swap_request: send_swap_request.sender,
            send_message0: send_message0.sender,
            send_message1: send_message1.sender,
            send_message2: send_message2.sender,
            send_encrypted_signature: send_encrypted_signature.sender,
            recv_encrypted_signature_ack: recv_encrypted_signature_ack.receiver,
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
                            let _ = self.recv_swap_response.send(msg).await;
                        },
                        OutEvent::Message0(msg) => {
                            let _ = self.recv_message0.send(*msg).await;
                        }
                        OutEvent::Message1(msg) => {
                            let _ = self.recv_message1.send(*msg).await;
                        }
                        OutEvent::Message2 => info!("Alice acknowledged message 2 received"),
                        OutEvent::ExecutionSetupDone(res) => {
                            let _ = self.done_execution_setup.send(res.map(|state|*state)).await;
                        }
                        OutEvent::TransferProof(msg) => {
                            let _ = self.recv_transfer_proof.send(*msg).await;
                        }
                        OutEvent::EncryptedSignatureAcknowledged => {
                            debug!("Alice acknowledged encrypted signature");
                            let _ = self.recv_encrypted_signature_ack.send(()).await;
                        }
                    }
                },
                option = self.dial_alice.recv().fuse() => {
                    if option.is_some() {
                           let peer_id = self.alice_peer_id;
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
                swap_request = self.send_swap_request.recv().fuse() =>  {
                    if let Some(swap_request) = swap_request {
                        self.swarm.send_swap_request(self.alice_peer_id, swap_request);
                    }
                },

                msg0 = self.send_message0.recv().fuse() => {
                    if let Some(msg) = msg0 {
                        self.swarm.send_message0(self.alice_peer_id, msg);
                    }
                }

                msg1 = self.send_message1.recv().fuse() => {
                    if let Some(msg) = msg1 {
                        self.swarm.send_message1(self.alice_peer_id, msg);
                    }
                },
                msg2 = self.send_message2.recv().fuse() => {
                    if let Some(msg) = msg2 {
                        self.swarm.send_message2(self.alice_peer_id, msg);
                    }
                },
                option = self.start_execution_setup.recv().fuse() => {
                    if let Some(state0) = option {
                        let _ = self
                            .swarm
                            .start_execution_setup(self.alice_peer_id, state0, self.bitcoin_wallet.clone());
                    }
                },
                encrypted_signature = self.send_encrypted_signature.recv().fuse() => {
                    if let Some(tx_redeem_encsig) = encrypted_signature {
                        self.swarm.send_encrypted_signature(self.alice_peer_id, tx_redeem_encsig);
                    }
                }
            }
        }
    }
}
