use crate::asb::{FixedRate, Rate};
use crate::database::Database;
use crate::env::Config;
use crate::monero::BalanceTooLow;
use crate::network::quote::BidQuote;
use crate::network::{encrypted_signature, spot_price, transfer_proof};
use crate::protocol::alice::{AliceState, Behaviour, OutEvent, State3, Swap};
use crate::{bitcoin, kraken, monero};
use anyhow::{bail, Context, Result};
use futures::future;
use futures::future::{BoxFuture, FutureExt};
use futures::stream::{FuturesUnordered, StreamExt};
use libp2p::swarm::SwarmEvent;
use libp2p::{PeerId, Swarm};
use rand::rngs::OsRng;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, trace};
use uuid::Uuid;

#[allow(missing_debug_implementations)]
pub struct EventLoop<RS> {
    swarm: libp2p::Swarm<Behaviour>,
    env_config: Config,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    db: Arc<Database>,
    latest_rate: RS,
    max_buy: bitcoin::Amount,

    /// Stores a sender per peer for incoming [`EncryptedSignature`]s.
    recv_encrypted_signature: HashMap<PeerId, oneshot::Sender<encrypted_signature::Request>>,
    /// Stores a list of futures, waiting for transfer proof which will be sent
    /// to the given peer.
    send_transfer_proof:
        FuturesUnordered<BoxFuture<'static, Result<(PeerId, transfer_proof::Request)>>>,

    swap_sender: mpsc::Sender<Swap>,

    /// Tracks [`transfer_proof::Request`]s which could not yet be sent because
    /// we are currently disconnected from the peer.
    buffered_transfer_proofs: HashMap<PeerId, transfer_proof::Request>,
}

