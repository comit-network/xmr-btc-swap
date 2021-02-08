use crate::{
    execution_params::ExecutionParams,
    network::{transport, TokioExecutor},
    protocol::{
        alice::{
            behaviour::{Behaviour, OutEvent},
            State3, SwapResponse, TransferProof,
        },
        bob::{EncryptedSignature, SwapRequest},
    },
};
use anyhow::{anyhow, Context, Result};
use libp2p::{
    core::Multiaddr, futures::FutureExt, request_response::ResponseChannel, PeerId, Swarm,
};
use tokio::{
    sync::mpsc::{Receiver, Sender},
    time::timeout,
};
use tracing::{debug, error, trace};

#[allow(missing_debug_implementations)]
pub struct Channels<T> {
    sender: Sender<T>,
    receiver: Receiver<T>,
}

impl<T> Default for Channels<T> {
    fn default() -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(100);
        Channels { sender, receiver }
    }
}

#[derive(Debug)]
pub struct EventLoopHandle {
    recv_encrypted_signature: Receiver<EncryptedSignature>,
    send_transfer_proof: Sender<(PeerId, TransferProof)>,
    recv_transfer_proof_ack: Receiver<()>,
}

impl EventLoopHandle {
    pub async fn recv_encrypted_signature(&mut self) -> Result<EncryptedSignature> {
        self.recv_encrypted_signature
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive Bitcoin encrypted signature from Bob"))
    }

    pub async fn send_transfer_proof(
        &mut self,
        bob: PeerId,
        msg: TransferProof,
        execution_params: ExecutionParams,
    ) -> Result<()> {
        let _ = self.send_transfer_proof.send((bob, msg)).await?;

        // TODO: Re-evaluate if these acknowledges are necessary at all.
        // If we don't use a timeout here and Alice fails to dial Bob she will wait
        // indefinitely for this acknowledge.
        if timeout(
            execution_params.bob_time_to_act,
            self.recv_transfer_proof_ack.recv(),
        )
        .await
        .is_err()
        {
            error!("Failed to receive transfer proof ack from Bob")
        }

        Ok(())
    }
}

#[allow(missing_debug_implementations)]
pub struct EventLoop {
    swarm: libp2p::Swarm<Behaviour>,
    recv_encrypted_signature: Sender<EncryptedSignature>,
    send_transfer_proof: Receiver<(PeerId, TransferProof)>,
    recv_transfer_proof_ack: Sender<()>,
}

impl EventLoop {
    pub fn new(
        identity: libp2p::identity::Keypair,
        listen: Multiaddr,
        peer_id: PeerId,
    ) -> Result<(Self, EventLoopHandle)> {
        let behaviour = Behaviour::default();
        let transport = transport::build(identity)?;

        let mut swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, peer_id)
            .executor(Box::new(TokioExecutor {
                handle: tokio::runtime::Handle::current(),
            }))
            .build();

        Swarm::listen_on(&mut swarm, listen.clone())
            .with_context(|| format!("Address is not supported: {:#}", listen))?;

        let recv_encrypted_signature = Channels::default();
        let send_transfer_proof = Channels::default();
        let recv_transfer_proof_ack = Channels::default();

        let driver = EventLoop {
            swarm,
            recv_encrypted_signature: recv_encrypted_signature.sender,
            send_transfer_proof: send_transfer_proof.receiver,
            recv_transfer_proof_ack: recv_transfer_proof_ack.sender,
        };

        let handle = EventLoopHandle {
            recv_encrypted_signature: recv_encrypted_signature.receiver,
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
                            debug!("Connection Established with {}", alice);
                        }
                        OutEvent::SwapRequest { msg, channel } => {
                            let _ = self.handle_swap_request(msg, channel).await;
                        }
                        OutEvent::ExecutionSetupDone(state3) => {
                            let _ = self.handle_execution_setup_done(*state3).await;
                        }
                        OutEvent::TransferProofAcknowledged => {
                            trace!("Bob acknowledged transfer proof");
                            let _ = self.recv_transfer_proof_ack.send(()).await;
                        }
                        OutEvent::EncryptedSignature{ msg, channel } => {
                            let _ = self.recv_encrypted_signature.send(*msg).await;
                            // Send back empty response so that the request/response protocol completes.
                            if let Err(error) = self.swarm.send_encrypted_signature_ack(channel) {
                                error!("Failed to send Encrypted Signature ack: {:?}", error);
                            }
                        }
                        OutEvent::ResponseSent => {}
                        OutEvent::Failure(err) => {
                            error!("Communication error: {:#}", err);
                        }
                    }
                },
                transfer_proof = self.send_transfer_proof.recv().fuse() => {
                    if let Some((bob_peer_id, msg)) = transfer_proof  {
                      self.swarm.send_transfer_proof(bob_peer_id, msg);
                    }
                },
            }
        }
    }

    async fn handle_swap_request(
        &self,
        _msg: SwapRequest,
        _channel: ResponseChannel<SwapResponse>,
    ) {
    }

    async fn handle_execution_setup_done(&self, _state3: State3) {}
}
