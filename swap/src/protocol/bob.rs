//! Run an XMR/BTC swap in the role of Bob.
//! Bob holds BTC and wishes receive XMR.
use anyhow::Result;
use libp2p::{
    core::{identity::Keypair, Multiaddr},
    NetworkBehaviour, PeerId,
};
use tracing::{debug, info};

use crate::{
    bitcoin::EncryptedSignature,
    network::{
        peer_tracker::{self, PeerTracker},
        transport::SwapTransport,
        Seed, TokioExecutor,
    },
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
    swap::{run_until, swap},
};

mod amounts;
pub mod event_loop;
mod message0;
mod message1;
mod message2;
mod message3;
pub mod state;
pub mod swap;

pub type Swarm = libp2p::Swarm<Behaviour>;

pub fn new_swarm(transport: SwapTransport, behaviour: Behaviour) -> Result<Swarm> {
    let local_peer_id = behaviour.peer_id();

    let swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, local_peer_id.clone())
        .executor(Box::new(TokioExecutor {
            handle: tokio::runtime::Handle::current(),
        }))
        .build();

    info!("Initialized swarm with identity {}", local_peer_id);

    Ok(swarm)
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
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pt: PeerTracker,
    amounts: Amounts,
    message0: message0::Behaviour,
    message1: message1::Behaviour,
    message2: message2::Behaviour,
    message3: message3::Behaviour,
    #[behaviour(ignore)]
    identity: Keypair,
}

impl Behaviour {
    pub fn new(seed: Seed) -> Self {
        let identity = seed.derive_libp2p_identity();

        Self {
            pt: PeerTracker::default(),
            amounts: Amounts::default(),
            message0: message0::Behaviour::default(),
            message1: message1::Behaviour::default(),
            message2: message2::Behaviour::default(),
            message3: message3::Behaviour::default(),
            identity,
        }
    }

    pub fn identity(&self) -> Keypair {
        self.identity.clone()
    }

    pub fn peer_id(&self) -> PeerId {
        PeerId::from(self.identity.public())
    }

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

impl Default for Behaviour {
    fn default() -> Behaviour {
        let identity = Keypair::generate_ed25519();

        Self {
            pt: PeerTracker::default(),
            amounts: Amounts::default(),
            message0: message0::Behaviour::default(),
            message1: message1::Behaviour::default(),
            message2: message2::Behaviour::default(),
            message3: message3::Behaviour::default(),
            identity,
        }
    }
}
