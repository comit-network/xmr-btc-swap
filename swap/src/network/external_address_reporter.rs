use crate::protocol::alice;
use libp2p::core::connection::ConnectionId;
use libp2p::core::Multiaddr;
use libp2p::swarm::protocols_handler::DummyProtocolsHandler;
use libp2p::swarm::{NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p::PeerId;
use std::collections::VecDeque;
use std::task::{Context, Poll};
use void::Void;

/// A NetworkBehaviour that emits all discovered external addresses as events.
///
/// This allows us to process them in the same manner as all other events that
/// happen in the network layer.
#[derive(Debug, Default)]
pub struct Behaviour {
    addresses_to_report: VecDeque<Multiaddr>,
}

pub struct ExternalAddress {
    pub addr: Multiaddr,
}

impl NetworkBehaviour for Behaviour {
    type ProtocolsHandler = DummyProtocolsHandler;
    type OutEvent = ExternalAddress;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        DummyProtocolsHandler::default()
    }

    fn addresses_of_peer(&mut self, _: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
    }

    fn inject_connected(&mut self, _: &PeerId) {}

    fn inject_disconnected(&mut self, _: &PeerId) {}

    fn inject_event(&mut self, _: PeerId, _: ConnectionId, _: Void) {}

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        self.addresses_to_report.push_back(addr.clone());
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Void, Self::OutEvent>> {
        if let Some(addr) = self.addresses_to_report.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(ExternalAddress {
                addr,
            }));
        }

        Poll::Pending
    }
}

impl From<ExternalAddress> for alice::OutEvent {
    fn from(external: ExternalAddress) -> Self {
        Self::NewExternalAddress {
            addr: external.addr,
        }
    }
}
