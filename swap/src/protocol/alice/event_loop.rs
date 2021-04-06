use crate::asb::Rate;
use crate::database::Database;
use crate::env::Config;
use crate::monero::BalanceTooLow;
use crate::network::quote::BidQuote;
use crate::network::{spot_price, transfer_proof};
use crate::protocol::alice::{AliceState, Behaviour, OutEvent, State0, Swap};
use crate::{bitcoin, kraken, monero};
use anyhow::{bail, Context, Result};
use futures::future;
use futures::future::{BoxFuture, FutureExt};
use futures::stream::{FuturesUnordered, StreamExt};
use libp2p::swarm::SwarmEvent;
use libp2p::{PeerId, Swarm};
use rand::rngs::OsRng;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::convert::Infallible;
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
    let mut recv_encrypted_signature = HashMap::new();
    let mut inflight_encrypted_signatures = FuturesUnordered::new();
    let mut send_transfer_proof = FuturesUnordered::new();

    // Tracks [`transfer_proof::Request`]s which could not yet be sent because
    // we are currently disconnected from the peer.
    let mut buffered_transfer_proofs = HashMap::new();

    // Tracks [`transfer_proof::Request`]s which are currently inflight and
    // awaiting an acknowledgement.
    let mut inflight_transfer_proofs = HashMap::<_, bmrng::Responder<()>>::new();

    // ensure that these streams are NEVER empty, otherwise it will
    // terminate forever.
    send_transfer_proof.push(future::pending().boxed());
    inflight_encrypted_signatures.push(future::pending().boxed());

    let unfinished_swaps = match db.unfinished_alice() {
        Ok(unfinished_swaps) => unfinished_swaps,
        Err(_) => {
            tracing::error!("Failed to load unfinished swaps");
            return;
        }
    };

    for (swap_id, state) in unfinished_swaps {
        let peer_id = match db.get_peer_id(swap_id) {
            Ok(peer_id) => peer_id,
            Err(_) => {
                tracing::warn!(%swap_id, "Resuming swap skipped because no peer-id found for swap in database");
                continue;
            }
        };

        let handle = new_handle(
            &mut recv_encrypted_signature,
            &mut send_transfer_proof,
            peer_id,
            swap_id,
        );

        let swap = Swap {
            event_loop_handle: handle,
            bitcoin_wallet: bitcoin_wallet.clone(),
            monero_wallet: monero_wallet.clone(),
            env_config,
            db: db.clone(),
            state: state.into(),
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
                    SwarmEvent::Behaviour(OutEvent::SpotPriceRequested { request, channel, peer }) => {
                        let btc = request.btc;
                        let xmr = match handle_spot_price_request(&mut latest_rate, btc, max_buy, monero_wallet.clone()).await {
                            Ok(xmr) => xmr,
                            Err(e) => {
                                tracing::warn!(%peer, "Failed to produce spot price for {}: {:#}", btc, e);
                                continue;
                            }
                        };

                        match swarm.spot_price.send_response(channel, spot_price::Response { xmr }) {
                            Ok(_) => {},
                            Err(_) => {
                                // if we can't respond, the peer probably just disconnected so it is not a huge deal, only log this on debug
                                tracing::debug!(%peer, "Failed to respond with spot price");
                                continue;
                            }
                        }

                        let state0 = match State0::new(btc, xmr, env_config, bitcoin_wallet.as_ref(), &mut OsRng).await {
                            Ok(state) => state,
                            Err(e) => {
                                tracing::warn!(%peer, "Failed to make State0 for execution setup: {:#}", e);
                                continue;
                            }
                        };

                        swarm.execution_setup.run(peer, state0);
                    }
                    SwarmEvent::Behaviour(OutEvent::QuoteRequested { channel, peer }) => {
                        let quote = match make_quote(&mut latest_rate, max_buy).await {
                            Ok(quote) => quote,
                            Err(e) => {
                                tracing::warn!(%peer, "Failed to make quote: {:#}", e);
                                continue;
                            }
                        };

                        if swarm.quote.send_response(channel, quote).is_err() {
                            tracing::debug!(%peer, "Failed to respond with quote");
                        }
                    }
                    SwarmEvent::Behaviour(OutEvent::ExecutionSetupDone { bob_peer_id, state3, swap_id }) => {
                        let swap = Swap {
                            event_loop_handle: new_handle(&mut recv_encrypted_signature, &mut send_transfer_proof, bob_peer_id, swap_id),
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
                        if let Some(responder) = inflight_transfer_proofs.remove(&id) {
                            let _ = responder.respond(());
                        }
                    }
                    SwarmEvent::Behaviour(OutEvent::EncryptedSignatureReceived { msg, channel, peer }) => {
                        let sender = match recv_encrypted_signature.remove(&msg.swap_id) {
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

                        inflight_encrypted_signatures.push(async move {
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

                        if let Some(transfer_proofs) = buffered_transfer_proofs.remove(&peer) {
                                for (transfer_proof, responder) in transfer_proofs {
                            tracing::debug!(%peer, "Found buffered transfer proof for peer");

                            let id = swarm.transfer_proof.send_request(&peer, transfer_proof);
                            inflight_transfer_proofs.insert(id, responder);
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
            next_transfer_proof = send_transfer_proof.next() => {
                match next_transfer_proof {
                    Some(Ok((peer, transfer_proof, responder))) => {
                        if !swarm.transfer_proof.is_connected(&peer) {
                            tracing::warn!(%peer, "No active connection to peer, buffering transfer proof");
                            buffered_transfer_proofs.entry(peer).or_insert_with(Vec::new).push( (transfer_proof, responder));
                            continue;
                        }

                        let id = swarm.transfer_proof.send_request(&peer, transfer_proof);
                        inflight_transfer_proofs.insert(id, responder);
                    },
                    Some(Err(e)) => {
                        tracing::debug!("A swap stopped without sending a transfer proof: {:#}", e);
                    }
                    None => {
                        unreachable!("stream of transfer proof receivers must never terminate")
                    }
                }
            }
            Some(response_channel) = inflight_encrypted_signatures.next() => {
                let _ = swarm.encrypted_signature.send_response(response_channel, ());
            }
        }
    }
}

async fn handle_spot_price_request<LR: LatestRate>(
    latest_rate: &mut LR,
    btc: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    monero_wallet: Arc<monero::Wallet>,
) -> Result<monero::Amount> {
    let rate = latest_rate
        .latest_rate()
        .context("Failed to get latest rate")?;

    if btc > max_buy {
        bail!(MaximumBuyAmountExceeded {
            actual: btc,
            max: max_buy
        })
    }

    let xmr_balance = monero_wallet.get_balance().await?;
    let xmr_lock_fees = monero_wallet.static_tx_fee_estimate();
    let xmr = rate.sell_quote(btc)?;

    if xmr_balance < xmr + xmr_lock_fees {
        bail!(BalanceTooLow {
            balance: xmr_balance
        })
    }

    Ok(xmr)
}

async fn make_quote<LR: LatestRate>(
    latest_rate: &mut LR,
    max_buy: bitcoin::Amount,
) -> Result<BidQuote> {
    let rate = latest_rate
        .latest_rate()
        .context("Failed to get latest rate")?;

    Ok(BidQuote {
        price: rate.ask().context("Failed to compute asking price")?,
        max_quantity: max_buy,
    })
}

/// Create a new [`EventLoopHandle`] that is scoped for communication with
/// the given peer.
fn new_handle(
    recv_encrypted_signature: &mut HashMap<
        Uuid,
        bmrng::RequestSender<bitcoin::EncryptedSignature, ()>,
    >,
    send_transfer_proof: &mut FuturesUnordered<OutgoingTransferProof>,
    peer: PeerId,
    swap_id: Uuid,
) -> EventLoopHandle {
    // we deliberately don't put timeouts on these channels because the swap always
    // races these futures against a timelock
    let (transfer_proof_sender, mut transfer_proof_receiver) = bmrng::channel(1);
    let encrypted_signature = bmrng::channel(1);

    recv_encrypted_signature.insert(swap_id, encrypted_signature.0);

    send_transfer_proof.push(
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

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("Refusing to buy {actual} because the maximum configured limit is {max}")]
pub struct MaximumBuyAmountExceeded {
    pub max: bitcoin::Amount,
    pub actual: bitcoin::Amount,
}
