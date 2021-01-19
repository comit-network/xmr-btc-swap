//! Run an XMR/BTC swap in the role of Bob.
//! Bob holds BTC and wishes receive XMR.
use anyhow::{bail, Result};
use libp2p::{core::Multiaddr, NetworkBehaviour, PeerId};
use tracing::{debug, info};

use crate::{
    bitcoin,
    bitcoin::EncryptedSignature,
    database, monero, network,
    network::peer_tracker::{self, PeerTracker},
    protocol::{alice, bob},
    SwapAmounts,
};

pub use self::{
    amounts::*,
    event_loop::{EventLoop, EventLoopHandle},
    message0::Message0,
    message1::Message1,
    message2::Message2,
    message3::Message3,
    state::*,
    swap::{run, run_until},
};
use crate::{
    config::Config, database::Database, network::transport::build, protocol::StartingBalances,
    seed::Seed,
};
use libp2p::identity::Keypair;
use rand::rngs::OsRng;
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;

mod amounts;
pub mod event_loop;
mod message0;
mod message1;
mod message2;
mod message3;
pub mod state;
pub mod swap;

pub struct Swap {
    pub state: BobState,
    pub event_loop_handle: bob::EventLoopHandle,
    pub db: Database,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub swap_id: Uuid,
}

pub struct SwapFactory {
    swap_id: Uuid,
    identity: Keypair,
    peer_id: PeerId,
    db_path: PathBuf,
    config: Config,

    alice_connect_address: Multiaddr,
    alice_connect_peer_id: PeerId,

    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub starting_balances: StartingBalances,
}

impl SwapFactory {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        seed: Seed,
        db_path: PathBuf,
        swap_id: Uuid,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        config: Config,
        starting_balances: StartingBalances,
        alice_connect_address: Multiaddr,
        alice_connect_peer_id: PeerId,
    ) -> Self {
        let identity = network::Seed::new(seed).derive_libp2p_identity();
        let peer_id = identity.public().into_peer_id();

        Self {
            swap_id,
            identity,
            peer_id,
            db_path,
            config,
            alice_connect_address,
            alice_connect_peer_id,
            bitcoin_wallet,
            monero_wallet,
            starting_balances,
        }
    }

    pub async fn new_swap_as_bob(
        &self,
        swap_amounts: SwapAmounts,
    ) -> Result<(bob::Swap, bob::EventLoop)> {
        let initial_state = init_bob_state(
            swap_amounts.btc,
            swap_amounts.xmr,
            self.bitcoin_wallet.clone(),
            self.config,
        )
        .await?;

        let (event_loop, event_loop_handle) = init_bob_event_loop(
            self.identity.clone(),
            self.peer_id.clone(),
            self.alice_connect_peer_id.clone(),
            self.alice_connect_address.clone(),
        )?;

        let db = Database::open(self.db_path.as_path())?;

        Ok((
            Swap {
                state: initial_state,
                event_loop_handle,
                db,
                bitcoin_wallet: self.bitcoin_wallet.clone(),
                monero_wallet: self.monero_wallet.clone(),
                swap_id: self.swap_id,
            },
            event_loop,
        ))
    }

    pub async fn resume(&self) -> Result<(bob::Swap, bob::EventLoop)> {
        // reopen the existing database
        let db = Database::open(self.db_path.clone().as_path())?;

        let resume_state = if let database::Swap::Bob(state) = db.get_state(self.swap_id)? {
            state.into()
        } else {
            bail!(
                "Trying to load swap with id {} for the wrong direction.",
                self.swap_id
            )
        };

        let (event_loop, event_loop_handle) = init_bob_event_loop(
            self.identity.clone(),
            self.peer_id.clone(),
            self.alice_connect_peer_id.clone(),
            self.alice_connect_address.clone(),
        )?;

        Ok((
            Swap {
                state: resume_state,
                event_loop_handle,
                db,
                bitcoin_wallet: self.bitcoin_wallet.clone(),
                monero_wallet: self.monero_wallet.clone(),
                swap_id: self.swap_id,
            },
            event_loop,
        ))
    }
}

async fn init_bob_state(
    btc_to_swap: bitcoin::Amount,
    xmr_to_swap: monero::Amount,
    bob_btc_wallet: Arc<bitcoin::Wallet>,
    config: Config,
) -> Result<BobState> {
    let amounts = SwapAmounts {
        btc: btc_to_swap,
        xmr: xmr_to_swap,
    };

    let refund_address = bob_btc_wallet.new_address().await?;
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

fn init_bob_event_loop(
    identity: Keypair,
    peer_id: PeerId,
    alice_peer_id: PeerId,
    alice_addr: Multiaddr,
) -> Result<(bob::event_loop::EventLoop, bob::event_loop::EventLoopHandle)> {
    let bob_behaviour = bob::Behaviour::default();
    let bob_transport = build(identity)?;

    bob::event_loop::EventLoop::new(
        bob_transport,
        bob_behaviour,
        peer_id,
        alice_peer_id,
        alice_addr,
    )
}

#[derive(Debug, Clone)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
    Amounts(SwapAmounts),
    Message0(Box<alice::Message0>),
    Message1(Box<alice::Message1>),
    Message2(alice::Message2),
    Message3,
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

impl From<amounts::OutEvent> for OutEvent {
    fn from(event: amounts::OutEvent) -> Self {
        match event {
            amounts::OutEvent::Amounts(amounts) => OutEvent::Amounts(amounts),
        }
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
            message2::OutEvent::Msg(msg) => OutEvent::Message2(msg),
        }
    }
}

impl From<message3::OutEvent> for OutEvent {
    fn from(event: message3::OutEvent) -> Self {
        match event {
            message3::OutEvent::Msg => OutEvent::Message3,
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Bob.
#[derive(NetworkBehaviour, Default)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pt: PeerTracker,
    amounts: Amounts,
    message0: message0::Behaviour,
    message1: message1::Behaviour,
    message2: message2::Behaviour,
    message3: message3::Behaviour,
}

impl Behaviour {
    /// Sends a message to Alice to get current amounts based on `btc`.
    pub fn request_amounts(&mut self, alice: PeerId, btc: u64) {
        let btc = ::bitcoin::Amount::from_sat(btc);
        let _id = self.amounts.request_amounts(alice.clone(), btc);
        info!("Requesting amounts from: {}", alice);
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
    pub fn send_message3(&mut self, alice: PeerId, tx_redeem_encsig: EncryptedSignature) {
        let msg = bob::Message3 { tx_redeem_encsig };
        self.message3.send(alice, msg);
        debug!("Sent Message3");
    }

    /// Add a known address for the given peer
    pub fn add_address(&mut self, peer_id: PeerId, address: Multiaddr) {
        self.pt.add_address(peer_id, address)
    }
}
