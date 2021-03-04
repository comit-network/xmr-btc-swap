use crate::bitcoin::EncryptedSignature;
use crate::network::quote::BidQuote;
use crate::network::{spot_price, transport, TokioExecutor};
use crate::protocol::alice::TransferProof;
use crate::protocol::bob::{Behaviour, OutEvent, State0, State2};
use crate::{bitcoin, monero};
use anyhow::{anyhow, bail, Context, Result};
use futures::FutureExt;
use libp2p::core::Multiaddr;
use libp2p::PeerId;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, trace};

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
    start_execution_setup: Sender<State0>,
    done_execution_setup: Receiver<Result<State2>>,
    recv_transfer_proof: Receiver<TransferProof>,
    conn_established: Receiver<PeerId>,
    dial_alice: Sender<()>,
    send_encrypted_signature: Sender<EncryptedSignature>,
    request_spot_price: Sender<spot_price::Request>,
    recv_spot_price: Receiver<spot_price::Response>,
    request_quote: Sender<()>,
    recv_quote: Receiver<BidQuote>,
}

impl EventLoopHandle {
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
        let _ = self.dial_alice.send(()).await?;

        self.conn_established
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive connection established from Alice"))?;

        Ok(())
    }

    pub async fn request_spot_price(&mut self, btc: bitcoin::Amount) -> Result<monero::Amount> {
        let _ = self
            .request_spot_price
            .send(spot_price::Request { btc })
            .await?;

        let response = self
            .recv_spot_price
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive spot price from Alice"))?;

        Ok(response.xmr)
    }

    pub async fn request_quote(&mut self) -> Result<BidQuote> {
        let _ = self.request_quote.send(()).await?;

        let quote = self
            .recv_quote
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive quote from Alice"))?;

        Ok(quote)
    }

    pub async fn send_encrypted_signature(
        &mut self,
        tx_redeem_encsig: EncryptedSignature,
    ) -> Result<()> {
        self.send_encrypted_signature.send(tx_redeem_encsig).await?;

        Ok(())
    }
}

#[allow(missing_debug_implementations)]
pub struct EventLoop {
    swarm: libp2p::Swarm<Behaviour>,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    alice_peer_id: PeerId,
    request_spot_price: Receiver<spot_price::Request>,
    recv_spot_price: Sender<spot_price::Response>,
    start_execution_setup: Receiver<State0>,
    done_execution_setup: Sender<Result<State2>>,
    recv_transfer_proof: Sender<TransferProof>,
    dial_alice: Receiver<()>,
    conn_established: Sender<PeerId>,
    send_encrypted_signature: Receiver<EncryptedSignature>,
    request_quote: Receiver<()>,
    recv_quote: Sender<BidQuote>,
}

impl EventLoop {
    pub fn new(
        identity: &libp2p::core::identity::Keypair,
        alice_peer_id: PeerId,
        alice_addr: Multiaddr,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
    ) -> Result<(Self, EventLoopHandle)> {
        let behaviour = Behaviour::default();
        let transport = transport::build(identity)?;

        let mut swarm = libp2p::swarm::SwarmBuilder::new(
            transport,
            behaviour,
            identity.public().into_peer_id(),
        )
        .executor(Box::new(TokioExecutor {
            handle: tokio::runtime::Handle::current(),
        }))
        .build();

        swarm.add_address(alice_peer_id, alice_addr);

        let start_execution_setup = Channels::new();
        let done_execution_setup = Channels::new();
        let recv_transfer_proof = Channels::new();
        let dial_alice = Channels::new();
        let conn_established = Channels::new();
        let send_encrypted_signature = Channels::new();
        let request_spot_price = Channels::new();
        let recv_spot_price = Channels::new();
        let request_quote = Channels::new();
        let recv_quote = Channels::new();

        let event_loop = EventLoop {
            swarm,
            alice_peer_id,
            bitcoin_wallet,
            start_execution_setup: start_execution_setup.receiver,
            done_execution_setup: done_execution_setup.sender,
            recv_transfer_proof: recv_transfer_proof.sender,
            conn_established: conn_established.sender,
            dial_alice: dial_alice.receiver,
            send_encrypted_signature: send_encrypted_signature.receiver,
            request_spot_price: request_spot_price.receiver,
            recv_spot_price: recv_spot_price.sender,
            request_quote: request_quote.receiver,
            recv_quote: recv_quote.sender,
        };

        let handle = EventLoopHandle {
            start_execution_setup: start_execution_setup.sender,
            done_execution_setup: done_execution_setup.receiver,
            recv_transfer_proof: recv_transfer_proof.receiver,
            conn_established: conn_established.receiver,
            dial_alice: dial_alice.sender,
            send_encrypted_signature: send_encrypted_signature.sender,
            request_spot_price: request_spot_price.sender,
            recv_spot_price: recv_spot_price.receiver,
            request_quote: request_quote.sender,
            recv_quote: recv_quote.receiver,
        };

        Ok((event_loop, handle))
    }

    pub async fn run(mut self) -> Result<Infallible> {
        loop {
            tokio::select! {
                swarm_event = self.swarm.next().fuse() => {
                    match swarm_event {
                        OutEvent::ConnectionEstablished(peer_id) => {
                            let _ = self.conn_established.send(peer_id).await;
                        }
                        OutEvent::SpotPriceReceived(msg) => {
                            let _ = self.recv_spot_price.send(msg).await;
                        },
                        OutEvent::QuoteReceived(msg) => {
                            let _ = self.recv_quote.send(msg).await;
                        },
                        OutEvent::ExecutionSetupDone(res) => {
                            let _ = self.done_execution_setup.send(res.map(|state|*state)).await;
                        }
                        OutEvent::TransferProof{ msg, channel }=> {
                            let _ = self.recv_transfer_proof.send(*msg).await;
                            // Send back empty response so that the request/response protocol completes.
                            if let Err(error) = self.swarm.transfer_proof.send_ack(channel) {
                                error!("Failed to send Transfer Proof ack: {:?}", error);
                            }
                        }
                        OutEvent::EncryptedSignatureAcknowledged => {
                            debug!("Alice acknowledged encrypted signature");
                        }
                        OutEvent::ResponseSent => {}
                        OutEvent::CommunicationError(err) => {
                            bail!("Communication error: {:#}", err)
                        }
                    }
                },
                option = self.dial_alice.recv().fuse() => {
                    if option.is_some() {
                           let peer_id = self.alice_peer_id;
                        if self.swarm.pt.is_connected(&peer_id) {
                            trace!("Already connected to Alice at {}", peer_id);
                            let _ = self.conn_established.send(peer_id).await;
                        } else {
                            debug!("Dialing alice at {}", peer_id);
                            libp2p::Swarm::dial(&mut self.swarm, &peer_id).context("Failed to dial alice")?;
                        }
                    }
                },
                spot_price_request = self.request_spot_price.recv().fuse() =>  {
                    if let Some(request) = spot_price_request {
                        self.swarm.request_spot_price(self.alice_peer_id, request);
                    }
                },
                quote_request = self.request_quote.recv().fuse() =>  {
                    if quote_request.is_some() {
                        self.swarm.request_quote(self.alice_peer_id);
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
