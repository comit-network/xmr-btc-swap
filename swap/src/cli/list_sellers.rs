use crate::cli::api::tauri_bindings::{
    ListSellersProgress, TauriBackgroundProgress, TauriBackgroundProgressHandle, TauriEmitter,
    TauriHandle,
};
use crate::network::quote::BidQuote;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::network::{quote, swarm};
use crate::protocol::Database;
use anyhow::Result;
use arti_client::TorClient;
use futures::StreamExt;
use libp2p::identify;
use libp2p::multiaddr::Protocol;
use libp2p::request_response;
use libp2p::swarm::dial_opts::DialOpts;
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{identity, ping, rendezvous, Multiaddr, PeerId, Swarm};
use semver::Version;
use serde::Serialize;
use serde_with::{serde_as, DisplayFromStr};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tor_rtcompat::tokio::TokioRustlsRuntime;
use typeshare::typeshare;

/// Builds an identify config for the CLI with appropriate protocol and agent versions.
/// This allows peers to identify our client version and protocol compatibility.
fn build_identify_config(identity: identity::Keypair) -> identify::Config {
    let protocol_version = "/comit/xmr/btc/1.0.0".to_string();
    let agent_version = format!("cli/{}", env!("CARGO_PKG_VERSION"));
    identify::Config::new(protocol_version, identity.public()).with_agent_version(agent_version)
}

/// Returns sorted list of sellers, with [Online](Status::Online) listed first.
///
/// First uses the rendezvous node to discover peers in the given namespace,
/// then fetches a quote from each peer that was discovered. If fetching a quote
/// from a discovered peer fails the seller's status will be
/// [Unreachable](Status::Unreachable).
///
/// If a database is provided, it will be used to get the list of peers that
/// have already been discovered previously and attempt to fetch a quote from them.
pub async fn list_sellers(
    rendezvous_points: Vec<(PeerId, Multiaddr)>,
    namespace: XmrBtcNamespace,
    maybe_tor_client: Option<Arc<TorClient<TokioRustlsRuntime>>>,
    identity: identity::Keypair,
    db: Option<Arc<dyn Database + Send + Sync>>,
    tauri_handle: Option<TauriHandle>,
) -> Result<Vec<SellerStatus>> {
    let behaviour = Behaviour {
        rendezvous: rendezvous::client::Behaviour::new(identity.clone()),
        quote: quote::cli(),
        ping: ping::Behaviour::new(ping::Config::new().with_timeout(Duration::from_secs(60))),
        identify: identify::Behaviour::new(build_identify_config(identity.clone())),
    };
    let swarm = swarm::cli(identity, maybe_tor_client, behaviour).await?;

    // If a database is passed in: Fetch all peer addresses from the database and fetch quotes from them
    let external_dial_queue = match db {
        Some(db) => {
            let peers = db.get_all_peer_addresses().await?;
            VecDeque::from(peers)
        }
        None => VecDeque::new(),
    };

    let event_loop = EventLoop::new(
        swarm,
        rendezvous_points,
        namespace,
        external_dial_queue,
        tauri_handle,
    );
    let sellers = event_loop.run().await;

    Ok(sellers)
}

#[serde_as]
#[typeshare]
#[derive(Debug, Serialize, PartialEq, Eq, Hash, Clone, Ord, PartialOrd)]
pub struct QuoteWithAddress {
    /// The multiaddr of the seller (at which we were able to connect to and get the quote from)
    #[serde_as(as = "DisplayFromStr")]
    #[typeshare(serialized_as = "string")]
    pub multiaddr: Multiaddr,

    /// The peer id of the seller
    #[typeshare(serialized_as = "string")]
    pub peer_id: PeerId,

    /// The quote of the seller
    pub quote: BidQuote,

    /// The version of the seller's agent
    #[serde_as(as = "DisplayFromStr")]
    #[typeshare(serialized_as = "string")]
    pub version: Version,
}

#[typeshare]
#[derive(Debug, Serialize, PartialEq, Eq, Hash, Clone, Ord, PartialOrd)]
pub struct UnreachableSeller {
    /// The peer id of the seller
    #[typeshare(serialized_as = "string")]
    pub peer_id: PeerId,
}

#[typeshare]
#[derive(Debug, Serialize, PartialEq, Eq, Hash, Clone, Ord, PartialOrd)]
#[serde(tag = "type", content = "content")]
pub enum SellerStatus {
    Online(QuoteWithAddress),
    Unreachable(UnreachableSeller),
}

#[allow(unused)]
#[derive(Debug)]
enum OutEvent {
    Rendezvous(rendezvous::client::Event),
    Quote(quote::OutEvent),
    Ping(ping::Event),
    Identify(Box<identify::Event>),
}

#[derive(NetworkBehaviour)]
#[behaviour(event_process = false)]
#[behaviour(out_event = "OutEvent")]
struct Behaviour {
    rendezvous: rendezvous::client::Behaviour,
    quote: quote::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
}

#[derive(Debug, Clone)]
enum PeerState {
    /// Initial state with just the peer ID
    Initial { peer_id: PeerId },
    /// We have received a reachable address
    HasAddress {
        peer_id: PeerId,
        reachable_addresses: Vec<Multiaddr>,
    },
    /// We have received the version
    HasVersion {
        peer_id: PeerId,
        version: Version,
        reachable_addresses: Vec<Multiaddr>,
    },
    /// We have received the quote
    HasQuote {
        peer_id: PeerId,
        quote: BidQuote,
        reachable_addresses: Vec<Multiaddr>,
    },
    /// We have received both address and version
    HasAddressAndVersion {
        peer_id: PeerId,
        version: Version,
        reachable_addresses: Vec<Multiaddr>,
    },
    /// We have received both address and quote
    HasAddressAndQuote {
        peer_id: PeerId,
        quote: BidQuote,
        reachable_addresses: Vec<Multiaddr>,
    },
    /// We have received both version and quote
    HasVersionAndQuote {
        peer_id: PeerId,
        version: Version,
        quote: BidQuote,
        reachable_addresses: Vec<Multiaddr>,
    },
    /// We have received all three: address, version, and quote
    Complete {
        peer_id: PeerId,
        version: Version,
        quote: BidQuote,
        reachable_addresses: Vec<Multiaddr>,
    },
    /// The peer failed with an error
    Failed {
        peer_id: PeerId,
        error_message: String,
        reachable_addresses: Vec<Multiaddr>,
    },
}

