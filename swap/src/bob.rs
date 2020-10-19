//! Run an XMR/BTC swap in the role of Bob.
//! Bob holds BTC and wishes receive XMR.
use anyhow::Result;
use futures::{
    channel::mpsc::{Receiver, Sender},
    StreamExt,
};
use libp2p::{core::identity::Keypair, Multiaddr, NetworkBehaviour, PeerId};
use std::{process, thread, time::Duration};
use tracing::{debug, info, warn};

mod amounts;

use self::amounts::*;
use crate::{
    bitcoin,
    network::{
        peer_tracker::{self, PeerTracker},
        request_response::TIMEOUT,
        transport, TokioExecutor,
    },
    Cmd, Rsp,
};

pub async fn swap(
    btc: u64,
    addr: Multiaddr,
    mut cmd_tx: Sender<Cmd>,
    mut rsp_rx: Receiver<Rsp>,
) -> Result<()> {
    let mut swarm = new_swarm()?;

    libp2p::Swarm::dial_addr(&mut swarm, addr)?;
    let id = match swarm.next().await {
        OutEvent::ConnectionEstablished(id) => id,
        other => panic!("unexpected event: {:?}", other),
    };
    info!("Connection established.");

    swarm.request_amounts(id, btc).await;

    match swarm.next().await {
        OutEvent::Response(amounts::OutEvent::Amounts(p)) => {
            debug!("Got response from Alice: {:?}", p);
            let cmd = Cmd::VerifyAmounts(p);
            cmd_tx.try_send(cmd)?;
            let response = rsp_rx.next().await;
            if response == Some(Rsp::Abort) {
                info!("Amounts no good, aborting ...");
                process::exit(0);
            }
            info!("User verified amounts, continuing with swap ...");
        }
        other => panic!("unexpected event: {:?}", other),
    }

    warn!("parking thread ...");
    thread::park();
    Ok(())
}

pub type Swarm = libp2p::Swarm<Bob>;

fn new_swarm() -> Result<Swarm> {
    let behaviour = Bob::default();

    let local_key_pair = behaviour.identity();
    let local_peer_id = behaviour.peer_id();

    let transport = transport::build(local_key_pair)?;

    let swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, local_peer_id.clone())
        .executor(Box::new(TokioExecutor {
            handle: tokio::runtime::Handle::current(),
        }))
        .build();

    info!("Initialized swarm with identity {}", local_peer_id);

    Ok(swarm)
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum OutEvent {
    Response(amounts::OutEvent),
    ConnectionEstablished(PeerId),
}

impl From<amounts::OutEvent> for OutEvent {
    fn from(event: amounts::OutEvent) -> Self {
        OutEvent::Response(event)
    }
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

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Bob.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Bob {
    amounts: Amounts,
    pt: PeerTracker,
    #[behaviour(ignore)]
    identity: Keypair,
}

impl Bob {
    pub fn identity(&self) -> Keypair {
        self.identity.clone()
    }

    pub fn peer_id(&self) -> PeerId {
        PeerId::from(self.identity.public())
    }

    /// Sends a message to Alice to get current amounts based on `btc`.
    pub async fn request_amounts(&mut self, alice: PeerId, btc: u64) {
        let btc = bitcoin::Amount::from_sat(btc);
        let _id = self.amounts.request_amounts(alice.clone(), btc).await;
        debug!("Requesting amounts from: {}", alice);
    }

    /// Returns Alice's peer id if we are connected.
    pub fn peer_id_of_alice(&self) -> Option<PeerId> {
        self.pt.counterparty_peer_id()
    }
}

impl Default for Bob {
    fn default() -> Bob {
        let identity = Keypair::generate_ed25519();
        let timeout = Duration::from_secs(TIMEOUT);

        Self {
            amounts: Amounts::new(timeout),
            pt: PeerTracker::default(),
            identity,
        }
    }
}
