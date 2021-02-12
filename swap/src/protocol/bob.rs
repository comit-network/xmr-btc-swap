//! Run an XMR/BTC swap in the role of Bob.
//! Bob holds BTC and wishes receive XMR.
use crate::{
    bitcoin, database,
    database::Database,
    execution_params::ExecutionParams,
    monero, network,
    network::{
        peer_tracker::{self, PeerTracker},
        transport::build,
    },
    protocol::{alice, alice::TransferProof, bob, SwapAmounts},
    seed::Seed,
};
use anyhow::{bail, Error, Result};
use libp2p::{core::Multiaddr, identity::Keypair, NetworkBehaviour, PeerId};
use rand::rngs::OsRng;
use std::{path::PathBuf, sync::Arc};
use tracing::{debug, info};
use uuid::Uuid;

pub use self::{
    cancel::cancel,
    encrypted_signature::EncryptedSignature,
    event_loop::{EventLoop, EventLoopHandle},
    quote_request::*,
    refund::refund,
    state::*,
    swap::{run, run_until},
};
pub use execution_setup::{Message0, Message2, Message4};
use libp2p::request_response::ResponseChannel;

pub mod cancel;
mod encrypted_signature;
pub mod event_loop;
mod execution_setup;
mod quote_request;
pub mod refund;
pub mod state;
pub mod swap;
mod transfer_proof;

pub struct Swap {
    pub state: BobState,
    pub event_loop_handle: bob::EventLoopHandle,
    pub db: Database,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub execution_params: ExecutionParams,
    pub swap_id: Uuid,
}

pub struct Builder {
    swap_id: Uuid,
    identity: Keypair,
    peer_id: PeerId,
    db_path: PathBuf,

    alice_address: Multiaddr,
    alice_peer_id: PeerId,

    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,

    init_params: InitParams,
    execution_params: ExecutionParams,
}

enum InitParams {
    None,
    New { swap_amounts: SwapAmounts },
}

impl Builder {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        seed: Seed,
        db_path: PathBuf,
        swap_id: Uuid,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        alice_address: Multiaddr,
        alice_peer_id: PeerId,
        execution_params: ExecutionParams,
    ) -> Self {
        let identity = network::Seed::new(seed).derive_libp2p_identity();
        let peer_id = identity.public().into_peer_id();

        Self {
            swap_id,
            identity,
            peer_id,
            db_path,
            alice_address,
            alice_peer_id,
            bitcoin_wallet,
            monero_wallet,
            init_params: InitParams::None,
            execution_params,
        }
    }

    pub fn with_init_params(self, swap_amounts: SwapAmounts) -> Self {
        Self {
            init_params: InitParams::New { swap_amounts },
            ..self
        }
    }

    pub async fn build(self) -> Result<(bob::Swap, bob::EventLoop)> {
        match self.init_params {
            InitParams::New { swap_amounts } => {
                let initial_state = self
                    .make_initial_state(swap_amounts.btc, swap_amounts.xmr, self.execution_params)
                    .await?;

                let (event_loop, event_loop_handle) = self.init_event_loop()?;

                let db = Database::open(self.db_path.as_path())?;

                Ok((
                    Swap {
                        state: initial_state,
                        event_loop_handle,
                        db,
                        bitcoin_wallet: self.bitcoin_wallet.clone(),
                        monero_wallet: self.monero_wallet.clone(),
                        swap_id: self.swap_id,
                        execution_params: self.execution_params,
                    },
                    event_loop,
                ))
            }

            InitParams::None => {
                // reopen the existing database
                let db = Database::open(self.db_path.as_path())?;

                let resume_state = if let database::Swap::Bob(state) = db.get_state(self.swap_id)? {
                    state.into()
                } else {
                    bail!(
                        "Trying to load swap with id {} for the wrong direction.",
                        self.swap_id
                    )
                };

                let (event_loop, event_loop_handle) = self.init_event_loop()?;

                Ok((
                    Swap {
                        state: resume_state,
                        event_loop_handle,
                        db,
                        bitcoin_wallet: self.bitcoin_wallet.clone(),
                        monero_wallet: self.monero_wallet.clone(),
                        swap_id: self.swap_id,
                        execution_params: self.execution_params,
                    },
                    event_loop,
                ))
            }
        }
    }

    fn init_event_loop(
        &self,
    ) -> Result<(bob::event_loop::EventLoop, bob::event_loop::EventLoopHandle)> {
        let bob_behaviour = bob::Behaviour::default();
        let bob_transport = build(&self.identity)?;

        bob::event_loop::EventLoop::new(
            bob_transport,
            bob_behaviour,
            self.peer_id,
            self.alice_peer_id,
            self.alice_address.clone(),
            self.bitcoin_wallet.clone(),
        )
    }

    async fn make_initial_state(
        &self,
        btc_to_swap: bitcoin::Amount,
        xmr_to_swap: monero::Amount,
        execution_params: ExecutionParams,
    ) -> Result<BobState> {
        let amounts = SwapAmounts {
            btc: btc_to_swap,
            xmr: xmr_to_swap,
        };

        let refund_address = self.bitcoin_wallet.new_address().await?;
        let state0 = bob::State0::new(
            &mut OsRng,
            btc_to_swap,
            xmr_to_swap,
            execution_params.bitcoin_cancel_timelock,
            execution_params.bitcoin_punish_timelock,
            refund_address,
            execution_params.monero_finality_confirmations,
        );

        Ok(BobState::Started { state0, amounts })
    }
}