/// Extracts the semver version from a user agent string.
/// Example input: "asb/2.0.0 (xmr-btc-swap-mainnet)"
/// Returns None if the version cannot be parsed.
fn extract_semver_from_agent_str(agent_str: &str) -> Option<Version> {
    // Split on '/' and take the second part
    let version_str = agent_str.split('/').nth(1)?;
    // Split on whitespace and take the first part
    let version_str = version_str.split_whitespace().next()?;
    // Parse the version string
    Version::parse(version_str).ok()
}

impl PeerState {
    fn new(peer_id: PeerId) -> Self {
        Self::Initial { peer_id }
    }

    fn add_reachable_address(self, address: Multiaddr) -> Self {
        let reachable_addresses = self.get_reachable_addresses();
        let mut new_reachable_addresses = reachable_addresses.clone();

        if !new_reachable_addresses.contains(&address) {
            new_reachable_addresses.push(address);
        }

        match self {
            Self::Initial { peer_id } => Self::HasAddress {
                peer_id,
                reachable_addresses: new_reachable_addresses,
            },
            Self::HasVersion {
                peer_id, version, ..
            } => Self::HasAddressAndVersion {
                peer_id,
                version,
                reachable_addresses: new_reachable_addresses,
            },
            Self::HasQuote { peer_id, quote, .. } => Self::HasAddressAndQuote {
                peer_id,
                quote,
                reachable_addresses: new_reachable_addresses,
            },
            Self::HasVersionAndQuote {
                peer_id,
                version,
                quote,
                ..
            } => Self::Complete {
                peer_id,
                version,
                quote,
                reachable_addresses: new_reachable_addresses,
            },
            Self::HasAddress { peer_id, .. } => Self::HasAddress {
                peer_id,
                reachable_addresses: new_reachable_addresses,
            },
            Self::HasAddressAndVersion {
                peer_id, version, ..
            } => Self::HasAddressAndVersion {
                peer_id,
                version,
                reachable_addresses: new_reachable_addresses,
            },
            Self::HasAddressAndQuote { peer_id, quote, .. } => Self::HasAddressAndQuote {
                peer_id,
                quote,
                reachable_addresses: new_reachable_addresses,
            },
            Self::Complete {
                peer_id,
                version,
                quote,
                ..
            } => Self::Complete {
                peer_id,
                version,
                quote,
                reachable_addresses: new_reachable_addresses,
            },
            Self::Failed {
                peer_id,
                error_message,
                ..
            } => Self::Failed {
                peer_id,
                error_message,
                reachable_addresses: new_reachable_addresses,
            },
        }
    }

    fn apply_quote(self, quote_result: Result<BidQuote>) -> Self {
        match (self, quote_result) {
            (state, Ok(quote)) => {
                let reachable_addresses = state.get_reachable_addresses();
                match state {
                    Self::Initial { peer_id } => Self::HasQuote {
                        peer_id,
                        quote,
                        reachable_addresses,
                    },
                    Self::HasAddress { peer_id, .. } => Self::HasAddressAndQuote {
                        peer_id,
                        quote,
                        reachable_addresses,
                    },
                    Self::HasVersion {
                        peer_id, version, ..
                    } => Self::HasVersionAndQuote {
                        peer_id,
                        version,
                        quote,
                        reachable_addresses,
                    },
                    Self::HasAddressAndVersion {
                        peer_id, version, ..
                    } => Self::Complete {
                        peer_id,
                        version,
                        quote,
                        reachable_addresses,
                    },
                    Self::HasQuote { .. }
                    | Self::HasAddressAndQuote { .. }
                    | Self::HasVersionAndQuote { .. }
                    | Self::Complete { .. } => state,
                    Self::Failed { .. } => state,
                }
            }
            (state, Err(error)) => {
                let reachable_addresses = state.get_reachable_addresses();
                Self::Failed {
                    peer_id: state.get_peer_id(),
                    error_message: error.to_string(),
                    reachable_addresses,
                }
            }
        }
    }

    fn apply_version(self, version: String) -> Self {
        let reachable_addresses = self.get_reachable_addresses();

        match extract_semver_from_agent_str(version.as_str()) {
            Some(version) => match self {
                Self::Initial { peer_id } => Self::HasVersion {
                    peer_id,
                    version,
                    reachable_addresses,
                },
                Self::HasAddress { peer_id, .. } => Self::HasAddressAndVersion {
                    peer_id,
                    version,
                    reachable_addresses,
                },
                Self::HasQuote { peer_id, quote, .. } => Self::HasVersionAndQuote {
                    peer_id,
                    version,
                    quote,
                    reachable_addresses,
                },
                Self::HasAddressAndQuote { peer_id, quote, .. } => Self::Complete {
                    peer_id,
                    version,
                    quote,
                    reachable_addresses,
                },
                Self::HasVersion { .. }
                | Self::HasAddressAndVersion { .. }
                | Self::HasVersionAndQuote { .. }
                | Self::Complete { .. } => self,
                Self::Failed { .. } => self,
            },
            None => self.mark_failed(format!(
                "Failed to parse version from user agent: {}",
                version
            )),
        }
    }

