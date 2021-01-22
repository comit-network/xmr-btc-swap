//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
pub use self::{
    event_loop::{EventLoop, EventLoopHandle},
    message0::Message0,
    message1::Message1,
    message4::Message4,
    state::*,
    swap::{run, run_until},
    swap_response::*,
};
use crate::{
    bitcoin,
    config::Config,
    database,
    database::Database,
    monero,
    network::{
        peer_tracker::{self, PeerTracker},
        request_response::AliceToBob,
        transport::build,
        Seed as NetworkSeed,
    },
    protocol::{bob, bob::Message5, SwapAmounts},
    seed::Seed,
};
use anyhow::{bail, Result};
use libp2p::{
    core::Multiaddr, identity::Keypair, request_response::ResponseChannel, NetworkBehaviour, PeerId,
};
use rand::rngs::OsRng;
use std::{path::PathBuf, sync::Arc};
use tracing::{debug, info};
use uuid::Uuid;

pub mod event_loop;
mod message0;
mod message1;
mod message2;
mod message4;
mod message5;
pub mod state;
mod steps;
pub mod swap;
mod swap_response;

pub struct Swap {
    pub state: AliceState,
    pub event_loop_handle: EventLoopHandle,
    pub bitcoin_wallet: Arc<bitcoin::Wallet>,
    pub monero_wallet: Arc<monero::Wallet>,
    pub config: Config,
    pub swap_id: Uuid,
    pub db: Database,
}

pub struct Builder {
    swap_id: Uuid,
    identity: Keypair,
    peer_id: PeerId,
    db_path: PathBuf,
    config: Config,

    listen_address: Multiaddr,

    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,

    init_params: InitParams,
}

enum InitParams {
    None,
    New { swap_amounts: SwapAmounts },
}

impl Builder {
    pub async fn new(
        seed: Seed,
        config: Config,
        swap_id: Uuid,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        db_path: PathBuf,
        listen_address: Multiaddr,
    ) -> Self {
        let network_seed = NetworkSeed::new(seed);
        let identity = network_seed.derive_libp2p_identity();
        let peer_id = PeerId::from(identity.public());

        Self {
            swap_id,
            identity,
            peer_id,
            db_path,
            config,
            listen_address,
            bitcoin_wallet,
            monero_wallet,
            init_params: InitParams::None,
        }
    }

    pub fn with_init_params(self, swap_amounts: SwapAmounts) -> Self {
        Self {
            init_params: InitParams::New { swap_amounts },
            ..self
        }
    }

    pub async fn build(self) -> Result<(Swap, EventLoop)> {
        match self.init_params {
            InitParams::New { swap_amounts } => {
                let initial_state = self
                    .make_initial_state(swap_amounts.btc, swap_amounts.xmr)
                    .await?;

                let (event_loop, event_loop_handle) = self.init_event_loop()?;

                let db = Database::open(self.db_path.as_path())?;

                Ok((
                    Swap {
                        event_loop_handle,
                        bitcoin_wallet: self.bitcoin_wallet,
                        monero_wallet: self.monero_wallet,
                        config: self.config,
                        db,
                        state: initial_state,
                        swap_id: self.swap_id,
                    },
                    event_loop,
                ))
            }
            InitParams::None => {
                // reopen the existing database
                let db = Database::open(self.db_path.as_path())?;

                let resume_state =
                    if let database::Swap::Alice(state) = db.get_state(self.swap_id)? {
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
                        bitcoin_wallet: self.bitcoin_wallet,
                        monero_wallet: self.monero_wallet,
                        config: self.config,
                        swap_id: self.swap_id,
                        db,
                    },
                    event_loop,
                ))
            }
        }
    }

    pub fn peer_id(&self) -> PeerId {
        self.peer_id.clone()
    }

    pub fn listen_address(&self) -> Multiaddr {
        self.listen_address.clone()
    }

    async fn make_initial_state(
        &self,
        btc_to_swap: bitcoin::Amount,
        xmr_to_swap: monero::Amount,
    ) -> Result<AliceState> {
        let rng = &mut OsRng;

        let amounts = SwapAmounts {
            btc: btc_to_swap,
            xmr: xmr_to_swap,
        };

        let a = bitcoin::SecretKey::new_random(rng);
        let s_a = cross_curve_dleq::Scalar::random(rng);
        let v_a = monero::PrivateViewKey::new_random(rng);
        let redeem_address = self.bitcoin_wallet.new_address().await?;
        let punish_address = redeem_address.clone();
        let state0 = State0::new(
            a,
            s_a,
            v_a,
            amounts.btc,
            amounts.xmr,
            self.config.bitcoin_cancel_timelock,
            self.config.bitcoin_punish_timelock,
            redeem_address,
            punish_address,
        );

        Ok(AliceState::Started { amounts, state0 })
    }

    fn init_event_loop(&self) -> Result<(EventLoop, EventLoopHandle)> {
        let alice_behaviour = Behaviour::default();
        let alice_transport = build(self.identity.clone())?;
        EventLoop::new(
            alice_transport,
            alice_behaviour,
            self.listen_address.clone(),
            self.peer_id.clone(),
        )
    }
}

#[derive(Debug)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
    // TODO (Franck): Change this to get both amounts so parties can verify the amounts are
    // expected early on.
    Request(Box<swap_response::OutEvent>), /* Not-uniform with Bob on purpose, ready for adding
                                            * Xmr
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
        msg: Box<bob::Message2>,
        bob_peer_id: PeerId,
    },
    Message4,
    Message5(Message5),
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

impl From<swap_response::OutEvent> for OutEvent {
    fn from(event: swap_response::OutEvent) -> Self {
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
            message2::OutEvent::Msg { msg, bob_peer_id } => OutEvent::Message2 {
                msg: Box::new(msg),
                bob_peer_id,
            },
        }
    }
}

impl From<message4::OutEvent> for OutEvent {
    fn from(event: message4::OutEvent) -> Self {
        match event {
            message4::OutEvent::Msg => OutEvent::Message4,
        }
    }
}

impl From<message5::OutEvent> for OutEvent {
    fn from(event: message5::OutEvent) -> Self {
        match event {
            message5::OutEvent::Msg(msg) => OutEvent::Message5(msg),
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
#[derive(NetworkBehaviour, Default)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pt: PeerTracker,
    amounts: swap_response::Behaviour,
    message0: message0::Behaviour,
    message1: message1::Behaviour,
    message2: message2::Behaviour,
    message4: message4::Behaviour,
    message5: message5::Behaviour,
}

impl Behaviour {
    /// Alice always sends her messages as a response to a request from Bob.
    pub fn send_swap_response(
        &mut self,
        channel: ResponseChannel<AliceToBob>,
        swap_response: SwapResponse,
    ) {
        self.amounts.send(channel, swap_response);
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

    /// Send Message4 to Bob.
    pub fn send_message4(&mut self, bob: PeerId, msg: Message4) {
        self.message4.send(bob, msg);
        debug!("Sent Message 4");
    }
}
