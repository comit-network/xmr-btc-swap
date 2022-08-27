use crate::bitcoin::EncryptedSignature;
use crate::cli::behaviour::{Behaviour, OutEvent};
use crate::monero;
use crate::network::encrypted_signature;
use crate::network::quote::BidQuote;
use crate::network::swap_setup::bob::NewSwap;
use crate::protocol::bob::State2;
use anyhow::{Context, Result};
use futures::future::{BoxFuture, OptionFuture};
use futures::{FutureExt, StreamExt};
use libp2p::request_response::{RequestId, ResponseChannel};
use libp2p::swarm::dial_opts::DialOpts;
use libp2p::swarm::SwarmEvent;
use libp2p::{PeerId, Swarm};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

#[allow(missing_debug_implementations)]
pub struct EventLoop {
    swap_id: Uuid,
    swarm: libp2p::Swarm<Behaviour>,
    alice_peer_id: PeerId,

    // these streams represents outgoing requests that we have to make
    quote_requests: bmrng::RequestReceiverStream<(), BidQuote>,
    encrypted_signatures: bmrng::RequestReceiverStream<EncryptedSignature, ()>,
    swap_setup_requests: bmrng::RequestReceiverStream<NewSwap, Result<State2>>,

    // these represents requests that are currently in-flight.
    // once we get a response to a matching [`RequestId`], we will use the responder to relay the
    // response.
    inflight_quote_requests: HashMap<RequestId, bmrng::Responder<BidQuote>>,
    inflight_encrypted_signature_requests: HashMap<RequestId, bmrng::Responder<()>>,
    inflight_swap_setup: Option<bmrng::Responder<Result<State2>>>,

    /// The sender we will use to relay incoming transfer proofs.
    transfer_proof: bmrng::RequestSender<monero::TransferProof, ()>,
    /// The future representing the successful handling of an incoming transfer
    /// proof.
    ///
    /// Once we've sent a transfer proof to the ongoing swap, this future waits
    /// until the swap took it "out" of the `EventLoopHandle`. As this future
    /// resolves, we use the `ResponseChannel` returned from it to send an ACK
    /// to Alice that we have successfully processed the transfer proof.
    pending_transfer_proof: OptionFuture<BoxFuture<'static, ResponseChannel<()>>>,
}

impl EventLoop {
    pub fn new(
        swap_id: Uuid,
        swarm: Swarm<Behaviour>,
        alice_peer_id: PeerId,
    ) -> Result<(Self, EventLoopHandle)> {
        let execution_setup = bmrng::channel_with_timeout(1, Duration::from_secs(60));
        let transfer_proof = bmrng::channel_with_timeout(1, Duration::from_secs(60));
        let encrypted_signature = bmrng::channel(1);
        let quote = bmrng::channel_with_timeout(1, Duration::from_secs(60));

        let event_loop = EventLoop {
            swap_id,
            swarm,
            alice_peer_id,
            swap_setup_requests: execution_setup.1.into(),
            transfer_proof: transfer_proof.0,
            encrypted_signatures: encrypted_signature.1.into(),
            quote_requests: quote.1.into(),
            inflight_quote_requests: HashMap::default(),
            inflight_swap_setup: None,
            inflight_encrypted_signature_requests: HashMap::default(),
            pending_transfer_proof: OptionFuture::from(None),
        };

        let handle = EventLoopHandle {
            swap_setup: execution_setup.0,
            transfer_proof: transfer_proof.1,
            encrypted_signature: encrypted_signature.0,
            quote: quote.0,
        };

        Ok((event_loop, handle))
    }

