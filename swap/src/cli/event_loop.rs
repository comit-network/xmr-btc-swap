use crate::bitcoin::EncryptedSignature;
use crate::cli::behaviour::{Behaviour, OutEvent};
use crate::monero;
use crate::network::cooperative_xmr_redeem_after_punish::{self, Request, Response};
use crate::network::encrypted_signature;
use crate::network::quote::BidQuote;
use crate::network::swap_setup::bob::NewSwap;
use crate::protocol::bob::swap::has_already_processed_transfer_proof;
use crate::protocol::bob::{BobState, State2};
use crate::protocol::Database;
use anyhow::{anyhow, Context, Result};
use futures::future::{BoxFuture, OptionFuture};
use futures::{FutureExt, StreamExt};
use libp2p::request_response::{OutboundFailure, OutboundRequestId, ResponseChannel};
use libp2p::swarm::dial_opts::DialOpts;
use libp2p::swarm::SwarmEvent;
use libp2p::{PeerId, Swarm};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

static REQUEST_RESPONSE_PROTOCOL_TIMEOUT: Duration = Duration::from_secs(60);
static EXECUTION_SETUP_PROTOCOL_TIMEOUT: Duration = Duration::from_secs(120);

#[allow(missing_debug_implementations)]
pub struct EventLoop {
    swap_id: Uuid,
    swarm: libp2p::Swarm<Behaviour>,
    alice_peer_id: PeerId,
    db: Arc<dyn Database + Send + Sync>,

    // These streams represents outgoing requests that we have to make
    // These are essentially queues of requests that we will send to Alice once we are connected to her.
    quote_requests: bmrng::RequestReceiverStream<(), Result<BidQuote, OutboundFailure>>,
    cooperative_xmr_redeem_requests: bmrng::RequestReceiverStream<
        (),
        Result<cooperative_xmr_redeem_after_punish::Response, OutboundFailure>,
    >,
    encrypted_signatures_requests:
        bmrng::RequestReceiverStream<EncryptedSignature, Result<(), OutboundFailure>>,
    execution_setup_requests: bmrng::RequestReceiverStream<NewSwap, Result<State2>>,

    // These represents requests that are currently in-flight.
    // Meaning that we have sent them to Alice, but we have not yet received a response.
    // Once we get a response to a matching [`RequestId`], we will use the responder to relay the
    // response.
    inflight_quote_requests:
        HashMap<OutboundRequestId, bmrng::Responder<Result<BidQuote, OutboundFailure>>>,
    inflight_encrypted_signature_requests:
        HashMap<OutboundRequestId, bmrng::Responder<Result<(), OutboundFailure>>>,
    inflight_swap_setup: Option<bmrng::Responder<Result<State2>>>,
    inflight_cooperative_xmr_redeem_requests: HashMap<
        OutboundRequestId,
        bmrng::Responder<Result<cooperative_xmr_redeem_after_punish::Response, OutboundFailure>>,
    >,

