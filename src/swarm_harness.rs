use libp2p::futures::future;
use libp2p::futures::future::FutureExt;
use libp2p::{
    core::{muxing::StreamMuxerBox, transport::memory::MemoryTransport, upgrade::Version},
    identity,
    noise::{self, NoiseConfig, X25519Spec},
    swarm::{NetworkBehaviour, SwarmBuilder, SwarmEvent},
    yamux::YamuxConfig,
    Multiaddr, PeerId, Swarm, Transport,
};
use std::{fmt::Debug, future::Future, time::Duration};
use tokio::runtime::Handle;
use tokio::time;

#[allow(missing_debug_implementations)]
pub struct Actor<B: NetworkBehaviour> {
    pub swarm: Swarm<B>,
    pub addr: Multiaddr,
    pub peer_id: PeerId,
}

pub async fn new_connected_swarm_pair<B, F>(behaviour_fn: F, handle: Handle) -> (Actor<B>, Actor<B>)
where
    B: NetworkBehaviour,
    F: Fn(PeerId, identity::Keypair) -> B + Clone,
    <B as NetworkBehaviour>::OutEvent: Debug,
{
    let (swarm, addr, peer_id) = new_swarm(behaviour_fn.clone(), handle.clone());
    let mut alice = Actor {
        swarm,
        addr,
        peer_id,
    };

    let (swarm, addr, peer_id) = new_swarm(behaviour_fn, handle);
    let mut bob = Actor {
        swarm,
        addr,
        peer_id,
    };

    connect(&mut alice.swarm, &mut bob.swarm).await;

    (alice, bob)
}

pub fn new_swarm<B: NetworkBehaviour, F: Fn(PeerId, identity::Keypair) -> B>(
    behaviour_fn: F,
    handle: Handle,
) -> (Swarm<B>, Multiaddr, PeerId) {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());

    let dh_keys = noise::Keypair::<X25519Spec>::new()
        .into_authentic(&id_keys)
        .expect("failed to create dh_keys");
    let noise = NoiseConfig::xx(dh_keys).into_authenticated();

    let transport = MemoryTransport::default()
        .upgrade(Version::V1)
        .authenticate(noise)
        .multiplex(YamuxConfig::default())
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    let mut swarm: Swarm<B> = SwarmBuilder::new(transport, behaviour_fn(peer_id, id_keys), peer_id)
        .executor(Box::new(move |f| {
            handle.spawn(f);
        }))
        .build();

    let address_port = rand::random::<u64>();
    let addr = format!("/memory/{}", address_port)
        .parse::<Multiaddr>()
        .unwrap();

    Swarm::listen_on(&mut swarm, addr.clone()).unwrap();

    (swarm, addr, peer_id)
}

pub async fn await_events_or_timeout<A, B>(
    alice_event: impl Future<Output = A>,
    bob_event: impl Future<Output = B>,
) -> (A, B) {
    time::timeout(
        Duration::from_secs(10),
        future::join(alice_event, bob_event),
    )
    .await
    .expect("network behaviours to emit an event within 10 seconds")
}

/// Connects two swarms with each other.
///
/// This assumes the transport that is in use can be used by Alice to connect to
/// the listen address that is emitted by Bob. In other words, they have to be
/// on the same network. The memory transport used by the above `new_swarm`
/// function fulfills this.
///
/// We also assume that the swarms don't emit any behaviour events during the
/// connection phase. Any event emitted is considered a bug from this functions
/// PoV because they would be lost.
pub async fn connect<B>(alice: &mut Swarm<B>, bob: &mut Swarm<B>)
where
    B: NetworkBehaviour,
    <B as NetworkBehaviour>::OutEvent: Debug,
{
    let mut alice_connected = false;
    let mut bob_connected = false;

    while !(alice_connected && bob_connected) {
        libp2p::futures::select! {
            alice_event = alice.next_event().fuse() => {
                match alice_event {
                    SwarmEvent::ConnectionEstablished { .. } => {
                        log::info!("alice connected");
                        alice_connected = true;
                    }
                    SwarmEvent::Behaviour(event) => {
                        panic!(
                            "alice unexpectedly emitted a behaviour event during connection: {:?}",
                            event
                        );
                    }
                    _ => {}
                }
            }

            bob_event = bob.next_event().fuse() => {
                match bob_event {
                    SwarmEvent::ConnectionEstablished { .. } => {
                        log::info!("bob connected");
                        bob_connected = true;
                    }
                    SwarmEvent::NewListenAddr(addr) => {
                        Swarm::dial_addr(alice, addr).unwrap();
                    }
                    SwarmEvent::Behaviour(event) => {
                        panic!(
                            "bob unexpectedly emitted a behaviour event during connection: {:?}",
                            event
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}
