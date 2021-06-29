use crate::asb::event_loop::LatestRate;
use crate::env;
use crate::network::quote::BidQuote;
use crate::network::swap_setup::alice;
use crate::network::swap_setup::alice::WalletSnapshot;
use crate::network::{encrypted_signature, quote, transfer_proof};
use crate::protocol::alice::State3;
use anyhow::{anyhow, Error};
use libp2p::identity::Keypair;
use libp2p::ping::{Ping, PingEvent};
use libp2p::rendezvous::{Event, Namespace, RegisterError};
use libp2p::request_response::{RequestId, ResponseChannel};
use libp2p::{rendezvous, NetworkBehaviour, PeerId};
use uuid::Uuid;

#[derive(Debug)]
pub enum OutEvent {
    SwapSetupInitiated {
        send_wallet_snapshot: bmrng::RequestReceiver<bitcoin::Amount, WalletSnapshot>,
    },
    SwapSetupCompleted {
        peer_id: PeerId,
        swap_id: Uuid,
        state3: Box<State3>,
    },
    SwapDeclined {
        peer: PeerId,
        error: alice::Error,
    },
    QuoteRequested {
        channel: ResponseChannel<BidQuote>,
        peer: PeerId,
    },
    TransferProofAcknowledged {
        peer: PeerId,
        id: RequestId,
    },
    EncryptedSignatureReceived {
        msg: Box<encrypted_signature::Request>,
        channel: ResponseChannel<()>,
        peer: PeerId,
    },
    Failure {
        peer: PeerId,
        error: Error,
    },
    Registered {
        rendezvous_node: PeerId,
        ttl: u64,
        namespace: Namespace,
    },
    RegisterFailed(RegisterError),
    /// "Fallback" variant that allows the event mapping code to swallow certain
    /// events that we don't want the caller to deal with.
    Other,
}

impl OutEvent {
    pub fn unexpected_request(peer: PeerId) -> OutEvent {
        OutEvent::Failure {
            peer,
            error: anyhow!("Unexpected request received"),
        }
    }

    pub fn unexpected_response(peer: PeerId) -> OutEvent {
        OutEvent::Failure {
            peer,
            error: anyhow!("Unexpected response received"),
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour<LR>
where
    LR: LatestRate + Send + 'static,
{
    pub rendezvous: rendezvous::Rendezvous,
    pub quote: quote::Behaviour,
    pub swap_setup: alice::Behaviour<LR>,
    pub transfer_proof: transfer_proof::Behaviour,
    pub encrypted_signature: encrypted_signature::Behaviour,

    /// Ping behaviour that ensures that the underlying network connection is
    /// still alive. If the ping fails a connection close event will be
    /// emitted that is picked up as swarm event.
    ping: Ping,
}

impl<LR> Behaviour<LR>
where
    LR: LatestRate + Send + 'static,
{
    pub fn new(
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
        latest_rate: LR,
        resume_only: bool,
        env_config: env::Config,
        keypair: Keypair,
    ) -> Self {
        Self {
            rendezvous: rendezvous::Rendezvous::new(keypair, rendezvous::Config::default()),
            quote: quote::asb(),
            swap_setup: alice::Behaviour::new(
                min_buy,
                max_buy,
                env_config,
                latest_rate,
                resume_only,
            ),
            transfer_proof: transfer_proof::alice(),
            encrypted_signature: encrypted_signature::alice(),
            ping: Ping::default(),
        }
    }
}

impl From<PingEvent> for OutEvent {
    fn from(_: PingEvent) -> Self {
        OutEvent::Other
    }
}

impl From<rendezvous::Event> for OutEvent {
    fn from(event: rendezvous::Event) -> Self {
        match event {
            Event::Discovered { .. } => unreachable!("The ASB does not discover other nodes"),
            Event::DiscoverFailed { .. } => unreachable!("The ASB does not discover other nodes"),
            Event::Registered {
                rendezvous_node,
                ttl,
                namespace,
            } => OutEvent::Registered {
                rendezvous_node,
                ttl,
                namespace,
            },
            Event::RegisterFailed(error) => OutEvent::RegisterFailed(error),
            Event::DiscoverServed { .. } => unreachable!("ASB does not act as rendezvous node"),
            Event::DiscoverNotServed { .. } => unreachable!("ASB does not act as rendezvous node"),
            Event::PeerRegistered { .. } => unreachable!("ASB does not act as rendezvous node"),
            Event::PeerNotRegistered { .. } => unreachable!("ASB does not act as rendezvous node"),
            Event::PeerUnregistered { .. } => unreachable!("ASB does not act as rendezvous node"),
            Event::RegistrationExpired(_) => unreachable!("ASB does not act as rendezvous node"),
        }
    }
}
