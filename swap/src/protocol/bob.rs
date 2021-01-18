//! Run an XMR/BTC swap in the role of Bob.
//! Bob holds BTC and wishes receive XMR.
use libp2p::{core::Multiaddr, NetworkBehaviour, PeerId};
use tracing::{debug, info};

use crate::{
    bitcoin,
    bitcoin::EncryptedSignature,
    monero,
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
use crate::database::Database;
use std::sync::Arc;
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

pub type Swarm = libp2p::Swarm<Behaviour>;

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

impl Default for Behaviour {
    fn default() -> Behaviour {
        Self {
            pt: PeerTracker::default(),
            amounts: Amounts::default(),
            message0: message0::Behaviour::default(),
            message1: message1::Behaviour::default(),
            message2: message2::Behaviour::default(),
            message3: message3::Behaviour::default(),
        }
    }
}