impl<LR> EventLoop<LR>
where
    LR: LatestRate,
{
    pub fn new(
        swarm: Swarm<Behaviour>,
        env_config: Config,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        db: Arc<Database>,
        latest_rate: LR,
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
            max_buy,
            recv_encrypted_signature: Default::default(),
            send_transfer_proof: Default::default(),
            buffered_transfer_proofs: Default::default(),
        };
        Ok((event_loop, swap_channel.receiver))
    }

    pub fn peer_id(&self) -> PeerId {
        *Swarm::local_peer_id(&self.swarm)
    }

    pub async fn run(mut self) {
        // ensure that the send_transfer_proof stream is NEVER empty, otherwise it will
        // terminate forever.
        self.send_transfer_proof.push(future::pending().boxed());

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

            let handle = self.new_handle(peer_id);

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
                swarm_event = self.swarm.next_event() => {
                    match swarm_event {
                        SwarmEvent::Behaviour(OutEvent::SpotPriceRequested { request, channel, peer }) => {
                            let btc = request.btc;
                            let xmr = match self.handle_spot_price_request(btc, self.monero_wallet.clone()).await {
                                Ok(xmr) => xmr,
                                Err(e) => {
                                    tracing::warn!(%peer, "failed to produce spot price for {}: {:#}", btc, e);
                                    continue;
                                }
                            };

                            match self.swarm.send_spot_price(channel, spot_price::Response { xmr }) {
                                Ok(_) => {},
                                Err(e) => {
                                    // if we can't respond, the peer probably just disconnected so it is not a huge deal, only log this on debug
                                    debug!(%peer, "failed to respond with spot price: {:#}", e);
                                    continue;
                                }
                            }

                            match self.swarm.start_execution_setup(peer, btc, xmr, self.env_config, self.bitcoin_wallet.as_ref(), &mut OsRng).await {
                                Ok(_) => {},
                                Err(e) => {
                                    tracing::warn!(%peer, "failed to start execution setup: {:#}", e);
                                }
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::QuoteRequested { channel, peer }) => {
                            let quote = match self.make_quote(self.max_buy).await {
                                Ok(quote) => quote,
                                Err(e) => {
                                    tracing::warn!(%peer, "failed to make quote: {:#}", e);
                                    continue;
                                }
                            };

                            match self.swarm.send_quote(channel, quote) {
                                Ok(_) => {},
                                Err(e) => {
                                    // if we can't respond, the peer probably just disconnected so it is not a huge deal, only log this on debug
                                    debug!(%peer, "failed to respond with quote: {:#}", e);
                                    continue;
                                }
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::ExecutionSetupDone{bob_peer_id, state3}) => {
                            let _ = self.handle_execution_setup_done(bob_peer_id, *state3).await;
                        }
                        SwarmEvent::Behaviour(OutEvent::TransferProofAcknowledged(peer)) => {
                            trace!(%peer, "Bob acknowledged transfer proof");
                        }
                        SwarmEvent::Behaviour(OutEvent::EncryptedSignatureReceived{ msg, channel, peer }) => {
                            match self.recv_encrypted_signature.remove(&peer) {
                                Some(sender) => {
                                    // this failing just means the receiver is no longer interested ...
                                    let _ = sender.send(*msg);
                                },
                                None => {
                                    tracing::warn!(%peer, "No sender for encrypted signature, maybe already handled?")
                                }
                            }

                            self.swarm.send_encrypted_signature_ack(channel);
                        }
                        SwarmEvent::Behaviour(OutEvent::ResponseSent) => {}
                        SwarmEvent::Behaviour(OutEvent::Failure {peer, error}) => {
                            error!(%peer, "Communication error: {:#}", error);
                        }
                        SwarmEvent::ConnectionEstablished { peer_id: peer, endpoint, .. } => {
                            tracing::debug!(%peer, address = %endpoint.get_remote_address(), "New connection established");

                            if let Some(transfer_proof) = self.buffered_transfer_proofs.remove(&peer) {
                                tracing::debug!(%peer, "Found buffered transfer proof for peer");

                                self.swarm
                                    .send_transfer_proof(peer, transfer_proof)
                                    .expect("must be able to send transfer proof after connection was established");
                            }
                        }
                        SwarmEvent::IncomingConnectionError { send_back_addr: address, error, .. } => {
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
                next_transfer_proof = self.send_transfer_proof.next() => {
                    match next_transfer_proof {
                        Some(Ok((peer, transfer_proof))) => {
                            let result = self.swarm.send_transfer_proof(peer, transfer_proof);

                            if let Err(transfer_proof) = result {
                                tracing::warn!(%peer, "No active connection to peer, buffering transfer proof");
                                self.buffered_transfer_proofs.insert(peer, transfer_proof);
                            }
                        },
                        Some(Err(_)) => {
                            tracing::debug!("A swap stopped without sending a transfer proof");
                        }
                        None => {
                            unreachable!("stream of transfer proof receivers must never terminate")
                        }
                    }
                }
            }
        }
    }

    async fn handle_spot_price_request(
        &mut self,
        btc: bitcoin::Amount,
        monero_wallet: Arc<monero::Wallet>,
    ) -> Result<monero::Amount> {
        let rate = self
            .latest_rate
            .latest_rate()
            .context("Failed to get latest rate")?;

        if btc > self.max_buy {
            bail!(MaximumBuyAmountExceeded {
                actual: btc,
                max: self.max_buy
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

    async fn make_quote(&mut self, max_buy: bitcoin::Amount) -> Result<BidQuote> {
        let rate = self
            .latest_rate
            .latest_rate()
            .context("Failed to get latest rate")?;

        Ok(BidQuote {
            price: rate.ask,
            max_quantity: max_buy,
        })
    }

    async fn handle_execution_setup_done(&mut self, bob_peer_id: PeerId, state3: State3) {
        let swap_id = Uuid::new_v4();
        let handle = self.new_handle(bob_peer_id);

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

        // TODO: Consider adding separate components for start/rsume of swaps

        // swaps save peer id so we can resume
        match self.db.insert_peer_id(swap_id, bob_peer_id).await {
            Ok(_) => {
                if let Err(error) = self.swap_sender.send(swap).await {
                    tracing::warn!(%swap_id, "Swap cannot be spawned: {}", error);
                }
            }
            Err(error) => {
                tracing::warn!(%swap_id, "Unable to save peer-id, swap cannot be spawned: {}", error);
            }
        }
    }

    /// Create a new [`EventLoopHandle`] that is scoped for communication with
    /// the given peer.
    fn new_handle(&mut self, peer: PeerId) -> EventLoopHandle {
        let (send_transfer_proof_sender, send_transfer_proof_receiver) = oneshot::channel();
        let (recv_enc_sig_sender, recv_enc_sig_receiver) = oneshot::channel();

        self.recv_encrypted_signature
            .insert(peer, recv_enc_sig_sender);
        self.send_transfer_proof.push(
            async move {
                let transfer_proof = send_transfer_proof_receiver.await?;

                Ok((peer, transfer_proof))
            }
            .boxed(),
        );

        EventLoopHandle {
            recv_encrypted_signature: Some(recv_enc_sig_receiver),
            send_transfer_proof: Some(send_transfer_proof_sender),
        }
    }
}

pub trait LatestRate {
    type Error: std::error::Error + Send + Sync + 'static;

    fn latest_rate(&mut self) -> Result<Rate, Self::Error>;
}

impl LatestRate for FixedRate {
    type Error = Infallible;

    fn latest_rate(&mut self) -> Result<Rate, Self::Error> {
        Ok(self.value())
    }
}

impl LatestRate for kraken::RateUpdateStream {
    type Error = kraken::Error;

    fn latest_rate(&mut self) -> Result<Rate, Self::Error> {
        self.latest_update()
    }
}

#[derive(Debug)]
pub struct EventLoopHandle {
    recv_encrypted_signature: Option<oneshot::Receiver<encrypted_signature::Request>>,
    send_transfer_proof: Option<oneshot::Sender<transfer_proof::Request>>,
}

impl EventLoopHandle {
    pub async fn recv_encrypted_signature(&mut self) -> Result<bitcoin::EncryptedSignature> {
        let signature = self
            .recv_encrypted_signature
            .take()
            .context("Encrypted signature was already received")?
            .await?
            .tx_redeem_encsig;

        Ok(signature)
    }

    pub async fn send_transfer_proof(&mut self, msg: monero::TransferProof) -> Result<()> {
        if self
            .send_transfer_proof
            .take()
            .context("Transfer proof was already sent")?
            .send(transfer_proof::Request { tx_lock_proof: msg })
            .is_err()
        {
            bail!("Failed to send transfer proof, receiver no longer listening?")
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("Refusing to buy {actual} because the maximum configured limit is {max}")]
pub struct MaximumBuyAmountExceeded {
    pub max: bitcoin::Amount,
    pub actual: bitcoin::Amount,
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