    fn mark_failed(self, error_message: String) -> Self {
        let reachable_addresses = self.get_reachable_addresses();
        Self::Failed {
            peer_id: self.get_peer_id(),
            error_message,
            reachable_addresses,
        }
    }

    fn get_peer_id(&self) -> PeerId {
        match self {
            Self::Initial { peer_id }
            | Self::HasAddress { peer_id, .. }
            | Self::HasVersion { peer_id, .. }
            | Self::HasQuote { peer_id, .. }
            | Self::HasAddressAndVersion { peer_id, .. }
            | Self::HasAddressAndQuote { peer_id, .. }
            | Self::HasVersionAndQuote { peer_id, .. }
            | Self::Complete { peer_id, .. }
            | Self::Failed { peer_id, .. } => *peer_id,
        }
    }

    fn get_reachable_addresses(&self) -> Vec<Multiaddr> {
        match self {
            Self::Initial { .. } => Vec::new(),
            Self::HasAddress {
                reachable_addresses,
                ..
            }
            | Self::HasVersion {
                reachable_addresses,
                ..
            }
            | Self::HasQuote {
                reachable_addresses,
                ..
            }
            | Self::HasAddressAndVersion {
                reachable_addresses,
                ..
            }
            | Self::HasAddressAndQuote {
                reachable_addresses,
                ..
            }
            | Self::HasVersionAndQuote {
                reachable_addresses,
                ..
            }
            | Self::Complete {
                reachable_addresses,
                ..
            }
            | Self::Failed {
                reachable_addresses,
                ..
            } => reachable_addresses.clone(),
        }
    }

    fn is_pending(&self) -> bool {
        !matches!(self, Self::Complete { .. } | Self::Failed { .. })
    }
}

#[derive(Debug)]
enum RendezvousPointStatus {
    Dialed,  // We have initiated dialing but do not know if it succeeded or not
    Failed,  // We have initiated dialing but we failed to connect OR failed to discover
    Success, // We have connected to the rendezvous point and discovered peers
}

impl RendezvousPointStatus {
    // A rendezvous point has been "completed" if it is either successfully dialed or failed
    fn is_complete(&self) -> bool {
        matches!(
            self,
            RendezvousPointStatus::Success | RendezvousPointStatus::Failed
        )
    }
}

struct EventLoop {
    swarm: Swarm<Behaviour>,

    /// The namespace to discover peers in
    namespace: XmrBtcNamespace,

    /// List to store which rendezvous points we have either dialed / failed to dial
    rendezvous_points_status: HashMap<PeerId, RendezvousPointStatus>,

    /// The rendezvous points to dial
    rendezvous_points: Vec<(PeerId, Multiaddr)>,

    /// The addresses of peers that have been discovered and are reachable
    reachable_asb_address: HashMap<PeerId, Multiaddr>,

    /// The state of each peer
    /// The state contains a mini state machine
    peer_states: HashMap<PeerId, PeerState>,

    /// The queue of peers to dial
    /// When we discover a peer we add it is then dialed by the event loop
    to_request_quote: VecDeque<(PeerId, Vec<Multiaddr>)>,

    /// Background progress handle for UI updates
    progress_handle: Option<TauriBackgroundProgressHandle<ListSellersProgress>>,
}

impl EventLoop {
    fn new(
        swarm: Swarm<Behaviour>,
        rendezvous_points: Vec<(PeerId, Multiaddr)>,
        namespace: XmrBtcNamespace,
        dial_queue: VecDeque<(PeerId, Vec<Multiaddr>)>,
        tauri_handle: Option<TauriHandle>,
    ) -> Self {
        let progress_handle =
            tauri_handle.new_background_process(TauriBackgroundProgress::ListSellers);

        Self {
            swarm,
            rendezvous_points_status: Default::default(),
            rendezvous_points,
            namespace,
            reachable_asb_address: Default::default(),
            peer_states: Default::default(),
            to_request_quote: dial_queue,
            progress_handle: Some(progress_handle),
        }
    }

    fn is_rendezvous_point(&self, peer_id: &PeerId) -> bool {
        self.rendezvous_points
            .iter()
            .any(|(rendezvous_peer_id, _)| rendezvous_peer_id == peer_id)
    }

    fn get_rendezvous_point(&self, peer_id: &PeerId) -> Option<Multiaddr> {
        self.rendezvous_points
            .iter()
            .find(|(rendezvous_peer_id, _)| rendezvous_peer_id == peer_id)
            .map(|(_, multiaddr)| multiaddr.clone())
    }

    fn get_progress(&self) -> ListSellersProgress {
        let rendezvous_connected = self
            .rendezvous_points_status
            .values()
            .filter(|status| matches!(status, RendezvousPointStatus::Success))
            .count()
            .try_into()
            .unwrap_or(u32::MAX);

        let rendezvous_failed = self
            .rendezvous_points_status
            .values()
            .filter(|status| matches!(status, RendezvousPointStatus::Failed))
            .count()
            .try_into()
            .unwrap_or(u32::MAX);

        let quotes_received = self
            .peer_states
            .values()
            .filter(|state| matches!(state, PeerState::Complete { .. }))
            .count()
            .try_into()
            .unwrap_or(u32::MAX);

        let quotes_failed = self
            .peer_states
            .values()
            .filter(|state| matches!(state, PeerState::Failed { .. }))
            .count()
            .try_into()
            .unwrap_or(u32::MAX);

        ListSellersProgress {
            rendezvous_points_connected: rendezvous_connected,
            rendezvous_points_total: self.rendezvous_points.len().try_into().unwrap_or(u32::MAX),
            rendezvous_points_failed: rendezvous_failed,
            peers_discovered: self.peer_states.len().try_into().unwrap_or(u32::MAX),
            quotes_received,
            quotes_failed,
        }
    }