#[derive(Debug)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
    QuoteResponse(alice::QuoteResponse),
    ExecutionSetupDone(Result<Box<State2>>),
    TransferProof {
        msg: Box<TransferProof>,
        channel: ResponseChannel<()>,
    },
    EncryptedSignatureAcknowledged,
    ResponseSent, // Same variant is used for all messages as no processing is done
    Failure(Error),
}

impl From<peer_tracker::OutEvent> for OutEvent {
    fn from(event: peer_tracker::OutEvent) -> Self {
        match event {
            peer_tracker::OutEvent::ConnectionEstablished(id) => {
                OutEvent::ConnectionEstablished(id)
            }
        }
    }
}

impl From<quote_request::OutEvent> for OutEvent {
    fn from(event: quote_request::OutEvent) -> Self {
        use quote_request::OutEvent::*;
        match event {
            MsgReceived(quote_response) => OutEvent::QuoteResponse(quote_response),
            Failure(err) => OutEvent::Failure(err.context("Failure with Quote Request")),
        }
    }
}

impl From<execution_setup::OutEvent> for OutEvent {
    fn from(event: execution_setup::OutEvent) -> Self {
        match event {
            execution_setup::OutEvent::Done(res) => OutEvent::ExecutionSetupDone(res.map(Box::new)),
        }
    }
}

impl From<transfer_proof::OutEvent> for OutEvent {
    fn from(event: transfer_proof::OutEvent) -> Self {
        use transfer_proof::OutEvent::*;
        match event {
            MsgReceived { msg, channel } => OutEvent::TransferProof {
                msg: Box::new(msg),
                channel,
            },
            AckSent => OutEvent::ResponseSent,
            Failure(err) => OutEvent::Failure(err.context("Failure with Transfer Proof")),
        }
    }
}

impl From<encrypted_signature::OutEvent> for OutEvent {
    fn from(event: encrypted_signature::OutEvent) -> Self {
        use encrypted_signature::OutEvent::*;
        match event {
            Acknowledged => OutEvent::EncryptedSignatureAcknowledged,
            Failure(err) => OutEvent::Failure(err.context("Failure with Encrypted Signature")),
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Bob.
#[derive(NetworkBehaviour, Default)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pt: PeerTracker,
    quote_request: quote_request::Behaviour,
    execution_setup: execution_setup::Behaviour,
    transfer_proof: transfer_proof::Behaviour,
    encrypted_signature: encrypted_signature::Behaviour,
}

impl Behaviour {
    /// Sends a quote request to Alice to retrieve the rate.
    pub fn send_quote_request(&mut self, alice: PeerId, quote_request: QuoteRequest) {
        let _id = self.quote_request.send(alice, quote_request);
        info!("Requesting quote from: {}", alice);
    }

    pub fn start_execution_setup(
        &mut self,
        alice_peer_id: PeerId,
        state0: State0,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
    ) {
        self.execution_setup
            .run(alice_peer_id, state0, bitcoin_wallet);
        info!("Start execution setup with {}", alice_peer_id);
    }

    /// Sends Bob's fourth message to Alice.
    pub fn send_encrypted_signature(
        &mut self,
        alice: PeerId,
        tx_redeem_encsig: bitcoin::EncryptedSignature,
    ) {
        let msg = EncryptedSignature { tx_redeem_encsig };
        self.encrypted_signature.send(alice, msg);
        debug!("Encrypted signature sent");
    }

    /// Add a known address for the given peer
    pub fn add_address(&mut self, peer_id: PeerId, address: Multiaddr) {
        self.pt.add_address(peer_id, address)
    }
}
