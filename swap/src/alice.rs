//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use anyhow::Result;
use libp2p::{
    core::{identity::Keypair, Multiaddr},
    request_response::ResponseChannel,
    NetworkBehaviour, PeerId,
};
use std::time::Duration;
use tracing::debug;

mod messenger;

use self::messenger::*;
use crate::{
    bitcoin, monero,
    network::{
        peer_tracker::{self, PeerTracker},
        request_response::{AliceToBob, TIMEOUT},
        transport, TokioExecutor,
    },
    Never, SwapParams,
};

pub type Swarm = libp2p::Swarm<Alice>;

pub async fn swap(listen: Multiaddr) -> Result<()> {
    let mut swarm = new_swarm(listen)?;

    loop {
        match swarm.next().await {
            BehaviourOutEvent::Request(messenger::BehaviourOutEvent::Btc { btc, channel }) => {
                debug!("Got request from Bob to swap {}", btc);
                let p = calculate_amounts(btc);
                swarm.send(channel, AliceToBob::Amounts(p));
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }
}

fn new_swarm(listen: Multiaddr) -> Result<Swarm> {
    use anyhow::Context as _;

    let behaviour = Alice::default();

    let local_key_pair = behaviour.identity();
    let local_peer_id = behaviour.peer_id();

    let transport = transport::build(local_key_pair)?;

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
pub enum BehaviourOutEvent {
    Request(messenger::BehaviourOutEvent),
    ConnectionEstablished(PeerId),
    Never, // FIXME: Why do we need this?
}

impl From<Never> for BehaviourOutEvent {
    fn from(_: Never) -> Self {
        BehaviourOutEvent::Never
    }
}

impl From<messenger::BehaviourOutEvent> for BehaviourOutEvent {
    fn from(event: messenger::BehaviourOutEvent) -> Self {
        BehaviourOutEvent::Request(event)
    }
}

impl From<peer_tracker::BehaviourOutEvent> for BehaviourOutEvent {
    fn from(event: peer_tracker::BehaviourOutEvent) -> Self {
        match event {
            peer_tracker::BehaviourOutEvent::ConnectionEstablished(id) => {
                BehaviourOutEvent::ConnectionEstablished(id)
            }
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "BehaviourOutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Alice {
    net: Messenger,
    pt: PeerTracker,
    #[behaviour(ignore)]
    identity: Keypair,
}

impl Alice {
    pub fn identity(&self) -> Keypair {
        self.identity.clone()
    }

    pub fn peer_id(&self) -> PeerId {
        PeerId::from(self.identity.public())
    }

    /// Alice always sends her messages as a response to a request from Bob.
    pub fn send(&mut self, channel: ResponseChannel<AliceToBob>, msg: AliceToBob) {
        self.net.send(channel, msg);
    }
}

impl Default for Alice {
    fn default() -> Self {
        let identity = Keypair::generate_ed25519();
        let timeout = Duration::from_secs(TIMEOUT);

        Self {
            net: Messenger::new(timeout),
            pt: PeerTracker::default(),
            identity,
        }
    }
}

// TODO: Check that this is correct.
fn calculate_amounts(btc: bitcoin::Amount) -> SwapParams {
    const XMR_PER_BTC: u64 = 100; // TODO: Get this from an exchange.

    // XMR uses 12 zerose BTC uses 8.
    let picos = (btc.as_sat() * 10000) * XMR_PER_BTC;
    let xmr = monero::Amount::from_piconero(picos);

    SwapParams { btc, xmr }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_BTC: u64 = 100_000_000;
    const HUNDRED_XMR: u64 = 100_000_000_000_000;

    #[test]
    fn one_bitcoin_equals_a_hundred_moneroj() {
        let btc = bitcoin::Amount::from_sat(ONE_BTC);
        let want = monero::Amount::from_piconero(HUNDRED_XMR);

        let SwapParams { xmr: got, .. } = calculate_amounts(btc);
        assert_eq!(got, want);
    }
}
