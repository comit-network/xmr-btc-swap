use crate::{
    asb::LatestRate,
    bitcoin,
    database::Database,
    execution_params::ExecutionParams,
    monero,
    monero::{Amount, BalanceTooLow},
    network::{transport, TokioExecutor},
    protocol::{
        alice,
        alice::{
            AliceState, Behaviour, OutEvent, QuoteResponse, State0, State3, Swap, TransferProof,
        },
        bob::{EncryptedSignature, QuoteRequest},
    },
    seed::Seed,
};
use anyhow::{bail, Context, Result};
use futures::future::RemoteHandle;
use libp2p::{
    core::Multiaddr, futures::FutureExt, request_response::ResponseChannel, PeerId, Swarm,
};
use rand::rngs::OsRng;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, mpsc::error::SendError};
use tracing::{debug, error, info, trace};
use uuid::Uuid;

#[allow(missing_debug_implementations)]
pub struct MpscChannels<T> {
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
pub struct BroadcastChannels<T>
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

#[derive(Debug)]
pub struct EventLoopHandle {
    recv_encrypted_signature: broadcast::Receiver<EncryptedSignature>,
    send_transfer_proof: mpsc::Sender<(PeerId, TransferProof)>,
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

#[allow(missing_debug_implementations)]
pub struct EventLoop<RS> {
    swarm: libp2p::Swarm<Behaviour>,
    peer_id: PeerId,
    execution_params: ExecutionParams,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    db: Arc<Database>,
    rate_service: RS,
    max_sell: Amount,

    recv_encrypted_signature: broadcast::Sender<EncryptedSignature>,
    send_transfer_proof: mpsc::Receiver<(PeerId, TransferProof)>,

    // Only used to produce new handles
    send_transfer_proof_sender: mpsc::Sender<(PeerId, TransferProof)>,

    swap_handle_sender: mpsc::Sender<RemoteHandle<Result<AliceState>>>,
}

impl<RS> EventLoop<RS>
where
    RS: LatestRate,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        listen_address: Multiaddr,
        seed: Seed,
        execution_params: ExecutionParams,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        db: Arc<Database>,
        rate_service: RS,
        max_sell: Amount,
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
            rate_service,
            recv_encrypted_signature: recv_encrypted_signature.sender,
            send_transfer_proof: send_transfer_proof.receiver,
            send_transfer_proof_sender: send_transfer_proof.sender,
            swap_handle_sender: swap_handle.sender,
            max_sell,
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
                        OutEvent::QuoteRequest { msg, channel, bob_peer_id } => {
                            if let Err(error) = self.handle_quote_request(msg, channel, bob_peer_id, self.monero_wallet.clone()).await {
                                error!("Failed to handle quote request: {:#}", error);
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

    async fn handle_quote_request(
        &mut self,
        quote_request: QuoteRequest,
        channel: ResponseChannel<QuoteResponse>,
        bob_peer_id: PeerId,
        monero_wallet: Arc<monero::Wallet>,
    ) -> Result<()> {
        // 1. Check if acceptable request
        // 2. Send response

        let rate = self
            .rate_service
            .latest_rate()
            .context("Failed to get latest rate")?;

        let btc_amount = quote_request.btc_amount;
        let xmr_amount = rate.sell_quote(btc_amount)?;

        if xmr_amount > self.max_sell {
            bail!(MaximumSellAmountExceeded {
                actual: xmr_amount,
                max_sell: self.max_sell
            })
        }

        let xmr_balance = monero_wallet.get_balance().await?;
        let xmr_lock_fees = monero_wallet.static_tx_fee_estimate();

        if xmr_balance < xmr_amount + xmr_lock_fees {
            bail!(BalanceTooLow {
                balance: xmr_balance
            })
        }

        let quote_response = QuoteResponse { xmr_amount };

        self.swarm
            .send_quote_response(channel, quote_response)
            .context("Failed to send quote response")?;

        // 3. Start setup execution

        let state0 = State0::new(
            btc_amount,
            xmr_amount,
            self.execution_params,
            self.bitcoin_wallet.as_ref(),
            &mut OsRng,
        )
        .await?;

        info!(
            "Starting execution setup to sell {} for {} (rate of {}) with {}",
            xmr_amount, btc_amount, rate, bob_peer_id
        );

        self.swarm.start_execution_setup(bob_peer_id, state0);
        // Continues once the execution setup protocol is done
        Ok(())
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

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("The amount {actual} exceeds the configured maximum sell amount of {max_sell} XMR")]
pub struct MaximumSellAmountExceeded {
    pub max_sell: Amount,
    pub actual: Amount,
}