    /// The sender we will use to relay incoming transfer proofs to the EventLoopHandle
    /// The corresponding receiver is stored in the EventLoopHandle
    transfer_proof_sender: bmrng::RequestSender<monero::TransferProof, ()>,

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
        db: Arc<dyn Database + Send + Sync>,
    ) -> Result<(Self, EventLoopHandle)> {
        // We still use a timeout here, because this protocol does not dial Alice itself
        // and we want to fail if we cannot reach Alice
        let (execution_setup_sender, execution_setup_receiver) =
            bmrng::channel_with_timeout(1, EXECUTION_SETUP_PROTOCOL_TIMEOUT);

        // It is okay to not have a timeout here, as timeouts are enforced by the request-response protocol
        let (transfer_proof_sender, transfer_proof_receiver) = bmrng::channel(1);
        let (encrypted_signature_sender, encrypted_signature_receiver) = bmrng::channel(1);
        let (quote_sender, quote_receiver) = bmrng::channel(1);
        let (cooperative_xmr_redeem_sender, cooperative_xmr_redeem_receiver) = bmrng::channel(1);

        let event_loop = EventLoop {
            swap_id,
            swarm,
            alice_peer_id,
            execution_setup_requests: execution_setup_receiver.into(),
            transfer_proof_sender,
            encrypted_signatures_requests: encrypted_signature_receiver.into(),
            cooperative_xmr_redeem_requests: cooperative_xmr_redeem_receiver.into(),
            quote_requests: quote_receiver.into(),
            inflight_quote_requests: HashMap::default(),
            inflight_swap_setup: None,
            inflight_encrypted_signature_requests: HashMap::default(),
            inflight_cooperative_xmr_redeem_requests: HashMap::default(),
            pending_transfer_proof: OptionFuture::from(None),
            db,
        };

        let handle = EventLoopHandle {
            execution_setup_sender,
            transfer_proof_receiver,
            encrypted_signature_sender,
            cooperative_xmr_redeem_sender,
            quote_sender,
        };

        Ok((event_loop, handle))
    }

    pub async fn run(mut self) {
        match self.swarm.dial(DialOpts::from(self.alice_peer_id)) {
            Ok(()) => {}
            Err(e) => {
                tracing::error!("Failed to initiate dial to Alice: {:?}", e);
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
                                let _ = responder.respond(Ok(response));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::SwapSetupCompleted(response)) => {
                            if let Some(responder) = self.inflight_swap_setup.take() {
                                let _ = responder.respond(*response);
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::TransferProofReceived { msg, channel, peer }) => {
                            let swap_id = msg.swap_id;

                            if swap_id == self.swap_id {
                                if peer != self.alice_peer_id {
                                    tracing::warn!(
                                                %swap_id,
                                                "Ignoring malicious transfer proof from {}, expected to receive it from {}",
                                                peer,
                                                self.alice_peer_id);
                                            continue;
                                }

                                // Immediately acknowledge if we've already processed this transfer proof
                                // This handles the case where Alice didn't receive our previous acknowledgment
                                // and is retrying sending the transfer proof
                                if let Ok(state) = self.db.get_state(swap_id).await {
                                    let state: BobState = state.try_into()
                                        .expect("Bobs database only contains Bob states");

                                    if has_already_processed_transfer_proof(&state) {
                                        tracing::warn!("Received transfer proof for swap {} but we are already in state {}. Acknowledging immediately. Alice most likely did not receive the acknowledgment when we sent it before", swap_id, state);

                                        // We set this to a future that will resolve immediately, and returns the channel
                                        // This will be resolved in the next iteration of the event loop, and a response will be sent to Alice
                                        self.pending_transfer_proof = OptionFuture::from(Some(async move {
                                            channel
                                        }.boxed()));

                                        continue;
                                    }
                                }

                                let mut responder = match self.transfer_proof_sender.send(msg.tx_lock_proof).await {
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
                            }else {
                                // Check if the transfer proof is sent from the correct peer and if we have a record of the swap
                                match self.db.get_peer_id(swap_id).await {
                                    // We have a record of the swap
                                    Ok(buffer_swap_alice_peer_id) => {
                                        if buffer_swap_alice_peer_id == self.alice_peer_id {
                                            // Save transfer proof in the database such that we can process it later when we resume the swap
                                            match self.db.insert_buffered_transfer_proof(swap_id, msg.tx_lock_proof).await {
                                                Ok(_) => {
                                                    tracing::info!("Received transfer proof for swap {} while running swap {}. Buffering this transfer proof in the database for later retrieval", swap_id, self.swap_id);
                                                    let _ = self.swarm.behaviour_mut().transfer_proof.send_response(channel, ());
                                                }
                                                Err(e) => {
                                                    tracing::error!("Failed to buffer transfer proof for swap {}: {:#}", swap_id, e);
                                                }
                                            };
                                        }else {
                                            tracing::warn!(
                                                %swap_id,
                                                "Ignoring malicious transfer proof from {}, expected to receive it from {}",
                                                self.swap_id,
                                                buffer_swap_alice_peer_id);
                                        }
                                    },
                                    // We do not have a record of the swap or an error occurred while retrieving the peer id of Alice
                                    Err(e) => {
                                        if let Some(sqlx::Error::RowNotFound) = e.downcast_ref::<sqlx::Error>() {
                                            tracing::warn!("Ignoring transfer proof for swap {} while running swap {}. We do not have a record of this swap", swap_id, self.swap_id);
                                        } else {
                                            tracing::error!("Ignoring transfer proof for swap {} while running swap {}. Failed to retrieve the peer id of Alice for the corresponding swap: {:#}", swap_id, self.swap_id, e);
                                        }
                                    }
                                }
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::EncryptedSignatureAcknowledged { id }) => {
                            if let Some(responder) = self.inflight_encrypted_signature_requests.remove(&id) {
                                let _ = responder.respond(Ok(()));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::CooperativeXmrRedeemFulfilled { id, swap_id, s_a, lock_transfer_proof }) => {
                            if let Some(responder) = self.inflight_cooperative_xmr_redeem_requests.remove(&id) {
                                let _ = responder.respond(Ok(Response::Fullfilled { s_a, swap_id, lock_transfer_proof }));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::CooperativeXmrRedeemRejected { id, swap_id, reason }) => {
                            if let Some(responder) = self.inflight_cooperative_xmr_redeem_requests.remove(&id) {
                                let _ = responder.respond(Ok(Response::Rejected { reason, swap_id }));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::Failure { peer, error }) => {
                            tracing::warn!(%peer, err = ?error, "Communication error");
                            return;
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } if peer_id == self.alice_peer_id => {
                            tracing::info!(peer_id = %endpoint.get_remote_address(), "Connected to Alice");
                        }
                        SwarmEvent::Dialing { peer_id: Some(alice_peer_id), connection_id } if alice_peer_id == self.alice_peer_id => {
                            tracing::debug!(%alice_peer_id, %connection_id, "Dialing Alice");
                        }
                        SwarmEvent::ConnectionClosed { peer_id, endpoint, num_established, cause: Some(error), connection_id } if peer_id == self.alice_peer_id && num_established == 0 => {
                            tracing::warn!(peer_id = %endpoint.get_remote_address(), cause = ?error, %connection_id, "Lost connection to Alice");

                            if let Some(duration) = self.swarm.behaviour_mut().redial.until_next_redial() {
                                tracing::info!(seconds_until_next_redial = %duration.as_secs(), "Waiting for next redial attempt");
                            }
                        }
                        SwarmEvent::ConnectionClosed { peer_id, num_established, cause: None, .. } if peer_id == self.alice_peer_id && num_established == 0 => {
                            // no error means the disconnection was requested
                            tracing::info!("Successfully closed connection to Alice");
                            return;
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id: Some(alice_peer_id),  error, connection_id } if alice_peer_id == self.alice_peer_id => {
                            tracing::warn!(%alice_peer_id, %connection_id, ?error, "Failed to connect to Alice");

                            if let Some(duration) = self.swarm.behaviour_mut().redial.until_next_redial() {
                                tracing::info!(seconds_until_next_redial = %duration.as_secs(), "Waiting for next redial attempt");
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::OutboundRequestResponseFailure {peer, error, request_id, protocol}) => {
                            tracing::error!(
                                %peer,
                                %request_id,
                                ?error,
                                %protocol,
                                "Failed to send request-response request to peer");

                            // If we fail to send a request-response request, we should notify the responder that the request failed
                            // We will remove the responder from the inflight requests and respond with an error

                            // Check for encrypted signature requests
                            if let Some(responder) = self.inflight_encrypted_signature_requests.remove(&request_id) {
                                let _ = responder.respond(Err(error));
                                continue;
                            }

                            // Check for quote requests
                            if let Some(responder) = self.inflight_quote_requests.remove(&request_id) {
                                let _ = responder.respond(Err(error));
                                continue;
                            }

                            // Check for cooperative xmr redeem requests
                            if let Some(responder) = self.inflight_cooperative_xmr_redeem_requests.remove(&request_id) {
                                let _ = responder.respond(Err(error));
                                continue;
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::InboundRequestResponseFailure {peer, error, request_id, protocol}) => {
                            tracing::error!(
                                %peer,
                                %request_id,
                                ?error,
                                %protocol,
                                "Failed to receive request-response request from peer");
                        }
                        _ => {}
                    }
                },

                // Handle to-be-sent outgoing requests for all our network protocols.
                Some(((), responder)) = self.quote_requests.next().fuse() => {
                    let id = self.swarm.behaviour_mut().quote.send_request(&self.alice_peer_id, ());
                    self.inflight_quote_requests.insert(id, responder);
                },
                Some((tx_redeem_encsig, responder)) = self.encrypted_signatures_requests.next().fuse() => {
                    let request = encrypted_signature::Request {
                        swap_id: self.swap_id,
                        tx_redeem_encsig
                    };

                    let id = self.swarm.behaviour_mut().encrypted_signature.send_request(&self.alice_peer_id, request);
                    self.inflight_encrypted_signature_requests.insert(id, responder);
                },
                Some((_, responder)) = self.cooperative_xmr_redeem_requests.next().fuse() => {
                    let id = self.swarm.behaviour_mut().cooperative_xmr_redeem.send_request(&self.alice_peer_id, Request {
                        swap_id: self.swap_id
                    });
                    self.inflight_cooperative_xmr_redeem_requests.insert(id, responder);
                },

                // We use `self.is_connected_to_alice` as a guard to "buffer" requests until we are connected.
                // because the protocol does not dial Alice itself
                // (unlike request-response above)
                Some((swap, responder)) = self.execution_setup_requests.next().fuse(), if self.is_connected_to_alice() => {
                    self.swarm.behaviour_mut().swap_setup.start(self.alice_peer_id, swap).await;
                    self.inflight_swap_setup = Some(responder);
                },

                // Send an acknowledgement to Alice once the EventLoopHandle has processed a received transfer proof
                // We use `self.is_connected_to_alice` as a guard to "buffer" requests until we are connected.
                //
                // Why do we do this here but not for the other request-response channels?
                // This is the only request, we don't have a retry mechanism for. We lazily send this.
                Some(response_channel) = &mut self.pending_transfer_proof, if self.is_connected_to_alice() => {
                    if self.swarm.behaviour_mut().transfer_proof.send_response(response_channel, ()).is_err() {
                        tracing::warn!("Failed to send acknowledgment to Alice that we have received the transfer proof");
                    } else {
                        tracing::info!("Sent acknowledgment to Alice that we have received the transfer proof");
                        self.pending_transfer_proof = OptionFuture::from(None);
                    }
                },
            }
        }
    }

    fn is_connected_to_alice(&self) -> bool {
        self.swarm.is_connected(&self.alice_peer_id)
    }
}

#[derive(Debug)]
pub struct EventLoopHandle {
    /// When a NewSwap object is sent into this channel, the EventLoop will:
    /// 1. Trigger the swap setup protocol with Alice to negotiate the swap parameters
    /// 2. Return the resulting State2 if successful
    /// 3. Return an anyhow error if the request fails
    execution_setup_sender: bmrng::RequestSender<NewSwap, Result<State2>>,

    /// Receiver for incoming Monero transfer proofs from Alice.
    /// When a proof is received, we process it and acknowledge receipt back to the EventLoop
    /// The EventLoop will then send an acknowledgment back to Alice over the network
    transfer_proof_receiver: bmrng::RequestReceiver<monero::TransferProof, ()>,

    /// When an encrypted signature is sent into this channel, the EventLoop will:
    /// 1. Send the encrypted signature to Alice over the network
    /// 2. Return Ok(()) if Alice acknowledges receipt, or
    /// 3. Return an OutboundFailure error if the request fails
    encrypted_signature_sender:
        bmrng::RequestSender<EncryptedSignature, Result<(), OutboundFailure>>,

    /// When a () is sent into this channel, the EventLoop will:
    /// 1. Request a price quote from Alice
    /// 2. Return the quote if successful
    /// 3. Return an OutboundFailure error if the request fails
    quote_sender: bmrng::RequestSender<(), Result<BidQuote, OutboundFailure>>,

    /// When a () is sent into this channel, the EventLoop will:
    /// 1. Request Alice's cooperation in redeeming the Monero
    /// 2. Return the a response object (Fullfilled or Rejected), if the network request is successful
    ///    The Fullfilled object contains the keys required to redeem the Monero
    /// 3. Return an OutboundFailure error if the network request fails
    cooperative_xmr_redeem_sender: bmrng::RequestSender<
        (),
        Result<cooperative_xmr_redeem_after_punish::Response, OutboundFailure>,
    >,
}

impl EventLoopHandle {
    fn create_retry_config(max_elapsed_time: Duration) -> backoff::ExponentialBackoff {
        backoff::ExponentialBackoffBuilder::new()
            .with_max_elapsed_time(max_elapsed_time.into())
            .with_max_interval(Duration::from_secs(5))
            .build()
    }

    pub async fn setup_swap(&mut self, swap: NewSwap) -> Result<State2> {
        tracing::debug!(swap = ?swap, "Sending swap setup request");

        let backoff = Self::create_retry_config(EXECUTION_SETUP_PROTOCOL_TIMEOUT);

        backoff::future::retry_notify(backoff, || async {
            match self.execution_setup_sender.send_receive(swap.clone()).await {
                Ok(Ok(state2)) => Ok(state2),
                // These are errors thrown by the swap_setup/bob behaviour
                Ok(Err(err)) => {
                    Err(backoff::Error::transient(err.context("A network error occurred while setting up the swap")))
                }
                // This will happen if we don't establish a connection to Alice within the timeout of the MPSC channel
                // The protocol does not dial Alice it self
                // This is handled by redial behaviour
                Err(bmrng::error::RequestError::RecvTimeoutError) => {
                    Err(backoff::Error::permanent(anyhow!("We failed to setup the swap in the allotted time by the event loop channel")))
                }
                Err(_) => {
                    unreachable!("We never drop the receiver of the execution setup channel, so this should never happen")
                }
            }
        }, |err, wait_time: Duration| {
            tracing::warn!(
                error = ?err,
                "Failed to setup swap. We will retry in {} seconds",
                wait_time.as_secs()
            )
        })
        .await
        .context("Failed to setup swap after retries")
    }

    pub async fn recv_transfer_proof(&mut self) -> Result<monero::TransferProof> {
        let (transfer_proof, responder) = self
            .transfer_proof_receiver
            .recv()
            .await
            .context("Failed to receive transfer proof")?;

        responder
            .respond(())
            .context("Failed to acknowledge receipt of transfer proof")?;

        Ok(transfer_proof)
    }

    pub async fn request_quote(&mut self) -> Result<BidQuote> {
        tracing::debug!("Requesting quote");

        let backoff = Self::create_retry_config(REQUEST_RESPONSE_PROTOCOL_TIMEOUT);

        backoff::future::retry_notify(backoff, || async {
            match self.quote_sender.send_receive(()).await {
                Ok(Ok(quote)) => Ok(quote),
                Ok(Err(err)) => {
                    Err(backoff::Error::transient(anyhow!(err).context("A network error occurred while requesting a quote")))
                }
                Err(_) => {
                    unreachable!("We initiate the quote channel without a timeout and store both the sender and receiver in the same struct, so this should never happen");
                }
            }
        }, |err, wait_time: Duration| {
            tracing::warn!(
                error = ?err,
                "Failed to request quote. We will retry in {} seconds",
                wait_time.as_secs()
            )
        })
        .await
        .context("Failed to request quote after retries")
    }

    pub async fn request_cooperative_xmr_redeem(&mut self) -> Result<Response> {
        tracing::debug!("Requesting cooperative XMR redeem");

        let backoff = Self::create_retry_config(REQUEST_RESPONSE_PROTOCOL_TIMEOUT);

        backoff::future::retry_notify(backoff, || async {
            match self.cooperative_xmr_redeem_sender.send_receive(()).await {
                Ok(Ok(response)) => Ok(response),
                Ok(Err(err)) => {
                    Err(backoff::Error::transient(anyhow!(err).context("A network error occurred while requesting cooperative XMR redeem")))
                }
                Err(_) => {
                    unreachable!("We initiate the cooperative xmr redeem channel without a timeout and store both the sender and receiver in the same struct, so this should never happen");
                }
            }
        }, |err, wait_time: Duration| {
            tracing::warn!(
                error = ?err,
                "Failed to request cooperative XMR redeem. We will retry in {} seconds",
                wait_time.as_secs()
            )
        })
        .await
        .context("Failed to request cooperative XMR redeem after retries")
    }

    pub async fn send_encrypted_signature(
        &mut self,
        tx_redeem_encsig: EncryptedSignature,
    ) -> Result<()> {
        tracing::debug!("Sending encrypted signature");

        // We will retry indefinitely until we succeed
        let backoff = backoff::ExponentialBackoffBuilder::new()
            .with_max_elapsed_time(None)
            .with_max_interval(REQUEST_RESPONSE_PROTOCOL_TIMEOUT)
            .build();

        backoff::future::retry_notify(backoff, || async {
            match self.encrypted_signature_sender.send_receive(tx_redeem_encsig.clone()).await {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(err)) => {
                    Err(backoff::Error::transient(anyhow!(err).context("A network error occurred while sending the encrypted signature")))
                }
                Err(_) => {
                    unreachable!("We initiate the encrypted signature channel without a timeout and store both the sender and receiver in the same struct, so this should never happen");
                }
            }
        }, |err, wait_time: Duration| {
            tracing::warn!(
                error = ?err,
                "Failed to send encrypted signature. We will retry in {} seconds",
                wait_time.as_secs()
            )
        })
        .await
        .context("Failed to send encrypted signature after retries")
    }
}
