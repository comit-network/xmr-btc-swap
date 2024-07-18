use crate::cli;
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use futures::future::FutureExt;
use libp2p::core::connection::ConnectionId;
use libp2p::core::Multiaddr;
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::protocols_handler::DummyProtocolsHandler;
use libp2p::swarm::{NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p::PeerId;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::{Instant, Sleep};
use void::Void;

pub enum OutEvent {
    AllAttemptsExhausted { peer: PeerId },
}

/// A [`NetworkBehaviour`] that tracks whether we are connected to the given
/// peer and attempts to re-establish a connection with an exponential backoff
/// if we lose the connection.
pub struct Behaviour {
    /// The peer we are interested in.
    peer: PeerId,
    /// If present, tracks for how long we need to sleep until we dial again.
    sleep: Option<Pin<Box<Sleep>>>,
    /// Tracks the current backoff state.
    backoff: ExponentialBackoff,
}

impl Behaviour {
    pub fn new(peer: PeerId, interval: Duration) -> Self {
        Self {
            peer,
            sleep: None,
            backoff: ExponentialBackoff {
                initial_interval: interval,
                current_interval: interval,
                // give up dialling after 5 minutes
                max_elapsed_time: Some(Duration::from_secs(5 * 60)),
                ..ExponentialBackoff::default()
            },
        }
    }

    pub fn until_next_redial(&self) -> Option<Duration> {
        let until_next_redial = self
            .sleep
            .as_ref()?
            .deadline()
            .checked_duration_since(Instant::now())?;

        Some(until_next_redial)
    }
}

impl NetworkBehaviour for Behaviour {
    type ProtocolsHandler = DummyProtocolsHandler;
    type OutEvent = OutEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        DummyProtocolsHandler::default()
    }

    fn addresses_of_peer(&mut self, _: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
    }

    fn inject_connected(&mut self, peer_id: &PeerId) {
        if peer_id != &self.peer {
            return;
        }

        // established a connection to the desired peer, cancel any active re-dialling
        self.sleep = None;
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        if peer_id != &self.peer {
            return;
        }

        // lost connection to the configured peer, trigger re-dialling with an
        // exponential backoff
        self.backoff.reset();
        self.sleep = Some(Box::pin(tokio::time::sleep(self.backoff.initial_interval)));
    }

    fn inject_event(&mut self, _: PeerId, _: ConnectionId, _: Void) {}

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ProtocolsHandler>> {
        let sleep = match self.sleep.as_mut() {
            None => return Poll::Pending, // early exit if we shouldn't be re-dialling
            Some(future) => future,
        };

        futures::ready!(sleep.poll_unpin(cx));

        let next_dial_in = match self.backoff.next_backoff() {
            Some(next_dial_in) => next_dial_in,
            None => {
                return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    OutEvent::AllAttemptsExhausted { peer: self.peer },
                ));
            }
        };

        self.sleep = Some(Box::pin(tokio::time::sleep(next_dial_in)));

        Poll::Ready(NetworkBehaviourAction::Dial {
            opts: DialOpts::peer_id(self.peer)
                .condition(PeerCondition::Disconnected)
                .build(),
            handler: Self::ProtocolsHandler::default(),
        })
    }
}

impl From<OutEvent> for cli::OutEvent {
    fn from(event: OutEvent) -> Self {
        match event {
            OutEvent::AllAttemptsExhausted { peer } => {
                cli::OutEvent::AllRedialAttemptsExhausted { peer }
            }
        }
    }
}
