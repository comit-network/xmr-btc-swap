use crate::bitcoin::EncryptedSignature;
use crate::network::quote::BidQuote;
use crate::network::{encrypted_signature, spot_price, transfer_proof};
use crate::protocol::bob::{Behaviour, OutEvent, State0, State2};
use crate::{bitcoin, monero};
use anyhow::{Context, Result};
use futures::future::{BoxFuture, OptionFuture};
use futures::{FutureExt, StreamExt};
use libp2p::request_response::{RequestId, ResponseChannel};
use libp2p::swarm::SwarmEvent;
use libp2p::{PeerId, Swarm};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[allow(missing_debug_implementations)]
pub struct EventLoop {
    swarm: libp2p::Swarm<Behaviour>,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    alice_peer_id: PeerId,

    // these streams represents outgoing requests that we have to make
    quote_requests: bmrng::RequestReceiverStream<(), BidQuote>,
    spot_price_requests: bmrng::RequestReceiverStream<spot_price::Request, spot_price::Response>,
    encrypted_signature_requests: bmrng::RequestReceiverStream<encrypted_signature::Request, ()>,
    execution_setup_requests: bmrng::RequestReceiverStream<State0, Result<State2>>,

    // these represents requests that are currently in-flight.
    // once we get a response to a matching [`RequestId`], we will use the responder to relay the
    // response.
    inflight_spot_price_requests: HashMap<RequestId, bmrng::Responder<spot_price::Response>>,
    inflight_quote_requests: HashMap<RequestId, bmrng::Responder<BidQuote>>,
    inflight_encrypted_signature_requests: HashMap<RequestId, bmrng::Responder<()>>,
    inflight_execution_setup: Option<bmrng::Responder<Result<State2>>>,

    /// The sender we will use to relay incoming transfer proofs.
    transfer_proof: bmrng::RequestSender<transfer_proof::Request, ()>,
    /// The future representing the successful handling of an incoming transfer
    /// proof.
    ///
    /// Once we've sent a transfer proof to the ongoing swap, this future waits
    /// until the swap took it "out" of the `EventLoopHandle`. As this future
    /// resolves, we use the `ResponseChannel` returned from it to send an ACK
    /// to Alice that we have successfully processed the transfer proof.
    pending_transfer_proof: OptionFuture<BoxFuture<'static, ResponseChannel<()>>>,

    backoff_retry_dial_alice: Duration,
}

impl EventLoop {
    pub fn new(
        swarm: Swarm<Behaviour>,
        alice_peer_id: PeerId,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
    ) -> Result<(Self, EventLoopHandle)> {
        let execution_setup = bmrng::channel_with_timeout(1, Duration::from_secs(30));
        let transfer_proof = bmrng::channel_with_timeout(1, Duration::from_secs(30));
        let encrypted_signature = bmrng::channel_with_timeout(1, Duration::from_secs(30));
        let spot_price = bmrng::channel_with_timeout(1, Duration::from_secs(30));
        let quote = bmrng::channel_with_timeout(1, Duration::from_secs(30));

        let event_loop = EventLoop {
            swarm,
            alice_peer_id,
            bitcoin_wallet,
            execution_setup_requests: execution_setup.1.into(),
            transfer_proof: transfer_proof.0,
            encrypted_signature_requests: encrypted_signature.1.into(),
            spot_price_requests: spot_price.1.into(),
            quote_requests: quote.1.into(),
            inflight_spot_price_requests: HashMap::default(),
            inflight_quote_requests: HashMap::default(),
            inflight_execution_setup: None,
            inflight_encrypted_signature_requests: HashMap::default(),
            pending_transfer_proof: OptionFuture::from(None),
            backoff_retry_dial_alice: Duration::from_secs(5),
        };

        let handle = EventLoopHandle {
            execution_setup: execution_setup.0,
            transfer_proof: transfer_proof.1,
            encrypted_signature: encrypted_signature.0,
            spot_price: spot_price.0,
            quote: quote.0,
        };

        Ok((event_loop, handle))
    }

