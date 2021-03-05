use crate::asb::{FixedRate, Rate};
use crate::database::Database;
use crate::execution_params::ExecutionParams;
use crate::monero::BalanceTooLow;
use crate::network::quote::BidQuote;
use crate::network::{spot_price, transport, TokioExecutor};
use crate::protocol::alice;
use crate::protocol::alice::{AliceState, Behaviour, OutEvent, State3, Swap, TransferProof};
use crate::protocol::bob::EncryptedSignature;
use crate::seed::Seed;
use crate::{bitcoin, kraken, monero};
use anyhow::{bail, Context, Result};
use futures::future::RemoteHandle;
use libp2p::core::Multiaddr;
use libp2p::futures::FutureExt;
use libp2p::{PeerId, Swarm};
use rand::rngs::OsRng;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, trace};
use uuid::Uuid;

#[allow(missing_debug_implementations)]
pub struct EventLoop<RS> {
    swarm: libp2p::Swarm<Behaviour>,
    peer_id: PeerId,
    execution_params: ExecutionParams,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    db: Arc<Database>,
    latest_rate: RS,
    max_buy: bitcoin::Amount,

    recv_encrypted_signature: broadcast::Sender<EncryptedSignature>,
    send_transfer_proof: mpsc::Receiver<(PeerId, TransferProof)>,

    // Only used to produce new handles
    send_transfer_proof_sender: mpsc::Sender<(PeerId, TransferProof)>,

    swap_handle_sender: mpsc::Sender<RemoteHandle<Result<AliceState>>>,
}

#[derive(Debug)]
pub struct EventLoopHandle {
    recv_encrypted_signature: broadcast::Receiver<EncryptedSignature>,
    send_transfer_proof: mpsc::Sender<(PeerId, TransferProof)>,
}

impl<LR> EventLoop<LR>
where
    LR: LatestRate,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        listen_address: Multiaddr,
        seed: Seed,
        execution_params: ExecutionParams,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        db: Arc<Database>,
        latest_rate: LR,
        max_buy: bitcoin::Amount,
    ) -> Result<(Self, mpsc::Receiver<RemoteHandle<Result<AliceState>>>)> {
        let identity = seed.derive_libp2p_identity();
        let behaviour = Behaviour::default();
        let transport = transport::build(&identity)?;
        let peer_id = PeerId::from(identity.public());

        let mut swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, peer_id)
            .executor(Box::new(TokioExecutor {
                handle: tokio::runtime::Handle::current(),
            }))
            .build();

        Swarm::listen_on(&mut swarm, listen_address.clone())
            .with_context(|| format!("Address is not supported: {:#}", listen_address))?;

        let recv_encrypted_signature = BroadcastChannels::default();
        let send_transfer_proof = MpscChannels::default();
        let swap_handle = MpscChannels::default();

        let event_loop = EventLoop {
            swarm,
            peer_id,
            execution_params,
            bitcoin_wallet,
            monero_wallet,
            db,
            latest_rate,
            recv_encrypted_signature: recv_encrypted_signature.sender,
            send_transfer_proof: send_transfer_proof.receiver,
            send_transfer_proof_sender: send_transfer_proof.sender,
            swap_handle_sender: swap_handle.sender,
            max_buy,
        };
        Ok((event_loop, swap_handle.receiver))
    }

    pub fn new_handle(&self) -> EventLoopHandle {
        EventLoopHandle {
            recv_encrypted_signature: self.recv_encrypted_signature.subscribe(),
            send_transfer_proof: self.send_transfer_proof_sender.clone(),
        }
    }

    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                swarm_event = self.swarm.next().fuse() => {
                    match swarm_event {
                        OutEvent::ConnectionEstablished(alice) => {
                            debug!("Connection Established with {}", alice);
                        }
                        OutEvent::SpotPriceRequested { msg, channel, peer } => {
                            let btc = msg.btc;
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

                            match self.swarm.start_execution_setup(peer, btc, xmr, self.execution_params, self.bitcoin_wallet.as_ref(), &mut OsRng).await {
                                Ok(_) => {},
                                Err(e) => {
                                    tracing::warn!(%peer, "failed to start execution setup: {:#}", e);
                                }
                            }
                        }
                        OutEvent::QuoteRequested { channel, peer } => {
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
                        OutEvent::ExecutionSetupDone{bob_peer_id, state3} => {
                            let _ = self.handle_execution_setup_done(bob_peer_id, *state3).await;
                        }
                        OutEvent::TransferProofAcknowledged => {
                            trace!("Bob acknowledged transfer proof");
                        }
                        OutEvent::EncryptedSignature{ msg, channel } => {
                            let _ = self.recv_encrypted_signature.send(*msg);
                            // Send back empty response so that the request/response protocol completes.
                            if let Err(error) = self.swarm.send_encrypted_signature_ack(channel) {
                                error!("Failed to send Encrypted Signature ack: {:?}", error);
                            }
                        }
                        OutEvent::ResponseSent => {}
                        OutEvent::Failure(err) => {
                            error!("Communication error: {:#}", err);
                        }
                    }
                },
                transfer_proof = self.send_transfer_proof.recv().fuse() => {
                    if let Some((bob_peer_id, msg)) = transfer_proof  {
                      self.swarm.send_transfer_proof(bob_peer_id, msg);
                    }
                },
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

    async fn handle_execution_setup_done(
        &mut self,
        bob_peer_id: PeerId,
        state3: State3,
    ) -> Result<()> {
        let swap_id = Uuid::new_v4();
        let handle = self.new_handle();

        let initial_state = AliceState::Started {
            state3: Box::new(state3),
            bob_peer_id,
        };

        let swap = Swap {
            event_loop_handle: handle,
            bitcoin_wallet: self.bitcoin_wallet.clone(),
            monero_wallet: self.monero_wallet.clone(),
            execution_params: self.execution_params,
            db: self.db.clone(),
            state: initial_state,
            swap_id,
        };

        let (swap, swap_handle) = alice::run(swap).remote_handle();
        tokio::spawn(swap);

        // For testing purposes the handle is currently sent via a channel so we can
        // await it. If a remote handle is dropped, the future of the swap is
        // also stopped. If we error upon sending the handle through the channel
        // we have to call forget to detach the handle from the swap future.
        if let Err(SendError(handle)) = self.swap_handle_sender.send(swap_handle).await {
            handle.forget();
        }

        Ok(())
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

impl EventLoopHandle {
    pub async fn recv_encrypted_signature(&mut self) -> Result<EncryptedSignature> {
        self.recv_encrypted_signature
            .recv()
            .await
            .context("Failed to receive Bitcoin encrypted signature from Bob")
    }
    pub async fn send_transfer_proof(&mut self, bob: PeerId, msg: TransferProof) -> Result<()> {
        let _ = self.send_transfer_proof.send((bob, msg)).await?;

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

#[allow(missing_debug_implementations)]
struct BroadcastChannels<T>
where
    T: Clone,
{
    sender: broadcast::Sender<T>,
}

impl<T> Default for BroadcastChannels<T>
where
    T: Clone,
{
    fn default() -> Self {
        let (sender, _receiver) = broadcast::channel(100);
        BroadcastChannels { sender }
    }
}
