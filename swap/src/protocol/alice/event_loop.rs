use crate::{
    execution_params::ExecutionParams,
    network::{transport::SwapTransport, TokioExecutor},
    protocol::{
        alice::{Behaviour, OutEvent, State0, State3, SwapResponse, TransferProof},
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
    done_execution_setup: Receiver<Result<State3>>,
    recv_encrypted_signature: Receiver<EncryptedSignature>,
    recv_swap_request: Receiver<(SwapRequest, ResponseChannel<SwapResponse>)>,
    conn_established: Receiver<PeerId>,
    send_swap_response: Sender<(ResponseChannel<SwapResponse>, SwapResponse)>,
    start_execution_setup: Sender<(PeerId, State0)>,
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

    pub async fn execution_setup(&mut self, bob_peer_id: PeerId, state0: State0) -> Result<State3> {
        let _ = self
            .start_execution_setup
            .send((bob_peer_id, state0))
            .await?;

        self.done_execution_setup
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to setup execution with Bob"))?
    }

    pub async fn recv_encrypted_signature(&mut self) -> Result<EncryptedSignature> {
        self.recv_encrypted_signature
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive Bitcoin encrypted signature from Bob"))
    }

    pub async fn recv_swap_request(
        &mut self,
    ) -> Result<(SwapRequest, ResponseChannel<SwapResponse>)> {
        self.recv_swap_request
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive amounts request from Bob"))
    }

    pub async fn send_swap_response(
        &mut self,
        channel: ResponseChannel<SwapResponse>,
        swap_response: SwapResponse,
    ) -> Result<()> {
        let _ = self
            .send_swap_response
            .send((channel, swap_response))
            .await?;
        Ok(())
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
    start_execution_setup: Receiver<(PeerId, State0)>,
    done_execution_setup: Sender<Result<State3>>,
    recv_encrypted_signature: Sender<EncryptedSignature>,
    recv_swap_request: Sender<(SwapRequest, ResponseChannel<SwapResponse>)>,
    conn_established: Sender<PeerId>,
    send_swap_response: Receiver<(ResponseChannel<SwapResponse>, SwapResponse)>,
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

        let start_execution_setup = Channels::new();
        let done_execution_setup = Channels::new();
        let recv_encrypted_signature = Channels::new();
        let request = Channels::new();
        let conn_established = Channels::new();
        let send_swap_response = Channels::new();
        let send_transfer_proof = Channels::new();
        let recv_transfer_proof_ack = Channels::new();

        let driver = EventLoop {
            swarm,
            start_execution_setup: start_execution_setup.receiver,
            done_execution_setup: done_execution_setup.sender,
            recv_encrypted_signature: recv_encrypted_signature.sender,
            recv_swap_request: request.sender,
            conn_established: conn_established.sender,
            send_swap_response: send_swap_response.receiver,
            send_transfer_proof: send_transfer_proof.receiver,
            recv_transfer_proof_ack: recv_transfer_proof_ack.sender,
        };

        let handle = EventLoopHandle {
            start_execution_setup: start_execution_setup.sender,
            done_execution_setup: done_execution_setup.receiver,
            recv_encrypted_signature: recv_encrypted_signature.receiver,
            recv_swap_request: request.receiver,
            conn_established: conn_established.receiver,
            send_swap_response: send_swap_response.sender,
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
                        OutEvent::SwapRequest { msg, channel } => {
                            let _ = self.recv_swap_request.send((msg, channel)).await;
                        }
                        OutEvent::ExecutionSetupDone(res) => {
                            let _ = self.done_execution_setup.send(res.map(|state|*state)).await;
                        }
                        OutEvent::TransferProofAcknowledged => {
                            trace!("Bob acknowledged transfer proof");
                            let _ = self.recv_transfer_proof_ack.send(()).await;
                        }
                        OutEvent::EncryptedSignature{ msg, channel } => {
                            let _ = self.recv_encrypted_signature.send(*msg).await;
                            // Send back empty response so that the request/response protocol completes.
                            if let Err(error) = self.swarm.encrypted_signature.send_ack(channel) {
                                error!("Failed to send Encrypted Signature ack: {:?}", error);
                            }
                        }
                        OutEvent::ResponseSent => {}
                        OutEvent::Failure(err) => {
                            error!("Communication error: {:#}", err);
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
                option = self.start_execution_setup.recv().fuse() => {
                    if let Some((bob_peer_id, state0)) = option {
                        let _ = self
                            .swarm
                            .start_execution_setup(bob_peer_id, state0);
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
}