    pub async fn run(mut self) {
        let _ = Swarm::dial(&mut self.swarm, &self.alice_peer_id);

        loop {
            // Note: We are making very elaborate use of `select!` macro's feature here. Make sure to read the documentation thoroughly: https://docs.rs/tokio/1.4.0/tokio/macro.select.html
            tokio::select! {
                swarm_event = self.swarm.next_event().fuse() => {
                    match swarm_event {
                        SwarmEvent::Behaviour(OutEvent::SpotPriceReceived { id, response }) => {
                            if let Some(responder) = self.inflight_spot_price_requests.remove(&id) {
                                let _ = responder.respond(response);
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::QuoteReceived { id, response }) => {
                            if let Some(responder) = self.inflight_quote_requests.remove(&id) {
                                let _ = responder.respond(response);
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::ExecutionSetupDone(response)) => {
                            if let Some(responder) = self.inflight_execution_setup.take() {
                                let _ = responder.respond(*response);
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::TransferProofReceived { msg, channel }) => {
                            let mut responder = match self.transfer_proof.send(*msg).await {
                                Ok(responder) => responder,
                                Err(_) => {
                                    tracing::warn!("Failed to pass on transfer proof");
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
                            self.try_reconnect_to_alice().await;
                        }
                        SwarmEvent::UnreachableAddr { peer_id, address, attempts_remaining, error } if peer_id == self.alice_peer_id && attempts_remaining == 0 => {
                            tracing::warn!("Failed to dial Alice at {}: {}", address, error);

                            self.try_reconnect_to_alice().await;
                        }
                        _ => {}
                    }
                },

                // Handle to-be-sent requests for all our network protocols.
                // Use `self.is_connected_to_alice` as a guard to "buffer" requests until we are connected.
                Some((request, responder)) = self.spot_price_requests.next().fuse(), if self.is_connected_to_alice() => {
                    let id = self.swarm.spot_price.send_request(&self.alice_peer_id, request);
                    self.inflight_spot_price_requests.insert(id, responder);
                },
                Some(((), responder)) = self.quote_requests.next().fuse(), if self.is_connected_to_alice() => {
                    let id = self.swarm.quote.send_request(&self.alice_peer_id, ());
                    self.inflight_quote_requests.insert(id, responder);
                },
                Some((request, responder)) = self.execution_setup_requests.next().fuse(), if self.is_connected_to_alice() => {
                    self.swarm.execution_setup.run(self.alice_peer_id, request, self.bitcoin_wallet.clone());
                    self.inflight_execution_setup = Some(responder);
                },
                Some((request, responder)) = self.encrypted_signature_requests.next().fuse(), if self.is_connected_to_alice() => {
                    let id = self.swarm.encrypted_signature.send_request(&self.alice_peer_id, request);
                    self.inflight_encrypted_signature_requests.insert(id, responder);
                },

                Some(response_channel) = &mut self.pending_transfer_proof => {
                    let _ = self.swarm.transfer_proof.send_response(response_channel, ());

                    self.pending_transfer_proof = OptionFuture::from(None);
                }
            }
        }
    }

    async fn try_reconnect_to_alice(&mut self) {
        if self.backoff_retry_dial_alice <= Duration::from_secs(3600) {
            tokio::time::sleep(self.backoff_retry_dial_alice).await;
            match libp2p::Swarm::dial(&mut self.swarm, &self.alice_peer_id) {
                Ok(()) => {}
                Err(e) => {
                    tracing::warn!("Failed to re-dial Alice: {}", e);
                    return;
                }
            }

            self.backoff_retry_dial_alice *= 2;
        } else {
            tracing::error!("Connection to alice could not be re-established after {:?}. Aborting the connection retry.", self.backoff_retry_dial_alice)
        }
    }

    fn is_connected_to_alice(&self) -> bool {
        Swarm::is_connected(&self.swarm, &self.alice_peer_id)
    }
}

#[derive(Debug)]
pub struct EventLoopHandle {
    execution_setup: bmrng::RequestSender<State0, Result<State2>>,
    transfer_proof: bmrng::RequestReceiver<transfer_proof::Request, ()>,
    encrypted_signature: bmrng::RequestSender<encrypted_signature::Request, ()>,
    spot_price: bmrng::RequestSender<spot_price::Request, spot_price::Response>,
    quote: bmrng::RequestSender<(), BidQuote>,
}

impl EventLoopHandle {
    pub async fn execution_setup(&mut self, state0: State0) -> Result<State2> {
        self.execution_setup.send_receive(state0).await?
    }

    pub async fn recv_transfer_proof(&mut self) -> Result<transfer_proof::Request> {
        let (request, responder) = self
            .transfer_proof
            .recv()
            .await
            .context("Failed to receive transfer proof")?;
        responder
            .respond(())
            .context("Failed to acknowledge receipt of transfer proof")?;

        Ok(request)
    }

    pub async fn request_spot_price(&mut self, btc: bitcoin::Amount) -> Result<monero::Amount> {
        Ok(self
            .spot_price
            .send_receive(spot_price::Request { btc })
            .await?
            .xmr)
    }

    pub async fn request_quote(&mut self) -> Result<BidQuote> {
        Ok(self.quote.send_receive(()).await?)
    }

    pub async fn send_encrypted_signature(
        &mut self,
        tx_redeem_encsig: EncryptedSignature,
    ) -> Result<()> {
        Ok(self
            .encrypted_signature
            .send_receive(encrypted_signature::Request { tx_redeem_encsig })
            .await?)
    }
}
