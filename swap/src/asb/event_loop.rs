use crate::asb::{Behaviour, OutEvent, Rate};
use crate::monero::Amount;
use crate::network::cooperative_xmr_redeem_after_punish::CooperativeXmrRedeemRejectReason;
use crate::network::cooperative_xmr_redeem_after_punish::Response::{Fullfilled, Rejected};
use crate::network::quote::BidQuote;
use crate::network::swap_setup::alice::WalletSnapshot;
use crate::network::transfer_proof;
use crate::protocol::alice::swap::has_already_processed_enc_sig;
use crate::protocol::alice::{AliceState, State3, Swap};
use crate::protocol::{Database, State};
use crate::{bitcoin, env, kraken, monero};
use anyhow::{anyhow, Context, Result};
use futures::future;
use futures::future::{BoxFuture, FutureExt};
use futures::stream::{FuturesUnordered, StreamExt};
use libp2p::request_response::{OutboundFailure, OutboundRequestId, ResponseChannel};
use libp2p::swarm::SwarmEvent;
use libp2p::{PeerId, Swarm};
use moka::future::Cache;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::convert::{Infallible, TryInto};
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

/// Simple unit struct to serve as a key for the quote cache.
/// Since all quotes are the same type currently, we can use a simple key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct QuoteCacheKey;

#[allow(missing_debug_implementations)]
pub struct EventLoop<LR>
where
    LR: LatestRate + Send + 'static + Debug + Clone,
{
    swarm: libp2p::Swarm<Behaviour<LR>>,
    env_config: env::Config,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    db: Arc<dyn Database + Send + Sync>,
    latest_rate: LR,
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    external_redeem_address: Option<bitcoin::Address>,
    
    /// Cache for quotes
    quote_cache: Cache<QuoteCacheKey, Result<Arc<BidQuote>, Arc<anyhow::Error>>>,

    swap_sender: mpsc::Sender<Swap>,

    /// Stores where to send [`EncryptedSignature`]s to
    /// The corresponding receiver for this channel is stored in the EventLoopHandle
    /// that is responsible for the swap.
    ///
    /// Once a [`EncryptedSignature`] has been sent to the EventLoopHandle,
    /// the sender is removed from this map.
    recv_encrypted_signature: HashMap<Uuid, bmrng::RequestSender<bitcoin::EncryptedSignature, ()>>,

    /// Once we receive an [`EncryptedSignature`] from Bob, we forward it to the EventLoopHandle.
    /// Once the EventLoopHandle acknowledges the receipt of the [`EncryptedSignature`], we need to confirm this to Bob.
    /// When the EventLoopHandle acknowledges the receipt, a future in this collection resolves and returns the libp2p channel
    /// which we use to confirm to Bob that we have received the [`EncryptedSignature`].
    ///
    /// Flow:
    /// 1. When signature forwarded via recv_encrypted_signature sender
    /// 2. New future pushed here to await EventLoopHandle's acknowledgement
    /// 3. When future completes, the EventLoop uses the ResponseChannel to send an acknowledgment to Bob
    /// 4. Future is removed from this collection
    inflight_encrypted_signatures: FuturesUnordered<BoxFuture<'static, ResponseChannel<()>>>,

    /// Channel for sending transfer proofs to Bobs. The sender is shared with every EventLoopHandle.
    /// The receiver is polled by the event loop to send transfer proofs over the network to Bob.
    ///
    /// Flow:
    /// 1. EventLoopHandle sends (PeerId, Request, Responder) through sender
    /// 2. Event loop receives and attempts to send to peer
    /// 3. Result (Ok or network failure) is sent back to EventLoopHandle
    #[allow(clippy::type_complexity)]
    outgoing_transfer_proofs_requests: tokio::sync::mpsc::UnboundedReceiver<(
        PeerId,
        transfer_proof::Request,
        oneshot::Sender<Result<(), OutboundFailure>>,
    )>,
    #[allow(clippy::type_complexity)]
    outgoing_transfer_proofs_sender: tokio::sync::mpsc::UnboundedSender<(
        PeerId,
        transfer_proof::Request,
        oneshot::Sender<Result<(), OutboundFailure>>,
    )>,

    /// Temporarily stores transfer proof requests for peers that are currently disconnected.
    ///
    /// When a transfer proof cannot be sent because there's no connection to the peer:
    /// 1. It is moved from [`outgoing_transfer_proofs_requests`] to this buffer
    /// 2. Once a connection is established with the peer, the proof is send back into the [`outgoing_transfer_proofs_sender`]
    /// 3. The buffered request is then removed from this collection
    #[allow(clippy::type_complexity)]
    buffered_transfer_proofs: HashMap<
        PeerId,
        Vec<(
            transfer_proof::Request,
            oneshot::Sender<Result<(), OutboundFailure>>,
        )>,
    >,

    /// Tracks [`transfer_proof::Request`]s which are currently inflight and awaiting an acknowledgement from Bob
    ///
    /// When a transfer proof is sent to Bob:
    /// 1. A unique request ID is generated by libp2p
    /// 2. The response channel is stored in this map with the request ID as key
    /// 3. When Bob acknowledges the proof, we use the stored channel to notify the EventLoopHandle
    /// 4. The entry is then removed from this map
    inflight_transfer_proofs:
        HashMap<OutboundRequestId, oneshot::Sender<Result<(), OutboundFailure>>>,
}

