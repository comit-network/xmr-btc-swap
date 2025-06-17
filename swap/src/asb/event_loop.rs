use crate::asb::{Behaviour, OutEvent, Rate};
use crate::network::cooperative_xmr_redeem_after_punish::CooperativeXmrRedeemRejectReason;
use crate::network::cooperative_xmr_redeem_after_punish::Response::{Fullfilled, Rejected};
use crate::network::quote::BidQuote;
use crate::network::swap_setup::alice::WalletSnapshot;
use crate::network::transfer_proof;
use crate::protocol::alice::swap::has_already_processed_enc_sig;
use crate::protocol::alice::{AliceState, ReservesMonero, State3, Swap};
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
use monero::Amount;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::convert::{Infallible, TryInto};
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use uuid::Uuid;

/// The time-to-live for quotes in the cache
const QUOTE_CACHE_TTL: Duration = Duration::from_secs(120);

/// The key for the quote cache
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct QuoteCacheKey {
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
}

#[allow(missing_debug_implementations)]
pub struct EventLoop<LR>
where
    LR: LatestRate + Send + 'static + Debug + Clone,
{
    swarm: libp2p::Swarm<Behaviour<LR>>,
    env_config: env::Config,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallets>,
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
        monero_wallet: Arc<monero::Wallets>,
        db: Arc<dyn Database + Send + Sync>,
        latest_rate: LR,
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
        external_redeem_address: Option<bitcoin::Address>,
    ) -> Result<(Self, mpsc::Receiver<Swap>)> {
        let swap_channel = MpscChannels::default();
        let (outgoing_transfer_proofs_sender, outgoing_transfer_proofs_requests) =
            tokio::sync::mpsc::unbounded_channel();

        let quote_cache = Cache::builder().time_to_live(QUOTE_CACHE_TTL).build();

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
                            match self.make_quote_or_use_cached(self.min_buy, self.max_buy).await {
                                Ok(quote_arc) => {
                                    if self.swarm.behaviour_mut().quote.send_response(channel, *quote_arc).is_err() {
                                        tracing::debug!(%peer, "Failed to respond with quote");
                                    }
                                }
                                // The error is already logged in the make_quote_or_use_cached function
                                // We don't log it here to avoid spamming on each request
                                Err(_) => {
                                    // We respond with a zero quote. This will stop Bob from trying to start a swap but doesn't require
                                    // a breaking network change by changing the definition of the quote protocol
                                    if self
                                        .swarm
                                        .behaviour_mut()
                                        .quote
                                        .send_response(channel, BidQuote::ZERO)
                                        .is_err()
                                    {
                                        tracing::debug!(%peer, "Failed to respond with zero quote");
                                    }
                                }
                            }
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
                            let State::Alice (AliceState::BtcPunished { state3, transfer_proof, .. } | AliceState::BtcPunishable { state3, transfer_proof, .. }) = swap_state else {
                                tracing::warn!(
                                    swap_id = %swap_id,
                                    reason = "swap is in invalid state",
                                    "Rejecting cooperative Monero redeem request"
                                );
                                if self.swarm.behaviour_mut().cooperative_xmr_redeem.send_response(channel, Rejected { swap_id, reason: CooperativeXmrRedeemRejectReason::SwapInvalidState }).is_err() {
                                    tracing::error!(swap_id = %swap_id, "Failed to send rejection for cooperative Monero redeem request");
                                }
                                continue;
                            };

                            if self.swarm.behaviour_mut().cooperative_xmr_redeem.send_response(channel, Fullfilled { swap_id, s_a: state3.s_a, lock_transfer_proof: transfer_proof }).is_err() {
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
                                ?error,
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
                                ?error,
                                %protocol,
                                "Failed to receive request-response request from peer");
                        }
                        SwarmEvent::Behaviour(OutEvent::Failure {peer, error}) => {
                            tracing::error!(
                                %peer,
                                "Communication error: {:?}", error);
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
                                        tracing::error!(%peer, error = ?e, "Failed to forward buffered transfer proof to event loop channel");
                                    }
                                }
                            }
                        }
                        SwarmEvent::IncomingConnectionError { send_back_addr: address, error, .. } => {
                            tracing::trace!(%address, "Failed to set up connection with peer: {:?}", error);
                        }
                        SwarmEvent::ConnectionClosed { peer_id: peer, num_established: 0, endpoint, cause: Some(error), connection_id } => {
                            tracing::trace!(%peer, address = %endpoint.get_remote_address(), %connection_id, "Lost connection to peer: {:?}", error);
                        }
                        SwarmEvent::ConnectionClosed { peer_id: peer, num_established: 0, endpoint, cause: None, connection_id } => {
                            tracing::trace!(%peer, address = %endpoint.get_remote_address(), %connection_id,  "Successfully closed connection");
                        }
                        SwarmEvent::NewListenAddr{address, ..} => {
                            let multiaddr = format!("{address}/p2p/{}", self.swarm.local_peer_id());
                            tracing::info!(%address, %multiaddr, "New listen address reported");
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

    /// Get a quote from the cache or calculate a new one by calling make_quote.
    /// Returns the result wrapped in Arcs for consistent caching.
    async fn make_quote_or_use_cached(
        &mut self,
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
    ) -> Result<Arc<BidQuote>, Arc<anyhow::Error>> {
        // We use the min and max buy amounts to create a unique key for the cache
        // Although these values stay constant over the lifetime of an instance of the asb, this might change in the future
        let key = QuoteCacheKey { min_buy, max_buy };

        // Check if we have a cached quote
        let maybe_cached_quote = self.quote_cache.get(&key).await;

        if let Some(cached_quote_result) = maybe_cached_quote {
            tracing::trace!("Got a request for a quote, using cached value.");
            return cached_quote_result;
        }

        // We have a cache miss, so we compute a new quote
        tracing::trace!("Got a request for a quote, computing new quote.");

        let rate = self.latest_rate.clone();

        let get_reserved_items = || async {
            let all_swaps = self.db.all().await?;
            let alice_states: Vec<_> = all_swaps
                .into_iter()
                .filter_map(|(_, state)| match state {
                    State::Alice(state) => Some(state),
                    _ => None,
                })
                .collect();

            Ok(alice_states)
        };

        let monero_wallet = self.monero_wallet.clone();
        let get_unlocked_balance = || async {
            unlocked_monero_balance_with_timeout(monero_wallet.main_wallet().await).await
        };

        let result = make_quote(
            min_buy,
            max_buy,
            rate,
            get_unlocked_balance,
            get_reserved_items,
        )
        .await;

        // Insert the computed quote into the cache
        // Need to clone it as insert takes ownership
        self.quote_cache.insert(key, result.clone()).await;

        // If the quote failed, we log the error
        if let Err(err) = result.clone() {
            tracing::warn!(%err, "Failed to make quote. We will retry again later.");
        }

        // Return the computed quote
        result
    }

    async fn handle_execution_setup_done(
        &mut self,
        bob_peer_id: PeerId,
        swap_id: Uuid,
        state3: State3,
    ) {
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
                    tracing::warn!(%swap_id, "Failed to start swap: {:?}", error);
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

/// Computes a quote given the provided dependencies
pub async fn make_quote<LR, F, Fut, I, Fut2, T>(
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    mut latest_rate: LR,
    get_unlocked_balance: F,
    get_reserved_items: I,
) -> Result<Arc<BidQuote>, Arc<anyhow::Error>>
where
    LR: LatestRate,
    F: FnOnce() -> Fut,
    Fut: futures::Future<Output = Result<Amount, anyhow::Error>>,
    I: FnOnce() -> Fut2,
    Fut2: futures::Future<Output = Result<Vec<T>, anyhow::Error>>,
    T: ReservesMonero,
{
    let ask_price = latest_rate
        .latest_rate()
        .map_err(|e| Arc::new(anyhow!(e).context("Failed to get latest rate")))?
        .ask()
        .map_err(|e| Arc::new(e.context("Failed to compute asking price")))?;

    // Get the unlocked balance
    let unlocked_balance = get_unlocked_balance()
        .await
        .context("Failed to get unlocked Monero balance")
        .map_err(Arc::new)?;

    // Get the reserved amounts
    let reserved_amounts: Vec<Amount> = get_reserved_items()
        .await
        .context("Failed to get reserved items")
        .map_err(Arc::new)?
        .into_iter()
        .map(|item| item.reserved_monero())
        .collect();

    let unreserved_xmr_balance =
        unreserved_monero_balance(unlocked_balance, reserved_amounts.into_iter());

    let max_bitcoin_for_monero = unreserved_xmr_balance
        .max_bitcoin_for_price(ask_price)
        .ok_or_else(|| {
            Arc::new(anyhow!(
                "Bitcoin price ({}) x Monero ({}) overflow",
                ask_price,
                unreserved_xmr_balance
            ))
        })?;

    tracing::trace!(%ask_price, %unreserved_xmr_balance, %max_bitcoin_for_monero, "Computed quote");

    if min_buy > max_bitcoin_for_monero {
        tracing::trace!(
            "Your Monero balance is too low to initiate a swap, as your minimum swap amount is {}. You could at most swap {}",
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
            "Your Monero balance is too low to initiate a swap with the maximum swap amount {} that you have specified in your config. You can at most swap {}",
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
}

/// Calculates the unreserved Monero balance by subtracting reserved amounts from unlocked balance
pub fn unreserved_monero_balance(
    unlocked_balance: Amount,
    reserved_amounts: impl Iterator<Item = Amount>,
) -> Amount {
    // Get the sum of all the individual reserved amounts
    let total_reserved = reserved_amounts.fold(Amount::ZERO, |acc, amount| acc + amount);

    // Check how much of our unlocked balance is left when we
    // take into account the reserved amounts
    unlocked_balance
        .checked_sub(total_reserved)
        .unwrap_or(Amount::ZERO)
}

/// Returns the unlocked Monero balance from the wallet
async fn unlocked_monero_balance_with_timeout(
    wallet: Arc<monero::Wallet>,
) -> Result<Amount, anyhow::Error> {
    /// This is how long we maximally wait for the wallet operation
    const MONERO_WALLET_OPERATION_TIMEOUT: Duration = Duration::from_secs(10);

    let balance = timeout(MONERO_WALLET_OPERATION_TIMEOUT, wallet.unlocked_balance())
        .await
        .context("Timeout while getting unlocked balance from Monero wallet")?;

    Ok(balance.into())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_unreserved_monero_balance_with_no_reserved_amounts() {
        let balance = Amount::from_monero(10.0).unwrap();
        let reserved_amounts = vec![];

        let result = unreserved_monero_balance(balance, reserved_amounts.into_iter());

        assert_eq!(result, balance);
    }

    #[tokio::test]
    async fn test_unreserved_monero_balance_with_reserved_amounts() {
        let balance = Amount::from_monero(10.0).unwrap();
        let reserved_amounts = vec![
            Amount::from_monero(2.0).unwrap(),
            Amount::from_monero(3.0).unwrap(),
        ];

        let result = unreserved_monero_balance(balance, reserved_amounts.into_iter());

        let expected = Amount::from_monero(5.0).unwrap();
        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn test_unreserved_monero_balance_insufficient_balance() {
        let balance = Amount::from_monero(5.0).unwrap();
        let reserved_amounts = vec![
            Amount::from_monero(3.0).unwrap(),
            Amount::from_monero(4.0).unwrap(), // Total reserved > balance
        ];

        let result = unreserved_monero_balance(balance, reserved_amounts.into_iter());

        // Should return zero when reserved > balance
        assert_eq!(result, Amount::ZERO);
    }

    #[tokio::test]
    async fn test_unreserved_monero_balance_exact_match() {
        let balance = Amount::from_monero(10.0).unwrap();
        let reserved_amounts = vec![
            Amount::from_monero(4.0).unwrap(),
            Amount::from_monero(6.0).unwrap(), // Exactly equals balance
        ];

        let result = unreserved_monero_balance(balance, reserved_amounts.into_iter());

        assert_eq!(result, Amount::ZERO);
    }

    #[tokio::test]
    async fn test_unreserved_monero_balance_zero_balance() {
        let balance = Amount::ZERO;
        let reserved_amounts = vec![Amount::from_monero(1.0).unwrap()];

        let result = unreserved_monero_balance(balance, reserved_amounts.into_iter());

        assert_eq!(result, Amount::ZERO);
    }

    #[tokio::test]
    async fn test_unreserved_monero_balance_empty_reserved_amounts() {
        let balance = Amount::from_monero(5.0).unwrap();
        let reserved_amounts: Vec<Amount> = vec![];

        let result = unreserved_monero_balance(balance, reserved_amounts.into_iter());

        assert_eq!(result, balance);
    }

    #[tokio::test]
    async fn test_unreserved_monero_balance_large_amounts() {
        let balance = Amount::from_piconero(1_000_000_000);
        let reserved_amounts = vec![Amount::from_piconero(300_000_000)];

        let result = unreserved_monero_balance(balance, reserved_amounts.into_iter());

        let expected = Amount::from_piconero(700_000_000);
        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn test_make_quote_successful_within_limits() {
        let min_buy = bitcoin::Amount::from_sat(100_000);
        let max_buy = bitcoin::Amount::from_sat(500_000);
        let rate = FixedRate::default();
        let balance = Amount::from_monero(1.0).unwrap();
        let reserved_items: Vec<MockReservedItem> = vec![];

        let result = make_quote(
            min_buy,
            max_buy,
            rate.clone(),
            || async { Ok(balance) },
            || async { Ok(reserved_items) },
        )
        .await
        .unwrap();

        assert_eq!(result.price, rate.value().ask().unwrap());
        assert_eq!(result.min_quantity, min_buy);
        assert_eq!(result.max_quantity, max_buy);
    }

    #[tokio::test]
    async fn test_make_quote_with_reserved_amounts() {
        let min_buy = bitcoin::Amount::from_sat(50_000);
        let max_buy = bitcoin::Amount::from_sat(300_000);
        let rate = FixedRate::default();
        let balance = Amount::from_monero(1.0).unwrap();
        let reserved_items = vec![
            MockReservedItem {
                reserved: Amount::from_monero(0.2).unwrap(),
            },
            MockReservedItem {
                reserved: Amount::from_monero(0.3).unwrap(),
            },
        ];

        let result = make_quote(
            min_buy,
            max_buy,
            rate.clone(),
            || async { Ok(balance) },
            || async { Ok(reserved_items) },
        )
        .await
        .unwrap();

        // With 1.0 XMR balance and 0.5 XMR reserved, we have 0.5 XMR available
        // At rate 0.01, that's 0.005 BTC = 500,000 sats maximum
        let expected_max = bitcoin::Amount::from_sat(300_000); // Limited by max_buy
        assert_eq!(result.min_quantity, min_buy);
        assert_eq!(result.max_quantity, expected_max);
    }

    #[tokio::test]
    async fn test_make_quote_insufficient_balance_for_min() {
        let min_buy = bitcoin::Amount::from_sat(600_000); // More than available
        let max_buy = bitcoin::Amount::from_sat(800_000);
        let rate = FixedRate::default();
        let balance = Amount::from_monero(0.5).unwrap(); // Only 0.005 BTC worth at rate 0.01
        let reserved_items: Vec<MockReservedItem> = vec![];

        let result = make_quote(
            min_buy,
            max_buy,
            rate.clone(),
            || async { Ok(balance) },
            || async { Ok(reserved_items) },
        )
        .await
        .unwrap();

        // Should return zero quantities when min_buy exceeds available balance
        assert_eq!(result.min_quantity, bitcoin::Amount::ZERO);
        assert_eq!(result.max_quantity, bitcoin::Amount::ZERO);
    }

    #[tokio::test]
    async fn test_make_quote_limited_by_balance() {
        let min_buy = bitcoin::Amount::from_sat(100_000);
        let max_buy = bitcoin::Amount::from_sat(800_000); // More than available
        let rate = FixedRate::default();
        let balance = Amount::from_monero(0.6).unwrap(); // 0.006 BTC worth at rate 0.01
        let reserved_items: Vec<MockReservedItem> = vec![];

        let result = make_quote(
            min_buy,
            max_buy,
            rate.clone(),
            || async { Ok(balance) },
            || async { Ok(reserved_items) },
        )
        .await
        .unwrap();

        // Calculate the actual max bitcoin for the given balance and rate
        let expected_max = balance
            .max_bitcoin_for_price(rate.value().ask().unwrap())
            .unwrap();
        assert_eq!(result.min_quantity, min_buy);
        assert_eq!(result.max_quantity, expected_max);
    }

    #[tokio::test]
    async fn test_make_quote_all_balance_reserved() {
        let min_buy = bitcoin::Amount::from_sat(100_000);
        let max_buy = bitcoin::Amount::from_sat(500_000);
        let rate = FixedRate::default();
        let balance = Amount::from_monero(1.0).unwrap();
        let reserved_items = vec![MockReservedItem {
            reserved: Amount::from_monero(1.0).unwrap(), // All balance reserved
        }];

        let result = make_quote(
            min_buy,
            max_buy,
            rate.clone(),
            || async { Ok(balance) },
            || async { Ok(reserved_items) },
        )
        .await
        .unwrap();

        // Should return zero quantities when all balance is reserved
        assert_eq!(result.min_quantity, bitcoin::Amount::ZERO);
        assert_eq!(result.max_quantity, bitcoin::Amount::ZERO);
    }

    #[tokio::test]
    async fn test_make_quote_error_getting_balance() {
        let min_buy = bitcoin::Amount::from_sat(100_000);
        let max_buy = bitcoin::Amount::from_sat(500_000);
        let rate = FixedRate::default();
        let reserved_items: Vec<MockReservedItem> = vec![];

        let result = make_quote(
            min_buy,
            max_buy,
            rate.clone(),
            || async { Err(anyhow::anyhow!("Failed to get balance")) },
            || async { Ok(reserved_items) },
        )
        .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to get unlocked Monero balance"));
    }

    #[tokio::test]
    async fn test_make_quote_empty_reserved_items() {
        let min_buy = bitcoin::Amount::from_sat(100_000);
        let max_buy = bitcoin::Amount::from_sat(500_000);
        let rate = FixedRate::default();
        let balance = Amount::from_monero(1.0).unwrap();
        let reserved_items: Vec<MockReservedItem> = vec![];

        let result = make_quote(
            min_buy,
            max_buy,
            rate.clone(),
            || async { Ok(balance) },
            || async { Ok(reserved_items) },
        )
        .await
        .unwrap();

        // Should work normally with empty reserved items
        assert_eq!(result.price, rate.value().ask().unwrap());
        assert_eq!(result.min_quantity, min_buy);
        assert_eq!(result.max_quantity, max_buy);
    }

    // Mock struct for testing
    #[derive(Debug, Clone)]
    struct MockReservedItem {
        reserved: Amount,
    }

    impl ReservesMonero for MockReservedItem {
        fn reserved_monero(&self) -> Amount {
            self.reserved
        }
    }
}
