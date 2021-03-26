use crate::network::quote::BidQuote;
use crate::network::{encrypted_signature, quote, spot_price, transfer_proof};
use crate::protocol::alice::{execution_setup, State3};
use anyhow::{anyhow, Error, Result};
use libp2p::core::Multiaddr;
use libp2p::kad::store::MemoryStore;
use libp2p::kad::{BootstrapResult, Kademlia, KademliaEvent, QueryResult, QueryStats};
use libp2p::request_response::{RequestId, ResponseChannel};
use libp2p::{NetworkBehaviour, PeerId};

#[derive(Debug)]
pub enum OutEvent {
    SpotPriceRequested {
        request: spot_price::Request,
        channel: ResponseChannel<spot_price::Response>,
        peer: PeerId,
    },
    QuoteRequested {
        channel: ResponseChannel<BidQuote>,
        peer: PeerId,
    },
    ExecutionSetupDone {
        bob_peer_id: PeerId,
        state3: Box<State3>,
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
    BootstrapComplete {
        result: BootstrapResult,
        stats: QueryStats,
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

impl From<KademliaEvent> for OutEvent {
    fn from(event: KademliaEvent) -> Self {
        match event {
            KademliaEvent::QueryResult {
                result: QueryResult::Bootstrap(result),
                stats,
                ..
            } => OutEvent::BootstrapComplete { result, stats },
            _ => OutEvent::Other,
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pub quote: quote::Behaviour,
    pub spot_price: spot_price::Behaviour,
    pub execution_setup: execution_setup::Behaviour,
    pub transfer_proof: transfer_proof::Behaviour,
    pub encrypted_signature: encrypted_signature::Behaviour,
    pub dht: Kademlia<MemoryStore>,
}

impl From<PeerId> for Behaviour {
    fn from(peer_id: PeerId) -> Self {
        Self {
            quote: quote::alice(),
            spot_price: spot_price::alice(),
            execution_setup: Default::default(),
            transfer_proof: transfer_proof::alice(),
            encrypted_signature: encrypted_signature::alice(),
            dht: Kademlia::new(peer_id, MemoryStore::new(peer_id)),
        }
    }
}

impl Behaviour {
    pub fn bootstrap(&mut self) -> Result<()> {
        tracing::info!("Starting DHT bootstrap");

        let address = "/dnsaddr/bootstrap.libp2p.io".parse::<Multiaddr>()?;

        for node in LIBP2P_BOOTSTRAP_NODES {
            self.dht.add_address(&node.parse()?, address.clone());
        }

        self.dht.bootstrap()?;

        Ok(())
    }
}

const LIBP2P_BOOTSTRAP_NODES: &[&str] = &[
    "QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
    "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
    "QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
    "QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
];