impl<LR> EventLoop<LR>
where
    LR: LatestRate + Send + 'static + Debug + Clone,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        swarm: Swarm<Behaviour<LR>>,
        env_config: env::Config,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        db: Arc<dyn Database + Send + Sync>,
        latest_rate: LR,
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
        external_redeem_address: Option<bitcoin::Address>,
    ) -> Result<(Self, mpsc::Receiver<Swap>)> {
        let swap_channel = MpscChannels::default();
        let (outgoing_transfer_proofs_sender, outgoing_transfer_proofs_requests) =
            tokio::sync::mpsc::unbounded_channel();
            
        // --- Initialize moka::future::Cache ---
        let quote_cache = Cache::builder()
            .time_to_live(Duration::from_secs(120)) // 2 minutes TTL
            .build(); // Builds a future::Cache
        // --- End cache initialization ---

        let event_loop = EventLoop {
            swarm,
            env_config,
            bitcoin_wallet,
            monero_wallet,
            db,
            latest_rate,
            swap_sender: swap_channel.sender,
            min_buy,
            max_buy,
            external_redeem_address,
            quote_cache,
            recv_encrypted_signature: Default::default(),
            inflight_encrypted_signatures: Default::default(),
            outgoing_transfer_proofs_requests,
            outgoing_transfer_proofs_sender,
            buffered_transfer_proofs: Default::default(),
            inflight_transfer_proofs: Default::default(),
        };
        Ok((event_loop, swap_channel.receiver))
    }

    pub fn peer_id(&self) -> PeerId {
        *Swarm::local_peer_id(&self.swarm)
    }

    pub async fn run(mut self) {
        // ensure that these streams are NEVER empty, otherwise it will
        // terminate forever.
        self.inflight_encrypted_signatures
            .push(future::pending().boxed());

        let swaps = match self.db.all().await {
            Ok(swaps) => swaps,
            Err(e) => {
                tracing::error!("Failed to load swaps from database: {}", e);
                return;
            }
        };

        let unfinished_swaps = swaps
            .into_iter()
            .filter(|(_swap_id, state)| !state.swap_finished())
            .collect::<Vec<(Uuid, State)>>();

        for (swap_id, state) in unfinished_swaps {
            let peer_id = match self.db.get_peer_id(swap_id).await {
                Ok(peer_id) => peer_id,
                Err(_) => {
                    tracing::warn!(%swap_id, "Resuming swap skipped because no peer-id found for swap in database");
                    continue;
                }
            };

            let handle = self.new_handle(peer_id, swap_id);

            let swap = Swap {
                event_loop_handle: handle,
                bitcoin_wallet: self.bitcoin_wallet.clone(),
                monero_wallet: self.monero_wallet.clone(),
                env_config: self.env_config,
                db: self.db.clone(),
                state: state.try_into().expect("Alice state loaded from db"),
                swap_id,
            };

            match self.swap_sender.send(swap).await {
                Ok(_) => tracing::info!(%swap_id, "Resuming swap"),
                Err(_) => {
                    tracing::warn!(%swap_id, "Failed to resume swap because receiver has been dropped")
                }
            }
        }

        loop {
            tokio::select! {
                swarm_event = self.swarm.select_next_some() => {
                    match swarm_event {
                        SwarmEvent::Behaviour(OutEvent::SwapSetupInitiated { mut send_wallet_snapshot }) => {
                            let (btc, responder) = match send_wallet_snapshot.recv().await {
                                Ok((btc, responder)) => (btc, responder),
                                Err(error) => {
                                    tracing::error!("Swap request will be ignored because of a failure when requesting information for the wallet snapshot: {:#}", error);
                                    continue;
                                }
                            };

                            let wallet_snapshot = match WalletSnapshot::capture(&self.bitcoin_wallet, &self.monero_wallet, &self.external_redeem_address, btc).await {
                                Ok(wallet_snapshot) => wallet_snapshot,
                                Err(error) => {
                                    tracing::error!("Swap request will be ignored because we were unable to create wallet snapshot for swap: {:#}", error);
                                    continue;
                                }
                            };

                            // Ignore result, we should never hit this because the receiver will alive as long as the connection is.
                            let _ = responder.respond(wallet_snapshot);
                        }
                        SwarmEvent::Behaviour(OutEvent::SwapSetupCompleted{peer_id, swap_id, state3}) => {
                            self.handle_execution_setup_done(peer_id, swap_id, state3).await;
                        }
                        SwarmEvent::Behaviour(OutEvent::SwapDeclined { peer, error }) => {
                            tracing::warn!(%peer, "Ignoring spot price request: {}", error);
                        }
                        SwarmEvent::Behaviour(OutEvent::QuoteRequested { channel, peer }) => {
                            // --- Use the new caching function ---
                            match self.make_quote_or_use_cached().await {
                                Ok(quote) => {
                                    if self.swarm.behaviour_mut().quote.send_response(channel, quote).is_err() {
                                        tracing::debug!(%peer, "Failed to respond with quote");
                                    }
                                }
                                Err(error) => {
                                    tracing::warn!(%peer, "Failed to make or retrieve quote: {:#}", error);
                                    continue;
                                }
                            }
                            // --- End use of caching function ---
                        }
                        SwarmEvent::Behaviour(OutEvent::TransferProofAcknowledged { peer, id }) => {
                            tracing::debug!(%peer, "Bob acknowledged transfer proof");

                            if let Some(responder) = self.inflight_transfer_proofs.remove(&id) {
                                let _ = responder.send(Ok(()));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::EncryptedSignatureReceived{ msg, channel, peer }) => {
                            let swap_id = msg.swap_id;
                            let swap_peer = self.db.get_peer_id(swap_id).await;

                            // Ensure that an incoming encrypted signature is sent by the peer-id associated with the swap
                            let swap_peer = match swap_peer {
                                Ok(swap_peer) => swap_peer,
                                Err(_) => {
                                    tracing::warn!(
                                        unknown_swap_id = %swap_id,
                                        from = %peer,
                                        "Ignoring encrypted signature for unknown swap");
                                    continue;
                                }
                            };

                            if swap_peer != peer {
                                tracing::warn!(
                                    %swap_id,
                                    received_from = %peer,
                                    expected_from = %swap_peer,
                                    "Ignoring malicious encrypted signature which was not expected from this peer",
                                    );
                                continue;
                            }

                            // Immediately acknowledge if we've already processed this encrypted signature
                            // This handles the case where Bob didn't receive our previous acknowledgment
                            // and is retrying sending the encrypted signature
                            if let Ok(state) = self.db.get_state(swap_id).await {
                                let state: AliceState = state.try_into()
                                    .expect("Alices database only contains Alice states");

                                // Check if we have already processed the encrypted signature
                                if has_already_processed_enc_sig(&state) {
                                    tracing::warn!(%swap_id, "Received encrypted signature for swap in state {}. We have already processed this encrypted signature. Acknowledging immediately.", state);

                                    // We push create a future that will resolve immediately, and returns the channel
                                    // This will be resolved in the next iteration of the event loop, and the acknowledgment will be sent to Bob
                                    self.inflight_encrypted_signatures.push(async move {
                                        channel
                                    }.boxed());

                                    continue;
                                }
                            }

                            let sender = match self.recv_encrypted_signature.remove(&swap_id) {
                                Some(sender) => sender,
                                None => {
                                    // TODO: Don't just drop encsig if we currently don't have a running swap for it, save in db
                                    // 1. Save the encrypted signature in the database
                                    // 2. Acknowledge the receipt of the encrypted signature
                                    tracing::warn!(%swap_id, "No sender for encrypted signature, maybe already handled?");
                                    continue;
                                }
                            };

                            let mut responder = match sender.send(msg.tx_redeem_encsig).await {
                                Ok(responder) => responder,
                                Err(_) => {
                                    tracing::warn!(%swap_id, "Failed to relay encrypted signature to swap");
                                    continue;
                                }
                            };

                            self.inflight_encrypted_signatures.push(async move {
                                let _ = responder.recv().await;

                                channel
                            }.boxed());
                        }
                        SwarmEvent::Behaviour(OutEvent::CooperativeXmrRedeemRequested { swap_id, channel, peer }) => {
                            let swap_peer = self.db.get_peer_id(swap_id).await;
                            let swap_state = self.db.get_state(swap_id).await;

                            // If we do not find the swap in the database, or we do not have a peer-id for it, reject
                            let (swap_peer, swap_state) = match (swap_peer, swap_state) {
                                (Ok(peer), Ok(state)) => (peer, state),
                                _ => {
                                    tracing::warn!(
                                        swap_id = %swap_id,
                                        received_from = %peer,
                                        reason = "swap not found",
                                        "Rejecting cooperative XMR redeem request"
                                    );
                                    if self.swarm.behaviour_mut().cooperative_xmr_redeem.send_response(channel, Rejected { swap_id, reason: CooperativeXmrRedeemRejectReason::UnknownSwap }).is_err() {
                                        tracing::error!(swap_id = %swap_id, "Failed to reject cooperative XMR redeem request");
                                    }
                                    continue;
                                }
                            };

                            // If the peer is not the one associated with the swap, reject
                            if swap_peer != peer {
                                tracing::warn!(
                                    swap_id = %swap_id,
                                    received_from = %peer,
                                    expected_from = %swap_peer,
                                    reason = "unexpected peer",
                                    "Rejecting cooperative XMR redeem request"
                                );
                                if self.swarm.behaviour_mut().cooperative_xmr_redeem.send_response(channel, Rejected { swap_id, reason: CooperativeXmrRedeemRejectReason::MaliciousRequest }).is_err() {
                                    tracing::error!(swap_id = %swap_id, "Failed to reject cooperative XMR redeem request");
                                }
                                continue;
                            }

                            // If we are in either of these states, the punish timelock has expired
                            // Bob cannot refund the Bitcoin anymore. We can publish tx_punish to redeem the Bitcoin.
                            // Therefore it is safe to reveal s_a to let him redeem the Monero
                            let State::Alice (AliceState::BtcPunished { state3 } | AliceState::BtcPunishable { state3, .. }) = swap_state else {
                                tracing::warn!(
                                    swap_id = %swap_id,
                                    reason = "swap is in invalid state",
                                    "Rejecting cooperative XMR redeem request"
                                );
                                if self.swarm.behaviour_mut().cooperative_xmr_redeem.send_response(channel, Rejected { swap_id, reason: CooperativeXmrRedeemRejectReason::SwapInvalidState }).is_err() {
                                    tracing::error!(swap_id = %swap_id, "Failed to reject cooperative XMR redeem request");
                                }
                                continue;
                            };

                            if self.swarm.behaviour_mut().cooperative_xmr_redeem.send_response(channel, Fullfilled { swap_id, s_a: state3.s_a }).is_err() {
                                tracing::error!(peer = %peer, "Failed to respond to cooperative XMR redeem request");
                                continue;
                            }

                            tracing::info!(swap_id = %swap_id, peer = %peer, "Fullfilled cooperative XMR redeem request");
                        }
                        SwarmEvent::Behaviour(OutEvent::Rendezvous(libp2p::rendezvous::client::Event::Registered { rendezvous_node, ttl, namespace })) => {
                            tracing::trace!("Successfully registered with rendezvous node: {} with namespace: {} and TTL: {:?}", rendezvous_node, namespace, ttl);
                        }
                        SwarmEvent::Behaviour(OutEvent::Rendezvous(libp2p::rendezvous::client::Event::RegisterFailed { rendezvous_node, namespace, error })) => {
                            tracing::trace!("Registration with rendezvous node {} failed for namespace {}: {:?}", rendezvous_node, namespace, error);
                        }
                        SwarmEvent::Behaviour(OutEvent::OutboundRequestResponseFailure {peer, error, request_id, protocol}) => {
                            tracing::error!(
                                %peer,
                                %request_id,
                                %error,
                                %protocol,
                                "Failed to send request-response request to peer");

                            if let Some(responder) = self.inflight_transfer_proofs.remove(&request_id) {
                                let _ = responder.send(Err(error));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::InboundRequestResponseFailure {peer, error, request_id, protocol}) => {
                            tracing::error!(
                                %peer,
                                %request_id,
                                %error,
                                %protocol,
                                "Failed to receive request-response request from peer");
                        }
                        SwarmEvent::Behaviour(OutEvent::Failure {peer, error}) => {
                            tracing::error!(
                                %peer,
                                "Communication error: {:#}", error);
                        }
                        SwarmEvent::ConnectionEstablished { peer_id: peer, endpoint, .. } => {
                            tracing::trace!(%peer, address = %endpoint.get_remote_address(), "New connection established");

                            // If we have buffered transfer proofs for this peer, we can now send them
                            if let Some(transfer_proofs) = self.buffered_transfer_proofs.remove(&peer) {
                                for (transfer_proof, responder) in transfer_proofs {
                                    tracing::debug!(%peer, "Found buffered transfer proof for peer");

                                    // We have an established connection to the peer, so we can add the transfer proof to the queue
                                    // This is then polled in the next iteration of the event loop, and attempted to be sent to the peer
                                    if let Err(e) = self.outgoing_transfer_proofs_sender.send((peer, transfer_proof, responder)) {
                                        tracing::error!(%peer, error = %e, "Failed to forward buffered transfer proof to event loop channel");
                                    }
                                }
                            }
                        }
                        SwarmEvent::IncomingConnectionError { send_back_addr: address, error, .. } => {
                            tracing::trace!(%address, "Failed to set up connection with peer: {:#}", error);
                        }
                        SwarmEvent::ConnectionClosed { peer_id: peer, num_established: 0, endpoint, cause: Some(error), connection_id } => {
                            tracing::trace!(%peer, address = %endpoint.get_remote_address(), %connection_id, "Lost connection to peer: {:#}", error);
                        }
                        SwarmEvent::ConnectionClosed { peer_id: peer, num_established: 0, endpoint, cause: None, connection_id } => {
                            tracing::trace!(%peer, address = %endpoint.get_remote_address(), %connection_id,  "Successfully closed connection");
                        }
                        SwarmEvent::NewListenAddr{address, ..} => {
                            tracing::info!(%address, "New listen address reported");
                        }
                        _ => {}
                    }
                },
                Some((peer, transfer_proof, responder)) = self.outgoing_transfer_proofs_requests.recv() => {
                    // If we are not connected to the peer, we buffer the transfer proof
                    if !self.swarm.behaviour_mut().transfer_proof.is_connected(&peer) {
                        tracing::warn!(%peer, "No active connection to peer, buffering transfer proof");
                        self.buffered_transfer_proofs.entry(peer).or_default().push((transfer_proof, responder));
                        continue;
                    }

                    // If we are connected to the peer, we attempt to send the transfer proof
                    let id = self.swarm.behaviour_mut().transfer_proof.send_request(&peer, transfer_proof);
                    self.inflight_transfer_proofs.insert(id, responder);
                },
                Some(response_channel) = self.inflight_encrypted_signatures.next() => {
                    let _ = self.swarm.behaviour_mut().encrypted_signature.send_response(response_channel, ());
                }
            }
        }
    }

    /// Get a quote from the cache or calculate a new one using moka::future::Cache
    /// Stores the Result<Arc<BidQuote>, Arc<anyhow::Error>> in the cache.
    async fn make_quote_or_use_cached(&self) -> Result<BidQuote> {
        let key = QuoteCacheKey;
        
        // Clone needed data for the async calculation block
        let min_buy = self.min_buy;
        let max_buy = self.max_buy;
        let mut latest_rate = self.latest_rate.clone();
        let monero_wallet = self.monero_wallet.clone();

        // get_with expects the future to return V, where V is our Result<..., Arc<Error>>
        let cached_result: Result<Arc<BidQuote>, Arc<anyhow::Error>> = self.quote_cache.get_with(key, async move {
            tracing::debug!("Cache miss or expired, calculating new quote result");
            
            // Inner function to perform the calculation and return Result<..., anyhow::Error>
            let calculation_result = async {
                 let ask_price = latest_rate
                    .latest_rate()
                    .context("Failed to get latest rate")?
                    .ask()
                    .context("Failed to compute asking price")?;

                let balance = monero_wallet.get_balance().await?;
                let xmr_balance = Amount::from_piconero(balance.unlocked_balance);

                let max_bitcoin_for_monero =
                    xmr_balance
                        .max_bitcoin_for_price(ask_price)
                        .ok_or_else(|| {
                            anyhow!(
                                "Bitcoin price ({}) x Monero ({}) overflow",
                                ask_price,
                                xmr_balance
                            )
                        })?;

                tracing::trace!(%ask_price, %xmr_balance, %max_bitcoin_for_monero, "Computed quote");

                if min_buy > max_bitcoin_for_monero {
                    tracing::trace!(
                        "Your Monero balance is too low... Min: {}, Max possible: {}",
                        min_buy, max_bitcoin_for_monero
                    );
                    return Ok(Arc::new(BidQuote {
                        price: ask_price,
                        min_quantity: bitcoin::Amount::ZERO,
                        max_quantity: bitcoin::Amount::ZERO,
                    }));
                }

                if max_buy > max_bitcoin_for_monero {
                    tracing::trace!(
                        "Your Monero balance is too low... Max requested: {}, Max possible: {}",
                        max_buy, max_bitcoin_for_monero
                    );
                    return Ok(Arc::new(BidQuote {
                        price: ask_price,
                        min_quantity: min_buy,
                        max_quantity: max_bitcoin_for_monero,
                    }));
                }

                Ok(Arc::new(BidQuote {
                    price: ask_price,
                    min_quantity: min_buy,
                    max_quantity: max_buy,
                }))
            }.await;
            
            // Map the Result<..., anyhow::Error> to Result<..., Arc<anyhow::Error>> for caching
            calculation_result.map_err(Arc::new)
        }).await;

        // The cached_result is the actual Result we stored.
        // Now, convert it back to the expected return type Result<BidQuote, anyhow::Error>
        match cached_result {
            Ok(bid_quote_arc) => Ok((*bid_quote_arc).clone()), // Clone the BidQuote out of the Arc
            Err(error_arc) => {
                 // Clone the error message from the Arc<anyhow::Error>
                 // We convert it back to a regular anyhow::Error
                 Err(anyhow::Error::msg(error_arc.to_string()))
            }
        }
    }

    /// Original make_quote (potentially unused if all callers switch)
    async fn make_quote(
        &mut self, // Note: might still need &mut if latest_rate() does
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
    ) -> Result<BidQuote> {
        let ask_price = self
            .latest_rate
            .latest_rate()
            .context("Failed to get latest rate")?
            .ask()
            .context("Failed to compute asking price")?;

        let balance = self.monero_wallet.get_balance().await?;
        let xmr_balance = Amount::from_piconero(balance.unlocked_balance);

        let max_bitcoin_for_monero =
            xmr_balance
                .max_bitcoin_for_price(ask_price)
                .ok_or_else(|| {
                    anyhow!(
                        "Bitcoin price ({}) x Monero ({}) overflow",
                        ask_price,
                        xmr_balance
                    )
                })?;

        tracing::trace!(%ask_price, %xmr_balance, %max_bitcoin_for_monero, "Computed quote");

        if min_buy > max_bitcoin_for_monero {
             tracing::trace!(
                 "Your Monero balance is too low... Min: {}, Max possible: {}",
                 min_buy, max_bitcoin_for_monero
             );
            return Ok(BidQuote {
                price: ask_price,
                min_quantity: bitcoin::Amount::ZERO,
                max_quantity: bitcoin::Amount::ZERO,
            });
        }

        if max_buy > max_bitcoin_for_monero {
             tracing::trace!(
                 "Your Monero balance is too low... Max requested: {}, Max possible: {}",
                 max_buy, max_bitcoin_for_monero
             );
            return Ok(BidQuote {
                price: ask_price,
                min_quantity: min_buy,
                max_quantity: max_bitcoin_for_monero,
            });
        }

        Ok(BidQuote {
            price: ask_price,
            min_quantity: min_buy,
            max_quantity: max_buy,
        })
    }

    /// Removed cache invalidation logic from handle_execution_setup_done for now
    async fn handle_execution_setup_done(
        &mut self,
        bob_peer_id: PeerId,
        swap_id: Uuid,
        state3: State3,
    ) {
        // Original logic without cache invalidation
        let handle = self.new_handle(bob_peer_id, swap_id);

        let initial_state = AliceState::Started {
            state3: Box::new(state3),
        };

        let swap = Swap {
            event_loop_handle: handle,
            bitcoin_wallet: self.bitcoin_wallet.clone(),
            monero_wallet: self.monero_wallet.clone(),
            env_config: self.env_config,
            db: self.db.clone(),
            state: initial_state,
            swap_id,
        };

        match self.db.insert_peer_id(swap_id, bob_peer_id).await {
            Ok(_) => {
                if let Err(error) = self.swap_sender.send(swap).await {
                    tracing::warn!(%swap_id, "Failed to start swap: {}", error);
                }
            }
            Err(error) => {
                tracing::warn!(%swap_id, "Unable to save peer-id in database: {}", error);
            }
        }
    }

    /// Create a new [`EventLoopHandle`] that is scoped for communication with
    /// the given peer.
    fn new_handle(&mut self, peer: PeerId, swap_id: Uuid) -> EventLoopHandle {
        // Create a new channel for receiving encrypted signatures from Bob
        // The channel has a capacity of 1 since we only expect one signature per swap
        let (encrypted_signature_sender, encrypted_signature_receiver) = bmrng::channel(1);

        // The sender is stored in the EventLoop
        // The receiver is stored in the EventLoopHandle
        // When a signature is received, the EventLoop uses the sender to notify the EventLoopHandle
        self.recv_encrypted_signature
            .insert(swap_id, encrypted_signature_sender);

        let transfer_proof_sender = self.outgoing_transfer_proofs_sender.clone();

        EventLoopHandle {
            swap_id,
            peer,
            recv_encrypted_signature: Some(encrypted_signature_receiver),
            transfer_proof_sender: Some(transfer_proof_sender),
        }
    }
}

pub trait LatestRate {
    type Error: std::error::Error + Send + Sync + 'static;

    fn latest_rate(&mut self) -> Result<Rate, Self::Error>;
}

#[derive(Clone, Debug)]
pub struct FixedRate(Rate);

impl FixedRate {
    pub const RATE: f64 = 0.01;

    pub fn value(&self) -> Rate {
        self.0
    }
}

impl Default for FixedRate {
    fn default() -> Self {
        let ask = bitcoin::Amount::from_btc(Self::RATE).expect("Static value should never fail");
        let spread = Decimal::from(0u64);

        Self(Rate::new(ask, spread))
    }
}

impl LatestRate for FixedRate {
    type Error = Infallible;

    fn latest_rate(&mut self) -> Result<Rate, Self::Error> {
        Ok(self.value())
    }
}

/// Produces [`Rate`]s based on [`PriceUpdate`]s from kraken and a configured
/// spread.
#[derive(Debug, Clone)]
pub struct KrakenRate {
    ask_spread: Decimal,
    price_updates: kraken::PriceUpdates,
}

impl KrakenRate {
    pub fn new(ask_spread: Decimal, price_updates: kraken::PriceUpdates) -> Self {
        Self {
            ask_spread,
            price_updates,
        }
    }
}

impl LatestRate for KrakenRate {
    type Error = kraken::Error;

    fn latest_rate(&mut self) -> Result<Rate, Self::Error> {
        let update = self.price_updates.latest_update()?;
        let rate = Rate::new(update.ask, self.ask_spread);

        Ok(rate)
    }
}

#[derive(Debug)]
pub struct EventLoopHandle {
    swap_id: Uuid,
    peer: PeerId,
    recv_encrypted_signature: Option<bmrng::RequestReceiver<bitcoin::EncryptedSignature, ()>>,
    #[allow(clippy::type_complexity)]
    transfer_proof_sender: Option<
        tokio::sync::mpsc::UnboundedSender<(
            PeerId,
            transfer_proof::Request,
            oneshot::Sender<Result<(), OutboundFailure>>,
        )>,
    >,
}

impl EventLoopHandle {
    fn build_transfer_proof_request(
        &self,
        transfer_proof: monero::TransferProof,
    ) -> transfer_proof::Request {
        transfer_proof::Request {
            swap_id: self.swap_id,
            tx_lock_proof: transfer_proof,
        }
    }

    /// Wait for an encrypted signature from Bob
    pub async fn recv_encrypted_signature(&mut self) -> Result<bitcoin::EncryptedSignature> {
        let receiver = self
            .recv_encrypted_signature
            .as_mut()
            .context("Encrypted signature was already received")?;

        let (tx_redeem_encsig, responder) = receiver.recv().await?;

        // Acknowledge receipt of the encrypted signature
        // This notifies the EventLoop that the signature has been processed
        // The EventLoop can then send an acknowledgement back to Bob over the network
        responder
            .respond(())
            .context("Failed to acknowledge receipt of encrypted signature")?;

        // Only take after successful receipt and acknowledgement
        self.recv_encrypted_signature.take();

        Ok(tx_redeem_encsig)
    }

    /// Send a transfer proof to Bob
    ///
    /// This function will retry indefinitely until the transfer proof is sent successfully
    /// and acknowledged by Bob
    ///
    /// This will fail if
    /// 1. the transfer proof has already been sent once
    /// 2. there is an error with the bmrng channel
    pub async fn send_transfer_proof(&mut self, msg: monero::TransferProof) -> Result<()> {
        let sender = self
            .transfer_proof_sender
            .as_ref()
            .context("Transfer proof was already sent")?;

        // We will retry indefinitely until we succeed
        let backoff = backoff::ExponentialBackoffBuilder::new()
            .with_max_elapsed_time(None)
            .with_max_interval(Duration::from_secs(60))
            .build();

        let transfer_proof = self.build_transfer_proof_request(msg);

        backoff::future::retry_notify(
            backoff,
            || async {
                // Create a oneshot channel to receive the acknowledgment of the transfer proof
                let (singular_sender, singular_receiver) = oneshot::channel();

                if let Err(err) = sender.send((self.peer, transfer_proof.clone(), singular_sender))
                {
                    return Err(backoff::Error::permanent(anyhow!(err).context(
                        "Failed to communicate transfer proof through event loop channel",
                    )));
                }

                match singular_receiver.await {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(err)) => Err(backoff::Error::transient(
                        anyhow!(err)
                            .context("A network error occurred while sending the transfer proof"),
                    )),
                    Err(_) => Err(backoff::Error::permanent(anyhow!(
                        "The sender channel should never be closed without sending a response"
                    ))),
                }
            },
            |e, wait_time: Duration| {
                tracing::warn!(
                    swap_id = %self.swap_id,
                    error = ?e,
                    "Failed to send transfer proof. We will retry in {} seconds",
                    wait_time.as_secs()
                )
            },
        )
        .await?;

        self.transfer_proof_sender.take();

        Ok(())
    }
}

#[allow(missing_debug_implementations)]
struct MpscChannels<T> {
    sender: mpsc::Sender<T>,
    receiver: mpsc::Receiver<T>,
}

impl<T> Default for MpscChannels<T> {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel(100);
        MpscChannels { sender, receiver }
    }
}
