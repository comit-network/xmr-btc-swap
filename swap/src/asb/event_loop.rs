use crate::asb::{Behaviour, OutEvent, Rate};
use crate::database::Database;
use crate::network::quote::BidQuote;
use crate::network::swap_setup::alice::WalletSnapshot;
use crate::network::transfer_proof;
use crate::protocol::alice::{AliceState, State3, Swap};
use crate::{bitcoin, env, kraken, monero};
use anyhow::{Context, Result};
use futures::future;
use futures::future::{BoxFuture, FutureExt};
use futures::stream::{FuturesUnordered, StreamExt};
use libp2p::request_response::{RequestId, ResponseChannel};
use libp2p::swarm::SwarmEvent;
use libp2p::{PeerId, Swarm};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

/// A future that resolves to a tuple of `PeerId`, `transfer_proof::Request` and
/// `Responder`.
///
/// When this future resolves, the `transfer_proof::Request` shall be sent to
/// the peer identified by the `PeerId`. Once the request has been acknowledged
/// by the peer, i.e. a `()` response has been received, the `Responder` shall
/// be used to let the original sender know about the successful transfer.
type OutgoingTransferProof =
    BoxFuture<'static, Result<(PeerId, transfer_proof::Request, bmrng::Responder<()>)>>;

#[allow(missing_debug_implementations)]
pub struct EventLoop<LR>
where
    LR: LatestRate + Send + 'static + Debug + Clone,
{
    swarm: libp2p::Swarm<Behaviour<LR>>,
    env_config: env::Config,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    db: Arc<Database>,
    latest_rate: LR,
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,

    swap_sender: mpsc::Sender<Swap>,

    /// Stores incoming [`EncryptedSignature`]s per swap.
    recv_encrypted_signature: HashMap<Uuid, bmrng::RequestSender<bitcoin::EncryptedSignature, ()>>,
    inflight_encrypted_signatures: FuturesUnordered<BoxFuture<'static, ResponseChannel<()>>>,

    send_transfer_proof: FuturesUnordered<OutgoingTransferProof>,

    /// Tracks [`transfer_proof::Request`]s which could not yet be sent because
    /// we are currently disconnected from the peer.
    buffered_transfer_proofs: HashMap<PeerId, Vec<(transfer_proof::Request, bmrng::Responder<()>)>>,

    /// Tracks [`transfer_proof::Request`]s which are currently inflight and
    /// awaiting an acknowledgement.
    inflight_transfer_proofs: HashMap<RequestId, bmrng::Responder<()>>,
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
        db: Arc<Database>,
        latest_rate: LR,
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
    ) -> Result<(Self, mpsc::Receiver<Swap>)> {
        let swap_channel = MpscChannels::default();

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
            recv_encrypted_signature: Default::default(),
            inflight_encrypted_signatures: Default::default(),
            send_transfer_proof: Default::default(),
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
        self.send_transfer_proof.push(future::pending().boxed());
        self.inflight_encrypted_signatures
            .push(future::pending().boxed());

        let unfinished_swaps = match self.db.unfinished_alice() {
            Ok(unfinished_swaps) => unfinished_swaps,
            Err(_) => {
                tracing::error!("Failed to load unfinished swaps");
                return;
            }
        };

        for (swap_id, state) in unfinished_swaps {
            let peer_id = match self.db.get_peer_id(swap_id) {
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
                state: state.into(),
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

                            let wallet_snapshot = match WalletSnapshot::capture(&self.bitcoin_wallet, &self.monero_wallet, btc).await {
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
                            let _ = self.handle_execution_setup_done(peer_id, swap_id, state3).await;
                        }
                        SwarmEvent::Behaviour(OutEvent::SwapDeclined { peer, error }) => {
                            tracing::warn!(%peer, "Ignoring spot price request: {}", error);
                        }
                        SwarmEvent::Behaviour(OutEvent::QuoteRequested { channel, peer }) => {
                            let quote = match self.make_quote(self.min_buy, self.max_buy).await {
                                Ok(quote) => quote,
                                Err(error) => {
                                    tracing::warn!(%peer, "Failed to make quote: {:#}", error);
                                    continue;
                                }
                            };

                            if self.swarm.behaviour_mut().quote.send_response(channel, quote).is_err() {
                                tracing::debug!(%peer, "Failed to respond with quote");
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::TransferProofAcknowledged { peer, id }) => {
                            tracing::debug!(%peer, "Bob acknowledged transfer proof");
                            if let Some(responder) = self.inflight_transfer_proofs.remove(&id) {
                                let _ = responder.respond(());
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::EncryptedSignatureReceived{ msg, channel, peer }) => {
                            let swap_id = msg.swap_id;
                            let swap_peer = self.db.get_peer_id(swap_id);

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

                            let sender = match self.recv_encrypted_signature.remove(&swap_id) {
                                Some(sender) => sender,
                                None => {
                                    // TODO: Don't just drop encsig if we currently don't have a running swap for it, save in db
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
                        SwarmEvent::Behaviour(OutEvent::Rendezvous(libp2p::rendezvous::Event::Registered { .. })) => {
                            tracing::info!("Successfully registered with rendezvous node");
                        }
                        SwarmEvent::Behaviour(OutEvent::Rendezvous(libp2p::rendezvous::Event::RegisterFailed(error))) => {
                            tracing::error!("Registration with rendezvous node failed: {:#}", error);
                        }
                        SwarmEvent::Behaviour(OutEvent::Failure {peer, error}) => {
                            tracing::error!(
                                %peer,
                                "Communication error: {:#}", error);
                        }
                        SwarmEvent::ConnectionEstablished { peer_id: peer, endpoint, .. } => {
                            tracing::debug!(%peer, address = %endpoint.get_remote_address(), "New connection established");

                            if let Some(transfer_proofs) = self.buffered_transfer_proofs.remove(&peer) {
                                for (transfer_proof, responder) in transfer_proofs {
                                    tracing::debug!(%peer, "Found buffered transfer proof for peer");

                                    let id = self.swarm.behaviour_mut().transfer_proof.send_request(&peer, transfer_proof);
                                    self.inflight_transfer_proofs.insert(id, responder);
                                }
                            }
                        }
                        SwarmEvent::IncomingConnectionError { send_back_addr: address, error, .. } => {
                            tracing::warn!(%address, "Failed to set up connection with peer: {:#}", error);
                        }
                        SwarmEvent::ConnectionClosed { peer_id: peer, num_established, endpoint, cause: Some(error) } if num_established == 0 => {
                            tracing::debug!(%peer, address = %endpoint.get_remote_address(), "Lost connection to peer: {:#}", error);
                        }
                        SwarmEvent::ConnectionClosed { peer_id: peer, num_established, endpoint, cause: None } if num_established == 0 => {
                            tracing::info!(%peer, address = %endpoint.get_remote_address(), "Successfully closed connection");
                        }
                        SwarmEvent::NewListenAddr(address) => {
                            tracing::info!(%address, "New listen address reported");
                        }
                        _ => {}
                    }
                },
                next_transfer_proof = self.send_transfer_proof.next() => {
                    match next_transfer_proof {
                        Some(Ok((peer, transfer_proof, responder))) => {
                            if !self.swarm.behaviour_mut().transfer_proof.is_connected(&peer) {
                                tracing::warn!(%peer, "No active connection to peer, buffering transfer proof");
                                self.buffered_transfer_proofs.entry(peer).or_insert_with(Vec::new).push((transfer_proof, responder));
                                continue;
                            }

                            let id = self.swarm.behaviour_mut().transfer_proof.send_request(&peer, transfer_proof);
                            self.inflight_transfer_proofs.insert(id, responder);
                        },
                        Some(Err(error)) => {
                            tracing::debug!("A swap stopped without sending a transfer proof: {:#}", error);
                        }
                        None => {
                            unreachable!("stream of transfer proof receivers must never terminate")
                        }
                    }
                }
                Some(response_channel) = self.inflight_encrypted_signatures.next() => {
                    let _ = self.swarm.behaviour_mut().encrypted_signature.send_response(response_channel, ());
                }
            }
        }
    }

    async fn make_quote(
        &mut self,
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
    ) -> Result<BidQuote> {
        let rate = self
            .latest_rate
            .latest_rate()
            .context("Failed to get latest rate")?;

        Ok(BidQuote {
            price: rate.ask().context("Failed to compute asking price")?,
            min_quantity: min_buy,
            max_quantity: max_buy,
        })
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

        // TODO: Consider adding separate components for start/resume of swaps

        // swaps save peer id so we can resume
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
        // we deliberately don't put timeouts on these channels because the swap always
        // races these futures against a timelock

        let (transfer_proof_sender, mut transfer_proof_receiver) = bmrng::channel(1);
        let encrypted_signature = bmrng::channel(1);

        self.recv_encrypted_signature
            .insert(swap_id, encrypted_signature.0);

        self.send_transfer_proof.push(
            async move {
                let (transfer_proof, responder) = transfer_proof_receiver.recv().await?;

                let request = transfer_proof::Request {
                    swap_id,
                    tx_lock_proof: transfer_proof,
                };

                Ok((peer, request, responder))
            }
            .boxed(),
        );

        EventLoopHandle {
            recv_encrypted_signature: Some(encrypted_signature.1),
            send_transfer_proof: Some(transfer_proof_sender),
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
    recv_encrypted_signature: Option<bmrng::RequestReceiver<bitcoin::EncryptedSignature, ()>>,
    send_transfer_proof: Option<bmrng::RequestSender<monero::TransferProof, ()>>,
}

impl EventLoopHandle {
    pub async fn recv_encrypted_signature(&mut self) -> Result<bitcoin::EncryptedSignature> {
        let (tx_redeem_encsig, responder) = self
            .recv_encrypted_signature
            .take()
            .context("Encrypted signature was already received")?
            .recv()
            .await?;

        responder
            .respond(())
            .context("Failed to acknowledge receipt of encrypted signature")?;

        Ok(tx_redeem_encsig)
    }

    pub async fn send_transfer_proof(&mut self, msg: monero::TransferProof) -> Result<()> {
        self.send_transfer_proof
            .take()
            .context("Transfer proof was already sent")?
            .send_receive(msg)
            .await
            .context("Failed to send transfer proof")?;

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
