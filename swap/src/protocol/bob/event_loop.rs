use crate::bitcoin::EncryptedSignature;
use crate::network::quote::BidQuote;
use crate::network::{encrypted_signature, spot_price, transfer_proof};
use crate::protocol::bob::{Behaviour, OutEvent, State0, State2};
use crate::{bitcoin, monero};
use anyhow::{anyhow, Result};
use futures::FutureExt;
use libp2p::swarm::SwarmEvent;
use libp2p::{PeerId, Swarm};
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error};

#[allow(missing_debug_implementations)]
pub struct EventLoop {
    swarm: libp2p::Swarm<Behaviour>,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    alice_peer_id: PeerId,
    request_spot_price: Receiver<spot_price::Request>,
    recv_spot_price: Sender<spot_price::Response>,
    start_execution_setup: Receiver<State0>,
    done_execution_setup: Sender<Result<State2>>,
    recv_transfer_proof: Sender<transfer_proof::Request>,
    send_encrypted_signature: Receiver<encrypted_signature::Request>,
    request_quote: Receiver<()>,
    recv_quote: Sender<BidQuote>,
}

impl EventLoop {
    pub fn new(
        swarm: Swarm<Behaviour>,
        alice_peer_id: PeerId,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
    ) -> Result<(Self, EventLoopHandle)> {
        let start_execution_setup = Channels::new();
        let done_execution_setup = Channels::new();
        let recv_transfer_proof = Channels::new();
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
            send_encrypted_signature: send_encrypted_signature.sender,
            request_spot_price: request_spot_price.sender,
            recv_spot_price: recv_spot_price.receiver,
            request_quote: request_quote.sender,
            recv_quote: recv_quote.receiver,
        };

        Ok((event_loop, handle))
    }

    pub async fn run(mut self) {
        let _ = Swarm::dial(&mut self.swarm, &self.alice_peer_id);

        loop {
            tokio::select! {
                swarm_event = self.swarm.next_event().fuse() => {
                    match swarm_event {
                        SwarmEvent::Behaviour(OutEvent::SpotPriceReceived(msg)) => {
                            let _ = self.recv_spot_price.send(msg).await;
                        }
                        SwarmEvent::Behaviour(OutEvent::QuoteReceived(msg)) => {
                            let _ = self.recv_quote.send(msg).await;
                        }
                        SwarmEvent::Behaviour(OutEvent::ExecutionSetupDone(res)) => {
                            let _ = self.done_execution_setup.send(res.map(|state|*state)).await;
                        }
                        SwarmEvent::Behaviour(OutEvent::TransferProofReceived{ msg, channel }) => {
                            let _ = self.recv_transfer_proof.send(*msg).await;
                            // Send back empty response so that the request/response protocol completes.
                            if let Err(error) = self.swarm.transfer_proof.send_response(channel, ()) {
                                error!("Failed to send Transfer Proof ack: {:?}", error);
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::EncryptedSignatureAcknowledged) => {
                            debug!("Alice acknowledged encrypted signature");
                        }
                        SwarmEvent::Behaviour(OutEvent::ResponseSent) => {

                        }
                        SwarmEvent::Behaviour(OutEvent::CommunicationError(error)) => {
                            tracing::warn!("Communication error: {:#}", error);
                            return;
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } if peer_id == self.alice_peer_id => {
                            tracing::debug!("Connected to Alice at {}", endpoint.get_remote_address());
                        }
                        SwarmEvent::Dialing(peer_id) if peer_id == self.alice_peer_id => {
                            tracing::debug!("Dialling Alice at {}", peer_id);
                        }
                        SwarmEvent::ConnectionClosed { peer_id, endpoint, num_established, cause } if peer_id == self.alice_peer_id && num_established == 0 => {
                            match cause {
                                Some(error) => {
                                    tracing::warn!("Lost connection to Alice at {}, cause: {}", endpoint.get_remote_address(), error);
                                },
                                None => {
                                    // no error means the disconnection was requested
                                    tracing::info!("Successfully closed connection to Alice");
                                    return;
                                }
                            }
                            match libp2p::Swarm::dial(&mut self.swarm, &self.alice_peer_id) {
                                Ok(()) => {},
                                Err(e) => {
                                    tracing::warn!("Failed to re-dial Alice: {}", e);
                                    return;
                                }
                            }
                        }
                        SwarmEvent::UnreachableAddr { peer_id, address, attempts_remaining, error } if peer_id == self.alice_peer_id && attempts_remaining == 0 => {
                            tracing::warn!("Failed to dial Alice at {}: {}", address, error);
                        }
                        _ => {}
                    }
                },
                spot_price_request = self.request_spot_price.recv().fuse() => {
                    if let Some(request) = spot_price_request {
                        self.swarm.spot_price.send_request(&self.alice_peer_id, request);
                    }
                },
                quote_request = self.request_quote.recv().fuse() =>  {
                    if quote_request.is_some() {
                        self.swarm.quote.send_request(&self.alice_peer_id, ());
                    }
                },
                option = self.start_execution_setup.recv().fuse() => {
                    if let Some(state0) = option {
                        let _ = self
                            .swarm
                            .execution_setup.run(self.alice_peer_id, state0, self.bitcoin_wallet.clone());
                    }
                },
                encrypted_signature = self.send_encrypted_signature.recv().fuse() => {
                    if let Some(tx_redeem_encsig) = encrypted_signature {
                        self.swarm.encrypted_signature.send_request(&self.alice_peer_id, tx_redeem_encsig);
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct EventLoopHandle {
    start_execution_setup: Sender<State0>,
    done_execution_setup: Receiver<Result<State2>>,
    recv_transfer_proof: Receiver<transfer_proof::Request>,
    send_encrypted_signature: Sender<encrypted_signature::Request>,
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

    pub async fn recv_transfer_proof(&mut self) -> Result<transfer_proof::Request> {
        self.recv_transfer_proof
            .recv()
            .await
            .ok_or_else(|| anyhow!("Failed to receive transfer proof from Alice"))
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
        self.send_encrypted_signature
            .send(encrypted_signature::Request { tx_redeem_encsig })
            .await?;

        Ok(())
    }
}

#[derive(Debug)]
struct Channels<T> {
    sender: Sender<T>,
    receiver: Receiver<T>,
}

impl<T> Channels<T> {
    fn new() -> Channels<T> {
        let (sender, receiver) = tokio::sync::mpsc::channel(100);
        Channels { sender, receiver }
    }
}

impl<T> Default for Channels<T> {
    fn default() -> Self {
        Self::new()
    }
}
