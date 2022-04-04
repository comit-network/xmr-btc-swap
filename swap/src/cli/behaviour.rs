use crate::network::quote::BidQuote;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::network::swap_setup::bob;
use crate::network::{encrypted_signature, quote, redial, transfer_proof};
use crate::protocol::bob::State2;
use crate::{bitcoin, env};
use anyhow::{anyhow, Error, Result};
use libp2p::core::Multiaddr;
use libp2p::identify::{Identify, IdentifyConfig, IdentifyEvent};
use libp2p::ping::{Ping, PingConfig, PingEvent};
use libp2p::request_response::{RequestId, ResponseChannel};
use libp2p::{identity, NetworkBehaviour, PeerId};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
pub enum OutEvent {
    QuoteReceived {
        id: RequestId,
        response: BidQuote,
    },
    SwapSetupCompleted(Box<Result<State2>>),
    TransferProofReceived {
        msg: Box<transfer_proof::Request>,
        channel: ResponseChannel<()>,
        peer: PeerId,
    },
    EncryptedSignatureAcknowledged {
        id: RequestId,
    },
    AllRedialAttemptsExhausted {
        peer: PeerId,
    },
    Failure {
        peer: PeerId,
        error: Error,
    },
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

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Bob.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pub quote: quote::Behaviour,
    pub swap_setup: bob::Behaviour,
    pub transfer_proof: transfer_proof::Behaviour,
    pub encrypted_signature: encrypted_signature::Behaviour,
    pub redial: redial::Behaviour,
    pub identify: Identify,

    /// Ping behaviour that ensures that the underlying network connection is
    /// still alive. If the ping fails a connection close event will be
    /// emitted that is picked up as swarm event.
    ping: Ping,
}

impl Behaviour {
    pub fn new(
        alice: PeerId,
        env_config: env::Config,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        identify_params: (identity::Keypair, XmrBtcNamespace),
    ) -> Self {
        let agentVersion = format!("cli/{} ({})", env!("CARGO_PKG_VERSION"), identify_params.1);
        let protocolVersion = "/comit/xmr/btc/1.0.0".to_string();
        let identifyConfig = IdentifyConfig::new(protocolVersion, identify_params.0.public())
            .with_agent_version(agentVersion);

        Self {
            quote: quote::cli(),
            swap_setup: bob::Behaviour::new(env_config, bitcoin_wallet),
            transfer_proof: transfer_proof::bob(),
            encrypted_signature: encrypted_signature::bob(),
            redial: redial::Behaviour::new(alice, Duration::from_secs(2)),
            ping: Ping::new(PingConfig::new().with_keep_alive(true)),
            identify: Identify::new(identifyConfig),
        }
    }

    /// Add a known address for the given peer
    pub fn add_address(&mut self, peer_id: PeerId, address: Multiaddr) {
        self.quote.add_address(&peer_id, address.clone());
        self.transfer_proof.add_address(&peer_id, address.clone());
        self.encrypted_signature.add_address(&peer_id, address);
    }
}

impl From<PingEvent> for OutEvent {
    fn from(_: PingEvent) -> Self {
        OutEvent::Other
    }
}

impl From<IdentifyEvent> for OutEvent {
    fn from(_: IdentifyEvent) -> Self {
        OutEvent::Other
    }
}
