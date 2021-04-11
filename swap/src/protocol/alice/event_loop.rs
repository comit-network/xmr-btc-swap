use crate::asb::Rate;
use crate::database::Database;
use crate::env::Config;
use crate::network::quote::BidQuote;
use crate::network::{spot_price, transfer_proof};
use crate::protocol::alice::{AliceState, Behaviour, OutEvent, State0, Swap};
use crate::{bitcoin, env, kraken, monero};
use anyhow::{bail, Context, Result};
use future::pending;
use futures::future;
use futures::future::{BoxFuture, FutureExt};
use futures::stream::{FuturesUnordered, StreamExt};
use libp2p::request_response::{RequestId, ResponseChannel};
use libp2p::swarm::SwarmEvent;
use libp2p::{PeerId, Swarm};
use rand::rngs::OsRng;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::convert::Infallible;
use std::iter::FromIterator;
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

/// An async function that acts as an event loop to primarily drive the swarm
/// and interact with the rest of the application via the [`EventLoopHandle`].
///
/// This function will not return unless there is a fatal error that makes
/// further processing pointless.
#[allow(clippy::too_many_arguments)]
pub async fn new<LR>(
    mut swarm: Swarm<Behaviour>,
    env_config: Config,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    db: Arc<Database>,
    mut latest_rate: LR,
    max_buy: bitcoin::Amount,
    swap_sender: mpsc::Sender<Swap>,
) where
    LR: LatestRate,
{
    let unfinished_swaps = match db.unfinished_alice() {
        Ok(unfinished_swaps) => unfinished_swaps,
        Err(_) => {
            tracing::error!("Failed to load unfinished swaps");
            return;
        }
    };

    let mut state = State::default();

    for (swap_id, swap_state) in unfinished_swaps {
        let peer_id = match db.get_peer_id(swap_id) {
            Ok(peer_id) => peer_id,
            Err(_) => {
                tracing::warn!(%swap_id, "Resuming swap skipped because no peer-id found for swap in database");
                continue;
            }
        };

        let handle = EventLoopHandle::new(
            &mut state.recv_encrypted_signature,
            &mut state.send_transfer_proof,
            peer_id,
            swap_id,
        );

        let swap = Swap {
            event_loop_handle: handle,
            bitcoin_wallet: bitcoin_wallet.clone(),
            monero_wallet: monero_wallet.clone(),
            env_config,
            db: db.clone(),
            state: swap_state.into(),
            swap_id,
        };

        match swap_sender.send(swap).await {
            Ok(_) => tracing::info!(%swap_id, "Resuming swap"),
            Err(_) => {
                tracing::warn!(%swap_id, "Failed to resume swap because receiver has been dropped")
            }
        }
    }

    loop {
        tokio::select! {
            swarm_event = swarm.next_event() => {
                match swarm_event {
                    SwarmEvent::Behaviour(OutEvent::SpotPriceRequested { request: spot_price::Request { btc }, channel, peer }) => {
                        if let Err(e) = handle_spot_price_request(&mut latest_rate, &mut swarm, monero_wallet.as_ref(), bitcoin_wallet.as_ref(), env_config, channel, peer, btc, max_buy).await {
                            tracing::warn!(%peer, "Failed to handle spot price request for {}: {:#}", btc, e);
                        };
                    }
                    SwarmEvent::Behaviour(OutEvent::QuoteRequested { channel, peer }) => {
                        if let Err(e) = handle_quote_request(&mut latest_rate, &mut swarm, channel, max_buy).await {
                            tracing::warn!(%peer, "Failed to handle quote request: {:#}", e);
                        };
                    }
                    SwarmEvent::Behaviour(OutEvent::ExecutionSetupDone { bob_peer_id, state3, swap_id }) => {
                        let swap = Swap {
                            event_loop_handle: EventLoopHandle::new(&mut state.recv_encrypted_signature, &mut state.send_transfer_proof, bob_peer_id, swap_id),
                            bitcoin_wallet: bitcoin_wallet.clone(),
                            monero_wallet: monero_wallet.clone(),
                            env_config,
                            db: db.clone(),
                            state: AliceState::Started { state3 },
                            swap_id,
                        };

                        // swaps save peer id so we can resume
                        match db.insert_peer_id(swap_id, bob_peer_id).await {
                            Ok(_) => {
                                if let Err(error) = swap_sender.send(swap).await {
                                    tracing::warn!(%swap_id, "Swap cannot be spawned: {}", error);
                                }
                            }
                            Err(error) => {
                                tracing::warn!(%swap_id, "Unable to save peer-id, swap cannot be spawned: {}", error);
                            }
                        }
                    }
                    SwarmEvent::Behaviour(OutEvent::TransferProofAcknowledged { peer, id }) => {
                        tracing::debug!(%peer, "Bob acknowledged transfer proof");
                        if let Some(responder) = state.inflight_transfer_proofs.remove(&id) {
                            let _ = responder.respond(());
                        }
                    }
                    SwarmEvent::Behaviour(OutEvent::EncryptedSignatureReceived { msg, channel, peer }) => {
                        let sender = match state.recv_encrypted_signature.remove(&msg.swap_id) {
                            Some(sender) => sender,
                            None => {
                                    // TODO: Don't just drop encsig if we currently don't have a running swap for it, save in db
                                tracing::warn!(%peer, "No sender for encrypted signature, maybe already handled?");
                                continue;
                            }
                        };

                        let mut responder = match sender.send(msg.tx_redeem_encsig).await {
                            Ok(responder) => responder,
                            Err(_) => {
                                tracing::warn!(%peer, "Failed to relay encrypted signature to swap");
                                continue;
                            }
                        };

                        state.inflight_encrypted_signatures.push(async move {
                            let _ = responder.recv().await;

                            channel
                        }.boxed());
                    }
                    SwarmEvent::Behaviour(OutEvent::ResponseSent) => {}
                    SwarmEvent::Behaviour(OutEvent::Failure {peer, error}) => {
                        tracing::error!(%peer, "Communication error: {:#}", error);
                    }
                    SwarmEvent::ConnectionEstablished { peer_id: peer, endpoint, .. } => {
                        tracing::debug!(%peer, address = %endpoint.get_remote_address(), "New connection established");

                        if let Some(transfer_proofs) = state.buffered_transfer_proofs.remove(&peer) {
                                for (transfer_proof, responder) in transfer_proofs {
                            tracing::debug!(%peer, "Found buffered transfer proof for peer");

                            let id = swarm.transfer_proof.send_request(&peer, transfer_proof);
                            state.inflight_transfer_proofs.insert(id, responder);
                        }
                    }
                    }SwarmEvent::IncomingConnectionError { send_back_addr: address, error, .. } => {
                        tracing::warn!(%address, "Failed to set up connection with peer: {}", error);
                    }
                    SwarmEvent::ConnectionClosed { peer_id: peer, num_established, endpoint, cause } if num_established == 0 => {
                        match cause {
                            Some(error) => {
                                tracing::warn!(%peer, address = %endpoint.get_remote_address(), "Lost connection: {}", error);
                            },
                            None => {
                                tracing::info!(%peer, address = %endpoint.get_remote_address(), "Successfully closed connection");
                            }
                        }
                    }
                    _ => {}
                }
            },
            next_transfer_proof = state.send_transfer_proof.next() => {
                match next_transfer_proof {
                    Some(Ok((peer, transfer_proof, responder))) => {
                        if !swarm.transfer_proof.is_connected(&peer) {
                            tracing::warn!(%peer, "No active connection to peer, buffering transfer proof");
                            state.buffered_transfer_proofs.entry(peer).or_insert_with(Vec::new).push( (transfer_proof, responder));
                            continue;
                        }

                        let id = swarm.transfer_proof.send_request(&peer, transfer_proof);
                        state.inflight_transfer_proofs.insert(id, responder);
                    },
                    Some(Err(e)) => {
                        tracing::debug!("A swap stopped without sending a transfer proof: {:#}", e);
                    }
                    None => {
                        unreachable!("stream of transfer proof receivers must never terminate")
                    }
                }
            }
            Some(response_channel) = state.inflight_encrypted_signatures.next() => {
                let _ = swarm.encrypted_signature.send_response(response_channel, ());
            }
        }
    }
}

