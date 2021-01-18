//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use anyhow::Result;
use libp2p::{request_response::ResponseChannel, NetworkBehaviour, PeerId};
use tracing::{debug, info};

use crate::{
    bitcoin, database, monero,
    network::{
        peer_tracker::{self, PeerTracker},
        request_response::AliceToBob,
        Seed as NetworkSeed,
    },
    protocol::bob,
    StartingBalances, SwapAmounts,
};

pub use self::{
    amounts::*,
    event_loop::{EventLoop, EventLoopHandle},
    message0::Message0,
    message1::Message1,
    message2::Message2,
    state::*,
    swap::{run, run_until},
};
use crate::{config::Config, database::Database, network::transport::build, seed::Seed};
use libp2p::{core::Multiaddr, identity::Keypair};
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
mod steps;
pub mod swap;

pub struct Swap {
    pub state: AliceState,
    pub event_loop_handle: EventLoopHandle,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub config: Config,
    pub swap_id: Uuid,
    pub db: Database,
}

pub struct AliceSwapFactory {
    listen_address: Multiaddr,
    identity: Keypair,
    peer_id: PeerId,

    db_path: PathBuf,
    swap_id: Uuid,

    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    config: Config,
    pub starting_balances: StartingBalances,
}

impl AliceSwapFactory {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        seed: Seed,
        config: Config,
        swap_id: Uuid,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        starting_balances: StartingBalances,
        db_path: PathBuf,
        listen_address: Multiaddr,
    ) -> Self {
        let network_seed = NetworkSeed::new(seed);
        let identity = network_seed.derive_libp2p_identity();
        let peer_id = PeerId::from(identity.public());

        Self {
            listen_address,
            identity,
            peer_id,
            db_path,
            swap_id,
            bitcoin_wallet,
            monero_wallet,
            config,
            starting_balances,
        }
    }

    pub async fn new_swap_as_alice(&self, swap_amounts: SwapAmounts) -> Result<(Swap, EventLoop)> {
        let initial_state = init_alice_state(
            swap_amounts.btc,
            swap_amounts.xmr,
            self.bitcoin_wallet.clone(),
            self.config,
        )
        .await?;

        let (event_loop, event_loop_handle) = init_alice_event_loop(
            self.listen_address.clone(),
            self.identity.clone(),
            self.peer_id.clone(),
        )?;

        let db = Database::open(self.db_path.as_path())?;

        Ok((
            Swap {
                event_loop_handle,
                bitcoin_wallet: self.bitcoin_wallet.clone(),
                monero_wallet: self.monero_wallet.clone(),
                config: self.config,
                db,
                state: initial_state,
                swap_id: self.swap_id,
            },
            event_loop,
        ))
    }

    pub async fn recover_alice_from_db(&self) -> Result<(Swap, EventLoop)> {
        // reopen the existing database
        let db = Database::open(self.db_path.clone().as_path())?;

        let resume_state = if let database::Swap::Alice(state) = db.get_state(self.swap_id)? {
            state.into()
        } else {
            unreachable!()
        };

        let (event_loop, event_loop_handle) = init_alice_event_loop(
            self.listen_address.clone(),
            self.identity.clone(),
            self.peer_id.clone(),
        )?;

        Ok((
            Swap {
                state: resume_state,
                event_loop_handle,
                bitcoin_wallet: self.bitcoin_wallet.clone(),
                monero_wallet: self.monero_wallet.clone(),
                config: self.config,
                swap_id: self.swap_id,
                db,
            },
            event_loop,
        ))
    }

    pub fn peer_id(&self) -> PeerId {
        self.peer_id.clone()
    }

    pub fn listen_address(&self) -> Multiaddr {
        self.listen_address.clone()
    }
}

async fn init_alice_state(
    btc_to_swap: bitcoin::Amount,
    xmr_to_swap: monero::Amount,
    alice_btc_wallet: Arc<bitcoin::Wallet>,
    config: Config,
) -> Result<AliceState> {
    let rng = &mut OsRng;

    let amounts = SwapAmounts {
        btc: btc_to_swap,
        xmr: xmr_to_swap,
    };

    let a = bitcoin::SecretKey::new_random(rng);
    let s_a = cross_curve_dleq::Scalar::random(rng);
    let v_a = monero::PrivateViewKey::new_random(rng);
    let redeem_address = alice_btc_wallet.as_ref().new_address().await?;
    let punish_address = redeem_address.clone();
    let state0 = State0::new(
        a,
        s_a,
        v_a,
        amounts.btc,
        amounts.xmr,
        config.bitcoin_cancel_timelock,
        config.bitcoin_punish_timelock,
        redeem_address,
        punish_address,
    );

    Ok(AliceState::Started { amounts, state0 })
}

fn init_alice_event_loop(
    listen: Multiaddr,
    identity: Keypair,
    peer_id: PeerId,
) -> Result<(EventLoop, EventLoopHandle)> {
    let alice_behaviour = Behaviour::default();
    let alice_transport = build(identity)?;
    EventLoop::new(alice_transport, alice_behaviour, listen, peer_id)
}

pub type Swarm = libp2p::Swarm<Behaviour>;

#[derive(Debug)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
    // TODO (Franck): Change this to get both amounts so parties can verify the amounts are
    // expected early on.
    Request(Box<amounts::OutEvent>), /* Not-uniform with Bob on purpose, ready for adding Xmr
                                      * event. */
    Message0 {
        msg: Box<bob::Message0>,
        channel: ResponseChannel<AliceToBob>,
    },
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
        OutEvent::Request(Box::new(event))
    }
}

impl From<message0::OutEvent> for OutEvent {
    fn from(event: message0::OutEvent) -> Self {
        match event {
            message0::OutEvent::Msg { channel, msg } => OutEvent::Message0 {
                msg: Box::new(msg),
                channel,
            },
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
    message0: message0::Behaviour,
    message1: message1::Behaviour,
    message2: message2::Behaviour,
    message3: message3::Behaviour,
}

impl Behaviour {
    /// Alice always sends her messages as a response to a request from Bob.
    pub fn send_amounts(&mut self, channel: ResponseChannel<AliceToBob>, amounts: SwapAmounts) {
        let msg = AliceToBob::Amounts(amounts);
        self.amounts.send(channel, msg);
        info!("Sent amounts response");
    }

    /// Send Message0 to Bob in response to receiving his Message0.
    pub fn send_message0(&mut self, channel: ResponseChannel<AliceToBob>, msg: Message0) {
        self.message0.send(channel, msg);
        debug!("Sent Message0");
    }

    /// Send Message1 to Bob in response to receiving his Message1.
    pub fn send_message1(&mut self, channel: ResponseChannel<AliceToBob>, msg: Message1) {
        self.message1.send(channel, msg);
        debug!("Sent Message1");
    }

    /// Send Message2 to Bob in response to receiving his Message2.
    pub fn send_message2(&mut self, channel: ResponseChannel<AliceToBob>, msg: Message2) {
        self.message2.send(channel, msg);
        debug!("Sent Message2");
    }
}

impl Default for Behaviour {
    fn default() -> Self {
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
