//! Run an XMR/BTC swap in the role of Bob.
//! Bob holds BTC and wishes receive XMR.
use anyhow::Result;
use futures::{
    channel::mpsc::{Receiver, Sender},
    StreamExt,
};
use libp2p::{core::identity::Keypair, Multiaddr, NetworkBehaviour, PeerId};
use rand::rngs::OsRng;
use std::{process, thread};
use tracing::{debug, info};

mod amounts;
mod message0;
mod message1;
mod message2;

use self::{amounts::*, message0::*, message1::*, message2::*};
use crate::{
    network::{
        peer_tracker::{self, PeerTracker},
        transport, TokioExecutor,
    },
    Cmd, Rsp, SwapAmounts, PUNISH_TIMELOCK, REFUND_TIMELOCK,
};
use xmr_btc::{
    alice,
    bitcoin::{BroadcastSignedTransaction, BuildTxLockPsbt, SignTxLock},
    bob::{self, State0},
};

// FIXME: This whole function is horrible, needs total re-write.
pub async fn swap<W>(
    btc: u64,
    addr: Multiaddr,
    mut cmd_tx: Sender<Cmd>,
    mut rsp_rx: Receiver<Rsp>,
    refund_address: ::bitcoin::Address,
    wallet: W,
) -> Result<()>
where
    W: BuildTxLockPsbt + SignTxLock + BroadcastSignedTransaction + Send + Sync + 'static,
{
    let mut swarm = new_swarm()?;

    libp2p::Swarm::dial_addr(&mut swarm, addr)?;
    let alice = match swarm.next().await {
        OutEvent::ConnectionEstablished(alice) => alice,
        other => panic!("unexpected event: {:?}", other),
    };
    info!("Connection established.");

    swarm.request_amounts(alice.clone(), btc);

    let (btc, xmr) = match swarm.next().await {
        OutEvent::Amounts(amounts) => {
            debug!("Got amounts from Alice: {:?}", amounts);
            let cmd = Cmd::VerifyAmounts(amounts);
            cmd_tx.try_send(cmd)?;
            let response = rsp_rx.next().await;
            if response == Some(Rsp::Abort) {
                info!("Amounts no good, aborting ...");
                process::exit(0);
            }

            info!("User verified amounts, continuing with swap ...");
            (amounts.btc, amounts.xmr)
        }
        other => panic!("unexpected event: {:?}", other),
    };

    // FIXME: Too many `bitcoin` crates/modules.
    let xmr = xmr_btc::monero::Amount::from_piconero(xmr.as_piconero());
    let btc = ::bitcoin::Amount::from_sat(btc.as_sat());

    // TODO: Pass this in using <R: RngCore + CryptoRng>
    let rng = &mut OsRng;
    let state0 = State0::new(
        rng,
        btc,
        xmr,
        REFUND_TIMELOCK,
        PUNISH_TIMELOCK,
        refund_address,
    );

    swarm.send_message0(alice.clone(), state0.next_message(rng));
    let state1 = match swarm.next().await {
        OutEvent::Message0(msg) => {
            state0.receive(&wallet, msg).await? // TODO: More graceful error
                                                // handling.
        }
        other => panic!("unexpected event: {:?}", other),
    };

    swarm.send_message1(alice.clone(), state1.next_message());
    let state2 = match swarm.next().await {
        OutEvent::Message1(msg) => {
            state1.receive(msg)? // TODO: More graceful error handling.
        }
        other => panic!("unexpected event: {:?}", other),
    };

    swarm.send_message2(alice.clone(), state2.next_message());

    info!("Handshake complete, we now have State2 for Bob.");

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
    ConnectionEstablished(PeerId),
    Amounts(SwapAmounts),
    Message0(alice::Message0),
    Message1(alice::Message1),
    Message2(alice::Message2),
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
            message0::OutEvent::Msg(msg) => OutEvent::Message0(msg),
        }
    }
}

impl From<message1::OutEvent> for OutEvent {
    fn from(event: message1::OutEvent) -> Self {
        match event {
            message1::OutEvent::Msg(msg) => OutEvent::Message1(msg),
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

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Bob.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Bob {
    pt: PeerTracker,
    amounts: Amounts,
    message0: Message0,
    message1: Message1,
    message2: Message2,
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
    pub fn request_amounts(&mut self, alice: PeerId, btc: u64) {
        let btc = ::bitcoin::Amount::from_sat(btc);
        let _id = self.amounts.request_amounts(alice.clone(), btc);
        debug!("Requesting amounts from: {}", alice);
    }

    /// Sends Bob's first message to Alice.
    pub fn send_message0(&mut self, alice: PeerId, msg: bob::Message0) {
        self.message0.send(alice, msg)
    }

    /// Sends Bob's second message to Alice.
    pub fn send_message1(&mut self, alice: PeerId, msg: bob::Message1) {
        self.message1.send(alice, msg)
    }

    /// Sends Bob's third message to Alice.
    pub fn send_message2(&mut self, alice: PeerId, msg: bob::Message2) {
        self.message2.send(alice, msg)
    }

    /// Returns Alice's peer id if we are connected.
    pub fn peer_id_of_alice(&self) -> Option<PeerId> {
        self.pt.counterparty_peer_id()
    }
}

impl Default for Bob {
    fn default() -> Bob {
        let identity = Keypair::generate_ed25519();

        Self {
            pt: PeerTracker::default(),
            amounts: Amounts::default(),
            message0: Message0::default(),
            message1: Message1::default(),
            message2: Message2::default(),
            identity,
        }
    }
}
