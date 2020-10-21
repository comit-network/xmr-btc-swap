//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use anyhow::Result;
use libp2p::{
    core::{identity::Keypair, Multiaddr},
    request_response::ResponseChannel,
    NetworkBehaviour, PeerId,
};
use rand::{CryptoRng, RngCore};
use std::{thread, time::Duration};
use tracing::debug;

mod amounts;
mod message0;

use self::{amounts::*, message0::*};
use crate::{
    network::{
        peer_tracker::{self, PeerTracker},
        request_response::{AliceToBob, TIMEOUT},
        transport, TokioExecutor,
    },
    SwapParams, PUNISH_TIMELOCK, REFUND_TIMELOCK,
};
use xmr_btc::{alice::State0, bob, monero};

pub type Swarm = libp2p::Swarm<Alice>;

pub async fn swap<R: RngCore + CryptoRng>(
    listen: Multiaddr,
    rng: &mut R,
    redeem_address: ::bitcoin::Address,
    punish_address: ::bitcoin::Address,
) -> Result<()> {
    let message0: Option<bob::Message0> = None;
    let mut last_amounts: Option<SwapParams> = None;

    let mut swarm = new_swarm(listen)?;

    loop {
        match swarm.next().await {
            OutEvent::ConnectionEstablished(id) => {
                tracing::info!("Connection established with: {}", id);
            }
            OutEvent::Request(amounts::OutEvent::Btc { btc, channel }) => {
                debug!("Got request from Bob to swap {}", btc);
                let p = calculate_amounts(btc);
                last_amounts = Some(p);
                swarm.send(channel, AliceToBob::Amounts(p));
            }
            OutEvent::Message0 => {
                debug!("Got message0 from Bob");
                // TODO: Do this in a more Rusty/functional way.
                // message0 = Some(msg);
                break;
            }
        };
    }

    let (xmr, btc) = match last_amounts {
        Some(p) => (p.xmr, p.btc),
        None => unreachable!("should have amounts by here"),
    };

    // FIXME: Too many `bitcoin` crates/modules.
    let xmr = monero::Amount::from_piconero(xmr.as_piconero());
    let btc = ::bitcoin::Amount::from_sat(btc.as_sat());

    let state0 = State0::new(
        rng,
        btc,
        xmr,
        REFUND_TIMELOCK,
        PUNISH_TIMELOCK,
        redeem_address,
        punish_address,
    );
    swarm.set_state0(state0.clone());

    let _state1 = match message0 {
        Some(msg) => state0.receive(msg),
        None => todo!("implement serde on Message0"),
    };

    tracing::warn!("parking thread ...");
    thread::park();
    Ok(())
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
pub enum OutEvent {
    ConnectionEstablished(PeerId),
    Request(amounts::OutEvent),
    // Message0(bob::Message0),
    Message0,
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
            // message0::OutEvent::Msg(msg) => OutEvent::Message0(msg),
            message0::OutEvent::Msg => OutEvent::Message0,
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Alice {
    pt: PeerTracker,
    amounts: Amounts,
    message0: Message0,
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
        self.amounts.send(channel, msg);
    }

    pub fn set_state0(&mut self, state: State0) {
        let _ = self.message0.set_state(state);
    }
}

impl Default for Alice {
    fn default() -> Self {
        let identity = Keypair::generate_ed25519();
        let timeout = Duration::from_secs(TIMEOUT);

        Self {
            pt: PeerTracker::default(),
            amounts: Amounts::new(timeout),
            message0: Message0::new(timeout),
            identity,
        }
    }
}

// TODO: Check that this is correct.
fn calculate_amounts(btc: ::bitcoin::Amount) -> SwapParams {
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
        let btc = ::bitcoin::Amount::from_sat(ONE_BTC);
        let want = monero::Amount::from_piconero(HUNDRED_XMR);

        let SwapParams { xmr: got, .. } = calculate_amounts(btc);
        assert_eq!(got, want);
    }
}
