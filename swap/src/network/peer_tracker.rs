use futures::task::Context;
use libp2p::{
    core::{connection::ConnectionId, ConnectedPoint},
    swarm::{
        protocols_handler::DummyProtocolsHandler, NetworkBehaviour, NetworkBehaviourAction,
        PollParameters,
    },
    Multiaddr, PeerId,
};
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    task::Poll,
};

#[derive(Debug)]
pub enum BehaviourOutEvent {
    ConnectionEstablished(PeerId),
}

/// A NetworkBehaviour that tracks connections to other peers.
#[derive(Default, Debug)]
pub struct PeerTracker {
    connected_peers: HashMap<PeerId, Vec<Multiaddr>>,
    address_hints: HashMap<PeerId, VecDeque<Multiaddr>>,
    events: VecDeque<BehaviourOutEvent>,
}

impl PeerTracker {
    /// Returns an arbitrary connected counterparty.
    /// This is useful if we are connected to a single other node.
    pub fn counterparty(&self) -> Option<PeerId> {
        // TODO: Refactor to use combinators.
        if let Some((id, _)) = self.connected_peers().next() {
            return Some(id);
        }
        None
    }

    pub fn connected_peers(&self) -> impl Iterator<Item = (PeerId, Vec<Multiaddr>)> {
        self.connected_peers.clone().into_iter()
    }

    /// Adds an address hint for the given peer id. The added address is
    /// considered most recent and hence is added at the start of the list
    /// because libp2p tries to connect with the first address first.
    pub fn add_recent_address_hint(&mut self, id: PeerId, addr: Multiaddr) {
        let old_addresses = self.address_hints.get_mut(&id);

        match old_addresses {
            None => {
                let mut hints = VecDeque::new();
                hints.push_back(addr);
                self.address_hints.insert(id, hints);
            }
            Some(hints) => {
                hints.push_front(addr);
            }
        }
    }
}

impl NetworkBehaviour for PeerTracker {
    type ProtocolsHandler = DummyProtocolsHandler;
    type OutEvent = BehaviourOutEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        DummyProtocolsHandler::default()
    }

    /// Note (from libp2p doc):
    /// The addresses will be tried in the order returned by this function,
    /// which means that they should be ordered by decreasing likelihood of
    /// reachability. In other words, the first address should be the most
    /// likely to be reachable.
    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        let mut addresses: Vec<Multiaddr> = vec![];

        // If we are connected then this address is most likely to be valid
        if let Some(connected) = self.connected_peers.get(peer) {
            for addr in connected.iter() {
                addresses.push(addr.clone())
            }
        }

        if let Some(hints) = self.address_hints.get(peer) {
            for hint in hints {
                addresses.push(hint.clone());
            }
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
        if let ConnectedPoint::Dialer { address } = point {
            self.connected_peers
                .entry(peer.clone())
                .or_default()
                .push(address.clone());

            self.events
                .push_back(BehaviourOutEvent::ConnectionEstablished(peer.clone()));
        }
    }

    fn inject_connection_closed(
        &mut self,
        peer: &PeerId,
        _: &ConnectionId,
        point: &ConnectedPoint,
    ) {
        if let ConnectedPoint::Dialer { address } = point {
            match self.connected_peers.entry(peer.clone()) {
                Entry::Vacant(_) => {}
                Entry::Occupied(mut entry) => {
                    let addresses = entry.get_mut();

                    if let Some(pos) = addresses.iter().position(|a| a == address) {
                        addresses.remove(pos);
                    }
                }
            }
        }
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