/// Holds the event-loop state.
///
/// All fields within this struct could also just be local variables in the
/// event loop. Bundling them up in a struct allows us to add documentation to
/// some of the fields which makes it clearer what they are used for.
struct State {
    /// Tracks the [`bmrng::RequestSender`]s for peers from which we expect to
    /// be sent an encrypted signature.
    recv_encrypted_signature: HashMap<Uuid, bmrng::RequestSender<bitcoin::EncryptedSignature, ()>>,
    /// Tracks [`transfer_proof::Request`]s which could not yet be sent because
    /// we are currently disconnected from the peer.
    buffered_transfer_proofs: HashMap<PeerId, Vec<(transfer_proof::Request, bmrng::Responder<()>)>>,
    /// Tracks [`transfer_proof::Request`]s which are currently inflight and
    /// awaiting an acknowledgement.
    inflight_transfer_proofs: HashMap<RequestId, bmrng::Responder<()>>,

    inflight_encrypted_signatures: FuturesUnordered<BoxFuture<'static, ResponseChannel<()>>>,
    send_transfer_proof: FuturesUnordered<OutgoingTransferProof>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            recv_encrypted_signature: Default::default(),
            buffered_transfer_proofs: Default::default(),
            inflight_transfer_proofs: Default::default(),
            inflight_encrypted_signatures: never_completing_futures_unordered(),
            send_transfer_proof: never_completing_futures_unordered(),
        }
    }
}

