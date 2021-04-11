use crate::bitcoin::EncryptedSignature;
use crate::network::quote::BidQuote;
use crate::network::{encrypted_signature, spot_price};
use crate::protocol::bob::{Behaviour, OutEvent, State0, State2};
use crate::{bitcoin, monero};
use anyhow::{Context, Result};
use bmrng::{RequestReceiverStream, RequestSender};
use futures::future::{BoxFuture, OptionFuture};
use futures::{FutureExt, StreamExt};
use libp2p::request_response::{RequestId, ResponseChannel};
use libp2p::swarm::SwarmEvent;
use libp2p::{PeerId, Swarm};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub fn new(
    swap_id: Uuid,
    mut swarm: Swarm<Behaviour>,
    alice_peer_id: PeerId,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
) -> Result<(impl Future<Output = ()>, EventLoopHandle)> {
    let (mut state, handle) = State::new();

    let event_loop = async move {
        let _ = Swarm::dial(&mut swarm, &alice_peer_id);

        loop {
            let is_connected_to_alice = Swarm::is_connected(&swarm, &alice_peer_id);

            // Note: We are making very elaborate use of `select!` macro's feature here. Make sure to read the documentation thoroughly: https://docs.rs/tokio/1.4.0/tokio/macro.select.html
            tokio::select! {
                swarm_event = swarm.next_event().fuse() => {
                    match swarm_event {
                        SwarmEvent::Behaviour(OutEvent::SpotPriceReceived { id, response }) => {
                            if let Some(responder) = state.inflight_spot_price_requests.remove(&id) {
                                let _ = responder.respond(response);
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::QuoteReceived { id, response }) => {
                            if let Some(responder) = state.inflight_quote_requests.remove(&id) {
                                let _ = responder.respond(response);
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::ExecutionSetupDone(response)) => {
                            if let Some(responder) = state.inflight_execution_setup.take() {
                                let _ = responder.respond(*response);
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::EncryptedSignatureAcknowledged { id }) => {
                            if let Some(responder) = state.inflight_encrypted_signature_requests.remove(&id) {
                                let _ = responder.respond(());
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::TransferProofReceived { msg, channel }) => {
                            if msg.swap_id != swap_id {

                                // TODO: Save unexpected transfer proofs in the database and check for messages in the database when handling swaps
                                tracing::warn!("Received unexpected transfer proof for swap {} while running swap {}. This transfer proof will be ignored.", msg.swap_id, swap_id);

                                // When receiving a transfer proof that is unexpected we still have to acknowledge that it was received
                                let _ = swarm.transfer_proof.send_response(channel, ());
                                continue;
                            }

                            let mut responder = match state.transfer_proof_sender.send(msg.tx_lock_proof).await {
                                Ok(responder) => responder,
                                Err(e) => {
                                    tracing::warn!("Failed to pass on transfer proof: {:#}", e);
                                    continue;
                                }
                            };

                            state.pending_transfer_proof = OptionFuture::from(Some(async move {
                                let _ = responder.recv().await;

                                channel
                            }.boxed()));
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } if peer_id == alice_peer_id => {
                            tracing::debug!("Connected to Alice at {}", endpoint.get_remote_address());
                        }
                        SwarmEvent::Dialing(peer_id) if peer_id == alice_peer_id => {
                            tracing::debug!("Dialling Alice at {}", peer_id);
                        }
                        SwarmEvent::UnreachableAddr { peer_id, address, attempts_remaining, error } if peer_id == alice_peer_id && attempts_remaining == 0 => {
                            tracing::warn!("Failed to dial Alice at {}: {}", address, error);
                        }
                        SwarmEvent::Behaviour(OutEvent::CommunicationError(error)) => {
                            tracing::warn!("Communication error: {:#}", error);
                            return;
                        }
                        SwarmEvent::ConnectionClosed { peer_id, endpoint, num_established, cause } if peer_id == alice_peer_id && num_established == 0 => {
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
                            match libp2p::Swarm::dial(&mut swarm, &alice_peer_id) {
                                Ok(()) => {},
                                Err(e) => {
                                    tracing::warn!("Failed to re-dial Alice: {}", e);
                                    return;
                                }
                            }
                        }
                        _ => {}
                    }
                },

                // Handle to-be-sent requests for all our network protocols.
                // Use `is_connected_to_alice` as a guard to "buffer" requests until we are connected.
                Some((request, responder)) = state.spot_price_requests.next().fuse(), if is_connected_to_alice => {
                    let id = swarm.spot_price.send_request(&alice_peer_id, request);
                    state.inflight_spot_price_requests.insert(id, responder);
                },
                Some(((), responder)) = state.quote_requests.next().fuse(), if is_connected_to_alice => {
                    let id = swarm.quote.send_request(&alice_peer_id, ());
                    state.inflight_quote_requests.insert(id, responder);
                },
                Some((request, responder)) = state.execution_setup_requests.next().fuse(), if is_connected_to_alice => {
                    swarm.execution_setup.run(alice_peer_id, request, bitcoin_wallet.clone());
                    state.inflight_execution_setup = Some(responder);
                },
                Some((tx_redeem_encsig, responder)) = state.encrypted_signature_requests.next().fuse(), if is_connected_to_alice => {
                    let request = encrypted_signature::Request {
                        swap_id,
                        tx_redeem_encsig
                    };
                    let id = swarm.encrypted_signature.send_request(&alice_peer_id, request);
                    state.inflight_encrypted_signature_requests.insert(id, responder);
                },

                Some(response_channel) = &mut state.pending_transfer_proof => {
                    let _ = swarm.transfer_proof.send_response(response_channel, ());

                    state.pending_transfer_proof = OptionFuture::from(None);
                }
            }
        }
    };

    Ok((event_loop, handle))
}

/// Holds the event-loop state.
///
/// All fields within this struct could also just be local variables in the
/// event loop. Bundling them up in a struct allows us to add documentation to
/// some of the fields which makes it clearer what they are used for.
struct State {
    // these represents requests that are currently in-flight.
    // once we get a response to a matching [`RequestId`], we will use the responder
    // to relay the response.
    inflight_spot_price_requests: HashMap<RequestId, bmrng::Responder<spot_price::Response>>,
    inflight_quote_requests: HashMap<RequestId, bmrng::Responder<BidQuote>>,
    inflight_encrypted_signature_requests: HashMap<RequestId, bmrng::Responder<()>>,
    inflight_execution_setup: Option<bmrng::Responder<Result<State2>>>,

    spot_price_requests: RequestReceiverStream<spot_price::Request, spot_price::Response>,
    quote_requests: RequestReceiverStream<(), BidQuote>,
    execution_setup_requests: RequestReceiverStream<State0, Result<State2>>,
    encrypted_signature_requests: RequestReceiverStream<bitcoin::EncryptedSignature, ()>,

    /// The future representing the successful handling of an incoming
    /// transfer proof.
    ///
    /// Once we've sent a transfer proof to the ongoing swap, this future
    /// waits until the swap took it "out" of the `EventLoopHandle`.
    /// As this future resolves, we use the `ResponseChannel`
    /// returned from it to send an ACK to Alice that we have
    /// successfully processed the transfer proof.
    pending_transfer_proof: OptionFuture<BoxFuture<'static, ResponseChannel<()>>>,

    transfer_proof_sender: RequestSender<monero::TransferProof, ()>,
}

impl State {
    fn new() -> (Self, EventLoopHandle) {
        let execution_setup = bmrng::channel_with_timeout(1, Duration::from_secs(30));
        let (transfer_proof_sender, transfer_proof_receiver) =
            bmrng::channel_with_timeout(1, Duration::from_secs(30));
        let encrypted_signature = bmrng::channel_with_timeout(1, Duration::from_secs(30));
        let spot_price = bmrng::channel_with_timeout(1, Duration::from_secs(30));
        let quote = bmrng::channel_with_timeout(1, Duration::from_secs(30));

        let state = State {
            inflight_spot_price_requests: Default::default(),
            inflight_quote_requests: Default::default(),
            inflight_encrypted_signature_requests: Default::default(),
            inflight_execution_setup: None,
            spot_price_requests: spot_price.1.into(),
            quote_requests: quote.1.into(),
            execution_setup_requests: execution_setup.1.into(),
            encrypted_signature_requests: encrypted_signature.1.into(),
            pending_transfer_proof: OptionFuture::from(None),
            transfer_proof_sender,
        };

        let handle = EventLoopHandle {
            execution_setup: execution_setup.0,
            transfer_proof: transfer_proof_receiver,
            encrypted_signature: encrypted_signature.0,
            spot_price: spot_price.0,
            quote: quote.0,
        };

        (state, handle)
    }
}

#[derive(Debug)]
pub struct EventLoopHandle {
    execution_setup: bmrng::RequestSender<State0, Result<State2>>,
    transfer_proof: bmrng::RequestReceiver<monero::TransferProof, ()>,
    encrypted_signature: bmrng::RequestSender<EncryptedSignature, ()>,
    spot_price: bmrng::RequestSender<spot_price::Request, spot_price::Response>,
    quote: bmrng::RequestSender<(), BidQuote>,
}

impl EventLoopHandle {
    pub async fn execution_setup(&mut self, state0: State0) -> Result<State2> {
        self.execution_setup.send_receive(state0).await?
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
            .send_receive(tx_redeem_encsig)
            .await?)
    }
}
