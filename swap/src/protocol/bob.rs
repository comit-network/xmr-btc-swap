//! Run an XMR/BTC swap in the role of Bob.
//! Bob holds BTC and wishes receive XMR.
use crate::{
    bitcoin,
    config::Config,
    database,
    database::Database,
    monero, network,
    network::{
        peer_tracker::{self, PeerTracker},
        transport::build,
    },
    protocol::{alice, bob, SwapAmounts},
    seed::Seed,
};
use anyhow::{bail, Result};
use libp2p::{core::Multiaddr, identity::Keypair, NetworkBehaviour, PeerId};
use rand::rngs::OsRng;
use std::{path::PathBuf, sync::Arc};
use tracing::{debug, info};
use uuid::Uuid;

pub use self::{
    encrypted_signature::EncryptedSignature,
    event_loop::{EventLoop, EventLoopHandle},
    message0::Message0,
    message1::Message1,
    message2::Message2,
    state::*,
    swap::{run, run_until},
    swap_request::*,
};
use crate::protocol::alice::TransferProof;

mod encrypted_signature;
pub mod event_loop;
mod message0;
mod message1;
mod message2;
pub mod state;
pub mod swap;
mod swap_request;
mod transfer_proof;

pub struct Swap {
    pub state: BobState,
    pub event_loop_handle: bob::EventLoopHandle,
    pub db: Database,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub config: Config,
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
    config: Config,
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
        config: Config,
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
            config,
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
                    .make_initial_state(swap_amounts.btc, swap_amounts.xmr, self.config)
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
                        config: self.config,
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
                        config: self.config,
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
        let bob_transport = build(self.identity.clone())?;

        bob::event_loop::EventLoop::new(
            bob_transport,
            bob_behaviour,
            self.peer_id.clone(),
            self.alice_peer_id.clone(),
            self.alice_address.clone(),
        )
    }

    async fn make_initial_state(
        &self,
        btc_to_swap: bitcoin::Amount,
        xmr_to_swap: monero::Amount,
        config: Config,
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
            config.bitcoin_cancel_timelock,
            config.bitcoin_punish_timelock,
            refund_address,
            config.monero_finality_confirmations,
        );

        Ok(BobState::Started { state0, amounts })
    }
}

#[derive(Debug, Clone)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
    SwapResponse(alice::SwapResponse),
    Message0(Box<alice::Message0>),
    Message1(Box<alice::Message1>),
    Message2,
    TransferProof(Box<TransferProof>),
    EncryptedSignature,
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

impl From<swap_request::OutEvent> for OutEvent {
    fn from(event: swap_request::OutEvent) -> Self {
        OutEvent::SwapResponse(event.swap_response)
    }
}

impl From<message0::OutEvent> for OutEvent {
    fn from(event: message0::OutEvent) -> Self {
        match event {
            message0::OutEvent::Msg(msg) => OutEvent::Message0(Box::new(msg)),
        }
    }
}

impl From<message1::OutEvent> for OutEvent {
    fn from(event: message1::OutEvent) -> Self {
        match event {
            message1::OutEvent::Msg(msg) => OutEvent::Message1(Box::new(msg)),
        }
    }
}

impl From<message2::OutEvent> for OutEvent {
    fn from(event: message2::OutEvent) -> Self {
        match event {
            message2::OutEvent::Msg => OutEvent::Message2,
        }
    }
}

impl From<transfer_proof::OutEvent> for OutEvent {
    fn from(event: transfer_proof::OutEvent) -> Self {
        match event {
            transfer_proof::OutEvent::Msg(msg) => OutEvent::TransferProof(Box::new(msg)),
        }
    }
}

impl From<encrypted_signature::OutEvent> for OutEvent {
    fn from(event: encrypted_signature::OutEvent) -> Self {
        match event {
            encrypted_signature::OutEvent::Msg => OutEvent::EncryptedSignature,
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Bob.
#[derive(NetworkBehaviour, Default)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pt: PeerTracker,
    swap_request: swap_request::Behaviour,
    message0: message0::Behaviour,
    message1: message1::Behaviour,
    message2: message2::Behaviour,
    transfer_proof: transfer_proof::Behaviour,
    encrypted_signature: encrypted_signature::Behaviour,
}

impl Behaviour {
    /// Sends a swap request to Alice to negotiate the swap.
    pub fn send_swap_request(&mut self, alice: PeerId, swap_request: SwapRequest) {
        let _id = self.swap_request.send(alice.clone(), swap_request);
        info!("Requesting swap from: {}", alice);
    }

    /// Sends Bob's first message to Alice.
    pub fn send_message0(&mut self, alice: PeerId, msg: bob::Message0) {
        self.message0.send(alice, msg);
        debug!("Sent Message0");
    }

    /// Sends Bob's second message to Alice.
    pub fn send_message1(&mut self, alice: PeerId, msg: bob::Message1) {
        self.message1.send(alice, msg);
        debug!("Sent Message1");
    }

    /// Sends Bob's third message to Alice.
    pub fn send_message2(&mut self, alice: PeerId, msg: bob::Message2) {
        self.message2.send(alice, msg);
        debug!("Sent Message2");
    }

    /// Sends Bob's fourth message to Alice.
    pub fn send_encrypted_signature(
        &mut self,
        alice: PeerId,
        tx_redeem_encsig: bitcoin::EncryptedSignature,
    ) {
        let msg = EncryptedSignature { tx_redeem_encsig };
        self.encrypted_signature.send(alice, msg);
        debug!("Sent Message3");
    }

    /// Add a known address for the given peer
    pub fn add_address(&mut self, peer_id: PeerId, address: Multiaddr) {
        self.pt.add_address(peer_id, address)
    }
}