/// Constructs a new instance of [`FuturesUnordered`] that will never fully
/// complete.
///
/// For our use of [`FuturesUnordered`] within the `select!` macro, it is
/// important that this stream never completes, otherwise it won't be polled
/// again later. As such, we add a [`pending()`] future to it. By design, this
/// future will never complete.
fn never_completing_futures_unordered<T>() -> FuturesUnordered<BoxFuture<'static, T>>
where
    T: Send + 'static,
{
    FuturesUnordered::from_iter(vec![pending().boxed()])
}

#[allow(clippy::too_many_arguments)]
async fn handle_spot_price_request<LR>(
    latest_rate: &mut LR,
    swarm: &mut Swarm<Behaviour>,
    monero_wallet: &monero::Wallet,
    bitcoin_wallet: &bitcoin::Wallet,
    env_config: env::Config,
    channel: ResponseChannel<spot_price::Response>,
    peer: PeerId,
    btc: bitcoin::Amount,
    max_buy: bitcoin::Amount,
) -> Result<()>
where
    LR: LatestRate,
{
    let rate = latest_rate
        .latest_rate()
        .context("Failed to get latest rate")?;

    if btc > max_buy {
        bail!(
            "Refusing to buy {} because the maximum configured limit is {}",
            btc,
            max_buy
        )
    }

    let xmr_balance = monero_wallet.get_balance().await?;
    let xmr_lock_fees = monero_wallet.static_tx_fee_estimate();
    let xmr = rate.sell_quote(btc)?;

    if xmr_balance < xmr + xmr_lock_fees {
        bail!("The balance is too low, current balance: {}", xmr_balance)
    }

    if swarm
        .spot_price
        .send_response(channel, spot_price::Response { xmr })
        .is_err()
    {
        bail!("Failed to respond with spot price")
    }

    let state0 = State0::new(btc, xmr, env_config, bitcoin_wallet, &mut OsRng)
        .await
        .context("Failed to make State0 for execution setup: {:#}")?;

    swarm.execution_setup.run(peer, state0);

    Ok(())
}

async fn handle_quote_request<LR>(
    latest_rate: &mut LR,
    swarm: &mut Swarm<Behaviour>,
    channel: ResponseChannel<BidQuote>,
    max_buy: bitcoin::Amount,
) -> Result<()>
where
    LR: LatestRate,
{
    let rate = latest_rate
        .latest_rate()
        .context("Failed to get latest rate")?;

    let quote = BidQuote {
        price: rate.ask().context("Failed to compute asking price")?,
        max_quantity: max_buy,
    };

    if swarm.quote.send_response(channel, quote).is_err() {
        bail!("Failed to respond with quote")
    }

    Ok(())
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
#[derive(Debug)]
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
    /// Create a new [`EventLoopHandle`] that is scoped for communication with
    /// the given peer.
    fn new(
        recv_encrypted_signature: &mut HashMap<
            Uuid,
            bmrng::RequestSender<bitcoin::EncryptedSignature, ()>,
        >,
        send_transfer_proof: &mut FuturesUnordered<OutgoingTransferProof>,
        peer: PeerId,
        swap_id: Uuid,
    ) -> Self {
        // we deliberately don't put timeouts on these channels because the swap always
        // races these futures against a timelock
        let (transfer_proof_sender, mut transfer_proof_receiver) = bmrng::channel(1);
        let encrypted_signature = bmrng::channel(1);

        recv_encrypted_signature.insert(swap_id, encrypted_signature.0);
        send_transfer_proof.push(
            async move {
                let (msg, responder) = transfer_proof_receiver.recv().await?;
                let request = transfer_proof::Request {
                    swap_id,
                    tx_lock_proof: msg,
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