    fn emit_progress(&self) {
        if let Some(ref progress_handle) = self.progress_handle {
            progress_handle.update(self.get_progress());
        }
    }

    fn ensure_multiaddr_has_p2p_suffix(&self, peer_id: PeerId, multiaddr: Multiaddr) -> Multiaddr {
        let p2p_suffix = Protocol::P2p(peer_id);

        // If the multiaddr does not end with the p2p suffix, we add it
        if !multiaddr.ends_with(&Multiaddr::empty().with(p2p_suffix.clone())) {
            multiaddr.clone().with(p2p_suffix)
        } else {
            // If the multiaddr already ends with the p2p suffix, we return it as is
            multiaddr.clone()
        }
    }

    async fn run(mut self) -> Vec<SellerStatus> {
        // Dial all rendezvous points initially
        for (peer_id, multiaddr) in &self.rendezvous_points {
            let dial_opts = DialOpts::peer_id(*peer_id)
                .addresses(vec![multiaddr.clone()])
                .extend_addresses_through_behaviour()
                .build();

            self.rendezvous_points_status
                .insert(*peer_id, RendezvousPointStatus::Dialed);

            if let Err(e) = self.swarm.dial(dial_opts) {
                tracing::error!(%peer_id, %multiaddr, error = %e, "Failed to dial rendezvous point");

                self.rendezvous_points_status
                    .insert(*peer_id, RendezvousPointStatus::Failed);
            }
        }

        loop {
            self.emit_progress();

            tokio::select! {
                Some((peer_id, multiaddresses)) = async { self.to_request_quote.pop_front() } => {
                    // We do not allow an overlap of rendezvous points and quote requests
                    // because if we do we cannot distinguish between a quote request and a rendezvous point later on
                    // because we are missing state information to
                    if self.is_rendezvous_point(&peer_id) {
                        continue;
                    }

                    // If we already have an entry for this peer, we skip it
                    // We probably discovered a peer at a rendezvous point which we already have an entry for locally
                    if self.peer_states.contains_key(&peer_id) {
                        continue;
                    }

                    // Initialize peer state
                    self.peer_states.insert(peer_id, PeerState::new(peer_id));

                    // Add all known addresses of this peer to the swarm
                    for multiaddr in multiaddresses {
                        self.swarm.add_peer_address(peer_id, multiaddr);
                    }

                    // Request a quote from the peer
                    let _request_id = self.swarm.behaviour_mut().quote.send_request(&peer_id, ());
                }
                swarm_event = self.swarm.select_next_some() => {
                    match swarm_event {
                        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                            if self.is_rendezvous_point(&peer_id) {
                                tracing::info!(
                                    "Connected to rendezvous point, discovering nodes in '{}' namespace ...",
                                    self.namespace
                                );

                                let namespace = rendezvous::Namespace::new(self.namespace.to_string()).expect("our namespace to be a correct string");

                                self.swarm.behaviour_mut().rendezvous.discover(
                                    Some(namespace),
                                    None,
                                    None,
                                    peer_id,
                                );
                            } else {
                                let address = endpoint.get_remote_address();
                                tracing::debug!(%peer_id, %address, "Connection established to peer for list-sellers");
                                self.reachable_asb_address.insert(peer_id, address.clone());

                                // Update the peer state with the reachable address
                                if let Some(state) = self.peer_states.remove(&peer_id) {
                                    let new_state = state.add_reachable_address(address.clone());
                                    self.peer_states.insert(peer_id, new_state);
                                }
                            }
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                            if let Some(peer_id) = peer_id {
                                if let Some(rendezvous_point) = self.get_rendezvous_point(&peer_id) {
                                    tracing::warn!(
                                        %peer_id,
                                        %rendezvous_point,
                                        "Failed to connect to rendezvous point: {}",
                                        error
                                    );

                                    // Update the status of the rendezvous point to failed
                                    self.rendezvous_points_status.insert(peer_id, RendezvousPointStatus::Failed);
                                } else {
                                    tracing::warn!(
                                        %peer_id,
                                        "Failed to connect to peer: {}",
                                        error
                                    );

                                    if let Some(state) = self.peer_states.remove(&peer_id) {
                                        let failed_state = state.mark_failed(format!("Failed to connect to peer: {}", error));
                                        self.peer_states.insert(peer_id, failed_state);
                                    }
                                }
                            } else {
                                tracing::warn!("Failed to connect (no peer id): {}", error);
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::Rendezvous(
                            libp2p::rendezvous::client::Event::Discovered { registrations, rendezvous_node, .. },
                        )) => {
                            tracing::debug!(%rendezvous_node, num_peers = %registrations.len(), "Discovered peers at rendezvous point");

                            for registration in registrations {
                                let peer = registration.record.peer_id();
                                let addresses = registration.record.addresses().iter().map(|addr| self.ensure_multiaddr_has_p2p_suffix(peer, addr.clone())).collect::<Vec<_>>();

                                self.to_request_quote.push_back((peer, addresses));
                            }

                            // Update the status of the rendezvous point to success
                            self.rendezvous_points_status.insert(rendezvous_node, RendezvousPointStatus::Success);
                        }
                        SwarmEvent::Behaviour(OutEvent::Rendezvous(
                            libp2p::rendezvous::client::Event::DiscoverFailed { rendezvous_node, .. },
                        )) => {
                            self.rendezvous_points_status.insert(rendezvous_node, RendezvousPointStatus::Failed);
                        }
                        SwarmEvent::Behaviour(OutEvent::Quote(quote_response)) => {
                            match quote_response {
                                request_response::Event::Message { peer, message } => {
                                    match message {
                                        request_response::Message::Response { response, .. } => {
                                            if let Some(state) = self.peer_states.remove(&peer) {
                                                let new_state = state.apply_quote(Ok(response));
                                                self.peer_states.insert(peer, new_state);
                                            } else {
                                                tracing::warn!(%peer, "Received bid quote from unexpected peer, this record will be removed!");
                                            }
                                        }
                                        request_response::Message::Request { .. } => unreachable!("we only request quotes, not respond")
                                    }
                                }
                                request_response::Event::OutboundFailure { peer, error, .. } => {
                                    if self.is_rendezvous_point(&peer) {
                                        tracing::debug!(%peer, "Outbound failure when communicating with rendezvous node: {:#}", error);

                                        // Update the status of the rendezvous point to failed
                                        self.rendezvous_points_status.insert(peer, RendezvousPointStatus::Failed);
                                    } else if let Some(state) = self.peer_states.remove(&peer) {
                                        let failed_state = state.apply_quote(Err(anyhow::anyhow!("Quote request failed: {}", error)));
                                        self.peer_states.insert(peer, failed_state);
                                    }
                                }
                                request_response::Event::InboundFailure { peer, error, .. } => {
                                    if self.is_rendezvous_point(&peer) {
                                        tracing::debug!(%peer, "Inbound failure when communicating with rendezvous node: {:#}", error);

                                        // Update the status of the rendezvous point to failed
                                        self.rendezvous_points_status.insert(peer, RendezvousPointStatus::Failed);
                                    } else if let Some(state) = self.peer_states.remove(&peer) {
                                        let failed_state = state.mark_failed(format!("Inbound failure: {}", error));
                                        self.peer_states.insert(peer, failed_state);
                                    }
                                },
                                request_response::Event::ResponseSent { .. } => unreachable!()
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::Identify(event)) => {
                            match *event {
                                identify::Event::Received { peer_id, info } => {
                                    if let Some(state) = self.peer_states.remove(&peer_id) {
                                        let new_state = state.apply_version(info.agent_version);
                                        self.peer_states.insert(peer_id, new_state);
                                    }
                                }
                                identify::Event::Error { peer_id, error } => {
                                    tracing::error!(%peer_id, error = %error, "Error when identifying peer");

                                    if let Some(state) = self.peer_states.remove(&peer_id) {
                                        let failed_state = state.mark_failed(format!("Error when identifying peer: {}", error));
                                        self.peer_states.insert(peer_id, failed_state);
                                    }
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }

            // We are finished if both of these conditions are true
            // 1. All rendezvous points have been successfully dialed or failed to dial / discover at namespace
            // 2. We don't have any pending quote requests
            // 3. We received quotes OR failed to from all peers we have requested quotes from

            // Check if all peer ids from rendezvous_points are present in rendezvous_points_status
            // Check if every entry in rendezvous_points_status is "complete"
            let all_rendezvous_points_requests_complete =
                self.rendezvous_points.iter().all(|(peer_id, _)| {
                    self.rendezvous_points_status
                        .get(peer_id)
                        .map(|status| status.is_complete())
                        .unwrap_or(false)
                });

            // Check if to_request_quote is empty
            let all_quotes_fetched = self.to_request_quote.is_empty();

            // If we have pending request to rendezvous points or quote requests, we continue
            if !all_rendezvous_points_requests_complete || !all_quotes_fetched {
                continue;
            }

            let all_quotes_fetched = self
                .peer_states
                .values()
                .map(|peer_state| match peer_state {
                    state if state.is_pending() => Err(StillPending {}),
                    PeerState::Complete {
                        peer_id,
                        version,
                        quote,
                        reachable_addresses,
                    } => Ok(SellerStatus::Online(QuoteWithAddress {
                        peer_id: *peer_id,
                        multiaddr: reachable_addresses[0].clone(),
                        quote: *quote,
                        version: version.clone(),
                    })),
                    PeerState::Failed {
                        peer_id,
                        error_message,
                        ..
                    } => {
                        tracing::warn!(%peer_id, error = %error_message, "Peer failed");

                        Ok(SellerStatus::Unreachable(UnreachableSeller {
                            peer_id: *peer_id,
                        }))
                    }
                    _ => unreachable!("All cases should be covered above"),
                })
                .collect::<Result<Vec<_>, _>>();

            match all_quotes_fetched {
                Ok(mut sellers) => {
                    sellers.sort();
                    if let Some(ref progress_handle) = self.progress_handle {
                        progress_handle.finish();
                    }
                    break sellers;
                }
                Err(StillPending {}) => continue,
            }
        }
    }
}

#[derive(Debug)]
struct StillPending {}

impl From<rendezvous::client::Event> for OutEvent {
    fn from(event: rendezvous::client::Event) -> Self {
        OutEvent::Rendezvous(event)
    }
}

impl From<quote::OutEvent> for OutEvent {
    fn from(event: quote::OutEvent) -> Self {
        OutEvent::Quote(event)
    }
}

impl From<identify::Event> for OutEvent {
    fn from(event: identify::Event) -> Self {
        OutEvent::Identify(Box::new(event))
    }
}

impl From<ping::Event> for OutEvent {
    fn from(event: ping::Event) -> Self {
        OutEvent::Ping(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    // Helper function to create a test multiaddr
    fn test_multiaddr() -> Multiaddr {
        "/ip4/127.0.0.1/tcp/8080".parse().unwrap()
    }

    // Helper function to create a test PeerId
    fn test_peer_id() -> PeerId {
        PeerId::random()
    }

    // Helper function to create a test BidQuote
    fn test_bid_quote() -> BidQuote {
        BidQuote {
            price: bitcoin::Amount::from_sat(50000),
            min_quantity: bitcoin::Amount::from_sat(1000),
            max_quantity: bitcoin::Amount::from_sat(100000),
        }
    }

    // Helper function to create a test Version
    fn test_version() -> Version {
        Version::parse("1.2.3").unwrap()
    }

    mod extract_semver_tests {
        use super::*;

        #[test]
        fn extract_semver_from_asb_agent_string() {
            assert_eq!(
                extract_semver_from_agent_str("asb/2.0.0 (xmr-btc-swap-mainnet)"),
                Some(Version::parse("2.0.0").unwrap())
            );
        }

        #[test]
        fn extract_semver_from_cli_agent_string() {
            assert_eq!(
                extract_semver_from_agent_str("cli/1.5.2"),
                Some(Version::parse("1.5.2").unwrap())
            );
        }

        #[test]
        fn extract_semver_with_prerelease() {
            assert_eq!(
                extract_semver_from_agent_str("asb/2.1.0-beta.2.1 (xmr-btc-swap-testnet)"),
                Some(Version::parse("2.1.0-beta.2.1").unwrap())
            );
        }

        #[test]
        fn extract_semver_invalid_format() {
            assert_eq!(extract_semver_from_agent_str("invalid-format"), None);
        }

        #[test]
        fn extract_semver_no_slash() {
            assert_eq!(extract_semver_from_agent_str("asb-2.0.0"), None);
        }

        #[test]
        fn extract_semver_invalid_version() {
            assert_eq!(extract_semver_from_agent_str("asb/invalid.version"), None);
        }

        #[test]
        fn extract_semver_empty_version() {
            assert_eq!(extract_semver_from_agent_str("asb/"), None);
        }
    }

    mod peer_state_tests {
        use super::*;

        #[test]
        fn new_peer_state_starts_as_initial() {
            let peer_id = test_peer_id();
            let state = PeerState::new(peer_id);

            assert!(matches!(state, PeerState::Initial { .. }));
            assert_eq!(state.get_peer_id(), peer_id);
            assert_eq!(state.get_reachable_addresses(), Vec::<Multiaddr>::new());
            assert!(state.is_pending());
        }

        #[test]
        fn initial_add_address_transitions_to_has_address() {
            let peer_id = test_peer_id();
            let address = test_multiaddr();
            let state = PeerState::new(peer_id);

            let new_state = state.add_reachable_address(address.clone());

            match &new_state {
                PeerState::HasAddress {
                    peer_id: p,
                    reachable_addresses,
                } => {
                    assert_eq!(*p, peer_id);
                    assert_eq!(*reachable_addresses, vec![address]);
                }
                _ => panic!("Expected HasAddress state"),
            }
            assert!(new_state.is_pending());
        }

        #[test]
        fn initial_apply_quote_transitions_to_has_quote() {
            let peer_id = test_peer_id();
            let quote = test_bid_quote();
            let state = PeerState::new(peer_id);

            let new_state = state.apply_quote(Ok(quote));

            match &new_state {
                PeerState::HasQuote {
                    peer_id: p,
                    quote: q,
                    reachable_addresses,
                } => {
                    assert_eq!(*p, peer_id);
                    assert_eq!(*q, quote);
                    assert_eq!(*reachable_addresses, Vec::<Multiaddr>::new());
                }
                _ => panic!("Expected HasQuote state"),
            }
            assert!(new_state.is_pending());
        }

        #[test]
        fn initial_apply_version_transitions_to_has_version() {
            let peer_id = test_peer_id();
            let version_str = "asb/1.2.3".to_string();
            let state = PeerState::new(peer_id);

            let new_state = state.apply_version(version_str);

            match &new_state {
                PeerState::HasVersion {
                    peer_id: p,
                    version,
                    reachable_addresses,
                } => {
                    assert_eq!(*p, peer_id);
                    assert_eq!(*version, Version::parse("1.2.3").unwrap());
                    assert_eq!(*reachable_addresses, Vec::<Multiaddr>::new());
                }
                _ => panic!("Expected HasVersion state"),
            }
            assert!(new_state.is_pending());
        }

        #[test]
        fn has_address_apply_quote_transitions_to_has_address_and_quote() {
            let peer_id = test_peer_id();
            let address = test_multiaddr();
            let quote = test_bid_quote();

            let state = PeerState::HasAddress {
                peer_id,
                reachable_addresses: vec![address.clone()],
            };

            let new_state = state.apply_quote(Ok(quote));

            match &new_state {
                PeerState::HasAddressAndQuote {
                    peer_id: p,
                    quote: q,
                    reachable_addresses,
                } => {
                    assert_eq!(*p, peer_id);
                    assert_eq!(*q, quote);
                    assert_eq!(*reachable_addresses, vec![address]);
                }
                _ => panic!("Expected HasAddressAndQuote state"),
            }
            assert!(new_state.is_pending());
        }

        #[test]
        fn has_address_apply_version_transitions_to_has_address_and_version() {
            let peer_id = test_peer_id();
            let address = test_multiaddr();
            let version_str = "cli/2.1.0".to_string();

            let state = PeerState::HasAddress {
                peer_id,
                reachable_addresses: vec![address.clone()],
            };

            let new_state = state.apply_version(version_str);

            match &new_state {
                PeerState::HasAddressAndVersion {
                    peer_id: p,
                    version,
                    reachable_addresses,
                } => {
                    assert_eq!(*p, peer_id);
                    assert_eq!(*version, Version::parse("2.1.0").unwrap());
                    assert_eq!(*reachable_addresses, vec![address]);
                }
                _ => panic!("Expected HasAddressAndVersion state"),
            }
            assert!(new_state.is_pending());
        }

        #[test]
        fn has_version_apply_quote_transitions_to_has_version_and_quote() {
            let peer_id = test_peer_id();
            let version = test_version();
            let quote = test_bid_quote();

            let state = PeerState::HasVersion {
                peer_id,
                version: version.clone(),
                reachable_addresses: vec![],
            };

            let new_state = state.apply_quote(Ok(quote));

            match &new_state {
                PeerState::HasVersionAndQuote {
                    peer_id: p,
                    version: v,
                    quote: q,
                    reachable_addresses,
                } => {
                    assert_eq!(*p, peer_id);
                    assert_eq!(*v, version);
                    assert_eq!(*q, quote);
                    assert_eq!(*reachable_addresses, Vec::<Multiaddr>::new());
                }
                _ => panic!("Expected HasVersionAndQuote state"),
            }
            assert!(new_state.is_pending());
        }

        #[test]
        fn has_quote_apply_version_transitions_to_has_version_and_quote() {
            let peer_id = test_peer_id();
            let quote = test_bid_quote();
            let version_str = "asb/3.0.0".to_string();

            let state = PeerState::HasQuote {
                peer_id,
                quote,
                reachable_addresses: vec![],
            };

            let new_state = state.apply_version(version_str);

            match &new_state {
                PeerState::HasVersionAndQuote {
                    peer_id: p,
                    version,
                    quote: q,
                    reachable_addresses,
                } => {
                    assert_eq!(*p, peer_id);
                    assert_eq!(*version, Version::parse("3.0.0").unwrap());
                    assert_eq!(*q, quote);
                    assert_eq!(*reachable_addresses, Vec::<Multiaddr>::new());
                }
                _ => panic!("Expected HasVersionAndQuote state"),
            }
            assert!(new_state.is_pending());
        }

        #[test]
        fn has_address_and_version_apply_quote_transitions_to_complete() {
            let peer_id = test_peer_id();
            let address = test_multiaddr();
            let version = test_version();
            let quote = test_bid_quote();

            let state = PeerState::HasAddressAndVersion {
                peer_id,
                version: version.clone(),
                reachable_addresses: vec![address.clone()],
            };

            let new_state = state.apply_quote(Ok(quote));

            match &new_state {
                PeerState::Complete {
                    peer_id: p,
                    version: v,
                    quote: q,
                    reachable_addresses,
                } => {
                    assert_eq!(*p, peer_id);
                    assert_eq!(*v, version);
                    assert_eq!(*q, quote);
                    assert_eq!(*reachable_addresses, vec![address]);
                }
                _ => panic!("Expected Complete state"),
            }
            assert!(!new_state.is_pending());
        }

        #[test]
        fn has_address_and_quote_apply_version_transitions_to_complete() {
            let peer_id = test_peer_id();
            let address = test_multiaddr();
            let quote = test_bid_quote();
            let version_str = "cli/1.0.0".to_string();

            let state = PeerState::HasAddressAndQuote {
                peer_id,
                quote,
                reachable_addresses: vec![address.clone()],
            };

            let new_state = state.apply_version(version_str);

            match &new_state {
                PeerState::Complete {
                    peer_id: p,
                    version,
                    quote: q,
                    reachable_addresses,
                } => {
                    assert_eq!(*p, peer_id);
                    assert_eq!(*version, Version::parse("1.0.0").unwrap());
                    assert_eq!(*q, quote);
                    assert_eq!(*reachable_addresses, vec![address]);
                }
                _ => panic!("Expected Complete state"),
            }
            assert!(!new_state.is_pending());
        }

        #[test]
        fn has_version_and_quote_add_address_transitions_to_complete() {
            let peer_id = test_peer_id();
            let address = test_multiaddr();
            let version = test_version();
            let quote = test_bid_quote();

            let state = PeerState::HasVersionAndQuote {
                peer_id,
                version: version.clone(),
                quote,
                reachable_addresses: vec![],
            };

            let new_state = state.add_reachable_address(address.clone());

            match &new_state {
                PeerState::Complete {
                    peer_id: p,
                    version: v,
                    quote: q,
                    reachable_addresses,
                } => {
                    assert_eq!(*p, peer_id);
                    assert_eq!(*v, version);
                    assert_eq!(*q, quote);
                    assert_eq!(*reachable_addresses, vec![address]);
                }
                _ => panic!("Expected Complete state"),
            }
            assert!(!new_state.is_pending());
        }

        #[test]
        fn apply_failed_quote_transitions_to_failed() {
            let peer_id = test_peer_id();
            let address = test_multiaddr();
            let error = anyhow!("Network error");

            let state = PeerState::HasAddress {
                peer_id,
                reachable_addresses: vec![address.clone()],
            };

            let new_state = state.apply_quote(Err(error));

            match &new_state {
                PeerState::Failed {
                    peer_id: p,
                    error_message,
                    reachable_addresses,
                } => {
                    assert_eq!(*p, peer_id);
                    assert_eq!(error_message, "Network error");
                    assert_eq!(*reachable_addresses, vec![address]);
                }
                _ => panic!("Expected Failed state"),
            }
            assert!(!new_state.is_pending());
        }

        #[test]
        fn apply_invalid_version_transitions_to_failed() {
            let peer_id = test_peer_id();
            let invalid_version = "invalid-version-string".to_string();

            let state = PeerState::new(peer_id);
            let new_state = state.apply_version(invalid_version.clone());

            match &new_state {
                PeerState::Failed {
                    peer_id: p,
                    error_message,
                    reachable_addresses,
                } => {
                    assert_eq!(*p, peer_id);
                    assert!(error_message.contains("Failed to parse version"));
                    assert!(error_message.contains(&invalid_version));
                    assert_eq!(*reachable_addresses, Vec::<Multiaddr>::new());
                }
                _ => panic!("Expected Failed state"),
            }
            assert!(!new_state.is_pending());
        }

        #[test]
        fn mark_failed_from_any_state() {
            let peer_id = test_peer_id();
            let address = test_multiaddr();
            let error_message = "Connection timeout".to_string();

            // Test from Initial
            let state = PeerState::new(peer_id);
            let failed = state.mark_failed(error_message.clone());
            assert!(matches!(failed, PeerState::Failed { .. }));
            assert!(!failed.is_pending());

            // Test from HasAddress
            let state = PeerState::HasAddress {
                peer_id,
                reachable_addresses: vec![address.clone()],
            };
            let failed = state.mark_failed(error_message.clone());
            match failed {
                PeerState::Failed {
                    peer_id: p,
                    error_message: msg,
                    reachable_addresses,
                } => {
                    assert_eq!(p, peer_id);
                    assert_eq!(msg, error_message);
                    assert_eq!(reachable_addresses, vec![address.clone()]);
                }
                _ => panic!("Expected Failed state"),
            }

            // Test from Complete
            let state = PeerState::Complete {
                peer_id,
                version: test_version(),
                quote: test_bid_quote(),
                reachable_addresses: vec![address.clone()],
            };
            let failed = state.mark_failed(error_message.clone());
            assert!(matches!(failed, PeerState::Failed { .. }));
        }

        #[test]
        fn add_duplicate_address_does_not_duplicate() {
            let peer_id = test_peer_id();
            let address = test_multiaddr();

            let state = PeerState::HasAddress {
                peer_id,
                reachable_addresses: vec![address.clone()],
            };

            let new_state = state.add_reachable_address(address.clone());

            match new_state {
                PeerState::HasAddress {
                    reachable_addresses,
                    ..
                } => {
                    assert_eq!(reachable_addresses.len(), 1);
                    assert_eq!(reachable_addresses[0], address);
                }
                _ => panic!("Expected HasAddress state"),
            }
        }

        #[test]
        fn add_multiple_addresses() {
            let peer_id = test_peer_id();
            let address1 = test_multiaddr();
            let address2: Multiaddr = "/ip4/192.168.1.1/tcp/9090".parse().unwrap();

            let state = PeerState::HasAddress {
                peer_id,
                reachable_addresses: vec![address1.clone()],
            };

            let new_state = state.add_reachable_address(address2.clone());

            match &new_state {
                PeerState::HasAddress {
                    reachable_addresses,
                    ..
                } => {
                    assert_eq!(reachable_addresses.len(), 2);
                    assert!(reachable_addresses.contains(&address1));
                    assert!(reachable_addresses.contains(&address2));
                }
                _ => panic!("Expected HasAddress state"),
            }
        }

        #[test]
        fn operations_on_complete_state_are_idempotent() {
            let peer_id = test_peer_id();
            let address = test_multiaddr();
            let version = test_version();
            let quote = test_bid_quote();

            let state = PeerState::Complete {
                peer_id,
                version: version.clone(),
                quote,
                reachable_addresses: vec![address.clone()],
            };

            // Apply quote again - should remain unchanged
            let new_quote = BidQuote {
                price: bitcoin::Amount::from_sat(99999),
                min_quantity: bitcoin::Amount::from_sat(1),
                max_quantity: bitcoin::Amount::from_sat(1000),
            };
            let new_state = state.apply_quote(Ok(new_quote));

            match &new_state {
                PeerState::Complete { quote: q, .. } => {
                    assert_eq!(*q, quote); // Original quote, not new_quote
                }
                _ => panic!("Expected Complete state to remain unchanged"),
            }

            // Apply version again - should remain unchanged
            let new_state = new_state.apply_version("asb/9.9.9".to_string());
            match &new_state {
                PeerState::Complete { version: v, .. } => {
                    assert_eq!(*v, version); // Original version
                }
                _ => panic!("Expected Complete state to remain unchanged"),
            }
        }

        #[test]
        fn operations_on_failed_state_are_idempotent() {
            let peer_id = test_peer_id();
            let address = test_multiaddr();
            let error_message = "Original error".to_string();

            let state = PeerState::Failed {
                peer_id,
                error_message: error_message.clone(),
                reachable_addresses: vec![address.clone()],
            };

            // Apply quote - should remain failed
            let new_state = state.apply_quote(Ok(test_bid_quote()));
            match &new_state {
                PeerState::Failed {
                    error_message: msg, ..
                } => {
                    assert_eq!(msg, &error_message);
                }
                _ => panic!("Expected Failed state to remain unchanged"),
            }

            // Apply version - should remain failed
            let new_state = new_state.apply_version("asb/1.0.0".to_string());
            match &new_state {
                PeerState::Failed {
                    error_message: msg, ..
                } => {
                    assert_eq!(msg, &error_message);
                }
                _ => panic!("Expected Failed state to remain unchanged"),
            }
        }
    }

    mod rendezvous_point_status_tests {
        use super::*;

        #[test]
        fn rendezvous_point_status_completion() {
            assert!(!RendezvousPointStatus::Dialed.is_complete());
            assert!(RendezvousPointStatus::Failed.is_complete());
            assert!(RendezvousPointStatus::Success.is_complete());
        }
    }

    #[test]
    fn sellers_sort_with_unreachable_coming_last() {
        let mut list = vec![
            SellerStatus::Unreachable(UnreachableSeller {
                peer_id: PeerId::random(),
            }),
            SellerStatus::Unreachable(UnreachableSeller {
                peer_id: PeerId::random(),
            }),
            SellerStatus::Online(QuoteWithAddress {
                multiaddr: "/ip4/127.0.0.1/tcp/5678".parse().unwrap(),
                peer_id: PeerId::random(),
                quote: BidQuote {
                    price: Default::default(),
                    min_quantity: Default::default(),
                    max_quantity: Default::default(),
                },
                version: Version::parse("1.0.0").unwrap(), // Fixed: Use valid semver
            }),
        ];

        list.sort();

        // Check that online sellers come first
        assert!(matches!(list[0], SellerStatus::Online(_)));
        assert!(matches!(list[1], SellerStatus::Unreachable(_)));
        assert!(matches!(list[2], SellerStatus::Unreachable(_)));
    }
}
