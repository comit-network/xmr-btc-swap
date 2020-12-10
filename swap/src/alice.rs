//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use self::{amounts::*, message0::*, message1::*, message2::*, message3::*};
use crate::{
    network::{
        peer_tracker::{self, PeerTracker},
        request_response::AliceToBob,
        transport::SwapTransport,
        TokioExecutor,
    },
    SwapAmounts,
};
use anyhow::Result;
use libp2p::{
    core::{identity::Keypair, Multiaddr},
    request_response::ResponseChannel,
    NetworkBehaviour, PeerId,
};
use tracing::{debug, info};
use xmr_btc::{alice::State0, bob};

mod amounts;
pub mod event_loop;
mod execution;
mod message0;
mod message1;
mod message2;
mod message3;
pub mod swap;

pub type Swarm = libp2p::Swarm<Behaviour>;

pub fn new_swarm(
    listen: Multiaddr,
    transport: SwapTransport,
    behaviour: Behaviour,
) -> Result<Swarm> {
    use anyhow::Context as _;

    let local_peer_id = behaviour.peer_id();

    let mut swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, local_peer_id.clone())
        .executor(Box::new(TokioExecutor {
            handle: tokio::runtime::Handle::current(),
        }))
        .build();

    Swarm::listen_on(&mut swarm, listen.clone())
        .with_context(|| format!("Address is not supported: {:#}", listen))?;

    tracing::info!("Initialized swarm: {}", local_peer_id);

    Ok(swarm)
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
    // TODO (Franck): Change this to get both amounts so parties can verify the amounts are
    // expected early on.
    Request(amounts::OutEvent), // Not-uniform with Bob on purpose, ready for adding Xmr event.
    Message0(bob::Message0),
    Message1 {
        msg: bob::Message1,
        channel: ResponseChannel<AliceToBob>,
    },
    Message2 {
        msg: bob::Message2,
        channel: ResponseChannel<AliceToBob>,
    },
    Message3(bob::Message3),
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
        OutEvent::Request(event)
    }
}

impl From<message0::OutEvent> for OutEvent {
    fn from(event: message0::OutEvent) -> Self {
        match event {
            message0::OutEvent::Msg(msg) => OutEvent::Message0(msg),
        }
    }
}

impl From<message1::OutEvent> for OutEvent {
    fn from(event: message1::OutEvent) -> Self {
        match event {
            message1::OutEvent::Msg { msg, channel } => OutEvent::Message1 { msg, channel },
        }
    }
}

impl From<message2::OutEvent> for OutEvent {
    fn from(event: message2::OutEvent) -> Self {
        match event {
            message2::OutEvent::Msg { msg, channel } => OutEvent::Message2 { msg, channel },
        }
    }
}

impl From<message3::OutEvent> for OutEvent {
    fn from(event: message3::OutEvent) -> Self {
        match event {
            message3::OutEvent::Msg(msg) => OutEvent::Message3(msg),
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pt: PeerTracker,
    amounts: Amounts,
    message0: Message0,
    message1: Message1,
    message2: Message2,
    message3: Message3,
    #[behaviour(ignore)]
    identity: Keypair,
}

impl Behaviour {
    pub fn new(state: State0) -> Self {
        let identity = Keypair::generate_ed25519();

        Self {
            pt: PeerTracker::default(),
            amounts: Amounts::default(),
            message0: Message0::new(state),
            message1: Message1::default(),
            message2: Message2::default(),
            message3: Message3::default(),
            identity,
        }
    }

    pub fn identity(&self) -> Keypair {
        self.identity.clone()
    }

    pub fn peer_id(&self) -> PeerId {
        PeerId::from(self.identity.public())
    }

    /// Alice always sends her messages as a response to a request from Bob.
    pub fn send_amounts(&mut self, channel: ResponseChannel<AliceToBob>, amounts: SwapAmounts) {
        let msg = AliceToBob::Amounts(amounts);
        self.amounts.send(channel, msg);
        info!("Sent amounts response");
    }

    /// Send Message1 to Bob in response to receiving his Message1.
    pub fn send_message1(
        &mut self,
        channel: ResponseChannel<AliceToBob>,
        msg: xmr_btc::alice::Message1,
    ) {
        self.message1.send(channel, msg);
        debug!("Sent Message1");
    }

    /// Send Message2 to Bob in response to receiving his Message2.
    pub fn send_message2(
        &mut self,
        channel: ResponseChannel<AliceToBob>,
        msg: xmr_btc::alice::Message2,
    ) {
        self.message2.send(channel, msg);
        debug!("Sent Message2");
    }
}
