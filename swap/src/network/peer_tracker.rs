use futures::task::Context;
use libp2p::core::connection::ConnectionId;
use libp2p::core::ConnectedPoint;
use libp2p::swarm::protocols_handler::DummyProtocolsHandler;
use libp2p::swarm::{NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p::{Multiaddr, PeerId};
use std::collections::{HashMap, VecDeque};
use std::task::Poll;

#[derive(Debug, Copy, Clone)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
}

/// A NetworkBehaviour that tracks connections to the counterparty. Although the
/// libp2p `NetworkBehaviour` abstraction encompasses connections to multiple
/// peers we only ever connect to a single counterparty. Peer Tracker tracks
/// that connection.
#[derive(Default, Debug)]
pub struct Behaviour {
    connected: Option<(PeerId, Multiaddr)>,
    address_of_peer: HashMap<PeerId, Multiaddr>,
    events: VecDeque<OutEvent>,
}

impl Behaviour {
    /// Return whether we are connected to the given peer.
    pub fn is_connected(&self, peer_id: &PeerId) -> bool {
        if let Some((connected_peer_id, _)) = &self.connected {
            if connected_peer_id == peer_id {
                return true;
            }
        }
        false
    }

    /// Returns the peer id of counterparty if we are connected.
    pub fn counterparty_peer_id(&self) -> Option<PeerId> {
        if let Some((id, _)) = &self.connected {
            return Some(*id);
        }
        None
    }

    /// Returns the peer_id and multiaddr of counterparty if we are connected.
    pub fn counterparty(&self) -> Option<(PeerId, Multiaddr)> {
        if let Some((peer_id, addr)) = &self.connected {
            return Some((*peer_id, addr.clone()));
        }
        None
    }

    /// Add an address for a given peer. We only store one address per peer.
    pub fn add_address(&mut self, peer_id: PeerId, address: Multiaddr) {
        self.address_of_peer.insert(peer_id, address);
    }
}

impl NetworkBehaviour for Behaviour {
    type ProtocolsHandler = DummyProtocolsHandler;
    type OutEvent = OutEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        DummyProtocolsHandler::default()
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        let mut addresses: Vec<Multiaddr> = vec![];

        if let Some((counterparty_peer_id, addr)) = self.counterparty() {
            if counterparty_peer_id == *peer_id {
                addresses.push(addr)
            }
        }

        if let Some(addr) = self.address_of_peer.get(peer_id) {
            addresses.push(addr.clone());
        }

        addresses
    }

    fn inject_connected(&mut self, _: &PeerId) {}

    fn inject_disconnected(&mut self, _: &PeerId) {}

    fn inject_connection_established(
        &mut self,
        peer: &PeerId,
        _: &ConnectionId,
        point: &ConnectedPoint,
    ) {
        match point {
            ConnectedPoint::Dialer { address } => {
                self.connected = Some((*peer, address.clone()));
            }
            ConnectedPoint::Listener {
                local_addr: _,
                send_back_addr,
            } => {
                self.connected = Some((*peer, send_back_addr.clone()));
            }
        }

        self.events
            .push_back(OutEvent::ConnectionEstablished(*peer));
    }

    fn inject_connection_closed(&mut self, _: &PeerId, _: &ConnectionId, _: &ConnectedPoint) {
        self.connected = None;
    }

    fn inject_event(&mut self, _: PeerId, _: ConnectionId, _: void::Void) {}

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<void::Void, Self::OutEvent>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }

        Poll::Pending
    }
}
