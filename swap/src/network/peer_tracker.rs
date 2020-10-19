use futures::task::Context;
use libp2p::{
    core::{connection::ConnectionId, ConnectedPoint},
    swarm::{
        protocols_handler::DummyProtocolsHandler, NetworkBehaviour, NetworkBehaviourAction,
        PollParameters,
    },
    Multiaddr, PeerId,
};
use std::{collections::VecDeque, task::Poll};

#[derive(Debug)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
}

/// A NetworkBehaviour that tracks connections to the counterparty. Although the
/// libp2p `NetworkBehaviour` abstraction encompasses connections to multiple
/// peers we only ever connect to a single counterparty. Peer Tracker tracks
/// that connection.
#[derive(Default, Debug)]
pub struct PeerTracker {
    connected: Option<(PeerId, Multiaddr)>,
    events: VecDeque<OutEvent>,
}

impl PeerTracker {
    /// Returns an arbitrary connected counterparty.
    /// This is useful if we are connected to a single other node.
    pub fn counterparty_peer_id(&self) -> Option<PeerId> {
        if let Some((id, _)) = &self.connected {
            return Some(id.clone());
        }
        None
    }

    /// Returns an arbitrary connected counterparty.
    /// This is useful if we are connected to a single other node.
    pub fn counterparty_addr(&self) -> Option<Multiaddr> {
        if let Some((_, addr)) = &self.connected {
            return Some(addr.clone());
        }
        None
    }
}

impl NetworkBehaviour for PeerTracker {
    type ProtocolsHandler = DummyProtocolsHandler;
    type OutEvent = OutEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        DummyProtocolsHandler::default()
    }

    fn addresses_of_peer(&mut self, _: &PeerId) -> Vec<Multiaddr> {
        let mut addresses: Vec<Multiaddr> = vec![];

        if let Some(addr) = self.counterparty_addr() {
            addresses.push(addr)
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
                self.connected = Some((peer.clone(), address.clone()));
            }
            ConnectedPoint::Listener {
                local_addr: _,
                send_back_addr,
            } => {
                self.connected = Some((peer.clone(), send_back_addr.clone()));
            }
        }

        self.events
            .push_back(OutEvent::ConnectionEstablished(peer.clone()));
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