    pub async fn run(mut self) {
        match self.swarm.dial(DialOpts::from(self.alice_peer_id)) {
            Ok(()) => {}
            Err(e) => {
                tracing::error!("Failed to initiate dial to Alice: {}", e);
                return;
            }
        }

        loop {
            // Note: We are making very elaborate use of `select!` macro's feature here. Make sure to read the documentation thoroughly: https://docs.rs/tokio/1.4.0/tokio/macro.select.html
            tokio::select! {
                swarm_event = self.swarm.select_next_some() => {
                    match swarm_event {
                        SwarmEvent::Behaviour(OutEvent::QuoteReceived { id, response }) => {
                            if let Some(responder) = self.inflight_quote_requests.remove(&id) {
                                let _ = responder.respond(response);
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::SwapSetupCompleted(response)) => {
                            if let Some(responder) = self.inflight_swap_setup.take() {
                                let _ = responder.respond(*response);
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::TransferProofReceived { msg, channel, peer }) => {
                            let swap_id = msg.swap_id;

                            if peer != self.alice_peer_id {
                                tracing::warn!(
                                            %swap_id,
                                            "Ignoring malicious transfer proof from {}, expected to receive it from {}",
                                            peer,
                                            self.alice_peer_id);
                                        continue;
                            }

                            if swap_id != self.swap_id {

                                // TODO: Save unexpected transfer proofs in the database and check for messages in the database when handling swaps
                                tracing::warn!("Received unexpected transfer proof for swap {} while running swap {}. This transfer proof will be ignored", swap_id, self.swap_id);

                                // When receiving a transfer proof that is unexpected we still have to acknowledge that it was received
                                let _ = self.swarm.behaviour_mut().transfer_proof.send_response(channel, ());
                                continue;
                            }

                            let mut responder = match self.transfer_proof.send(msg.tx_lock_proof).await {
                                Ok(responder) => responder,
                                Err(e) => {
                                    tracing::warn!("Failed to pass on transfer proof: {:#}", e);
                                    continue;
                                }
                            };

                            self.pending_transfer_proof = OptionFuture::from(Some(async move {
                                let _ = responder.recv().await;

                                channel
                            }.boxed()));
                        }
                        SwarmEvent::Behaviour(OutEvent::EncryptedSignatureAcknowledged { id }) => {
                            if let Some(responder) = self.inflight_encrypted_signature_requests.remove(&id) {
                                let _ = responder.respond(());
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::AllRedialAttemptsExhausted { peer }) if peer == self.alice_peer_id => {
                            tracing::error!("Exhausted all re-dial attempts to Alice");
                            return;
                        }
                        SwarmEvent::Behaviour(OutEvent::Failure { peer, error }) => {
                            tracing::warn!(%peer, "Communication error: {:#}", error);
                            return;
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } if peer_id == self.alice_peer_id => {
                            tracing::info!("Connected to Alice at {}", endpoint.get_remote_address());
                        }
                        SwarmEvent::Dialing(peer_id) if peer_id == self.alice_peer_id => {
                            tracing::debug!("Dialling Alice at {}", peer_id);
                        }
                        SwarmEvent::ConnectionClosed { peer_id, endpoint, num_established, cause: Some(error) } if peer_id == self.alice_peer_id && num_established == 0 => {
                            tracing::warn!("Lost connection to Alice at {}, cause: {}", endpoint.get_remote_address(), error);
                        }
                        SwarmEvent::ConnectionClosed { peer_id, num_established, cause: None, .. } if peer_id == self.alice_peer_id && num_established == 0 => {
                            // no error means the disconnection was requested
                            tracing::info!("Successfully closed connection to Alice");
                            return;
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id,  error } if matches!(peer_id, Some(alice_peer_id) if alice_peer_id == self.alice_peer_id) => {
                            tracing::warn!( "Failed to dial Alice: {}", error);

                            if let Some(duration) = self.swarm.behaviour_mut().redial.until_next_redial() {
                                tracing::info!("Next redial attempt in {}s", duration.as_secs());
                            }

                        }
                        _ => {}
                    }
                },

                // Handle to-be-sent requests for all our network protocols.
                // Use `self.is_connected_to_alice` as a guard to "buffer" requests until we are connected.
                Some(((), responder)) = self.quote_requests.next().fuse(), if self.is_connected_to_alice() => {
                    let id = self.swarm.behaviour_mut().quote.send_request(&self.alice_peer_id, ());
                    self.inflight_quote_requests.insert(id, responder);
                },
                Some((swap, responder)) = self.swap_setup_requests.next().fuse(), if self.is_connected_to_alice() => {
                    self.swarm.behaviour_mut().swap_setup.start(self.alice_peer_id, swap).await;
                    self.inflight_swap_setup = Some(responder);
                },
                Some((tx_redeem_encsig, responder)) = self.encrypted_signatures.next().fuse(), if self.is_connected_to_alice() => {
                    let request = encrypted_signature::Request {
                        swap_id: self.swap_id,
                        tx_redeem_encsig
                    };

                    let id = self.swarm.behaviour_mut().encrypted_signature.send_request(&self.alice_peer_id, request);
                    self.inflight_encrypted_signature_requests.insert(id, responder);
                },

                Some(response_channel) = &mut self.pending_transfer_proof => {
                    let _ = self.swarm.behaviour_mut().transfer_proof.send_response(response_channel, ());

                    self.pending_transfer_proof = OptionFuture::from(None);
                }
            }
        }
    }

    fn is_connected_to_alice(&self) -> bool {
        self.swarm.is_connected(&self.alice_peer_id)
    }
}

#[derive(Debug)]
pub struct EventLoopHandle {
    swap_setup: bmrng::RequestSender<NewSwap, Result<State2>>,
    transfer_proof: bmrng::RequestReceiver<monero::TransferProof, ()>,
    encrypted_signature: bmrng::RequestSender<EncryptedSignature, ()>,
    quote: bmrng::RequestSender<(), BidQuote>,
}

impl EventLoopHandle {
    pub async fn setup_swap(&mut self, swap: NewSwap) -> Result<State2> {
        self.swap_setup.send_receive(swap).await?
    }

    pub async fn recv_transfer_proof(&mut self) -> Result<monero::TransferProof> {
        let (transfer_proof, responder) = self
            .transfer_proof
            .recv()
            .await
            .context("Failed to receive transfer proof")?;
        responder
            .respond(())
            .context("Failed to acknowledge receipt of transfer proof")?;

        Ok(transfer_proof)
    }

    pub async fn request_quote(&mut self) -> Result<BidQuote> {
        Ok(self.quote.send_receive(()).await?)
    }

    pub async fn send_encrypted_signature(
        &mut self,
        tx_redeem_encsig: EncryptedSignature,
    ) -> Result<(), bmrng::error::RequestError<EncryptedSignature>> {
        self.encrypted_signature
            .send_receive(tx_redeem_encsig)
            .await
    }
}
