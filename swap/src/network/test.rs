use async_trait::async_trait;
use futures::stream::FusedStream;
use futures::{future, Future, Stream, StreamExt};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::upgrade::Version;
use libp2p::core::transport::MemoryTransport;
use libp2p::core::upgrade::SelectUpgrade;
use libp2p::core::{identity, Executor, Multiaddr, PeerId, Transport};
use libp2p::mplex::MplexConfig;
use libp2p::noise::{Keypair, NoiseConfig, X25519Spec};
use libp2p::swarm::{AddressScore, NetworkBehaviour, Swarm, SwarmBuilder, SwarmEvent};
use libp2p::tcp::TokioTcpConfig;
use libp2p::yamux::YamuxConfig;
use std::fmt::Debug;
use std::pin::Pin;
use std::time::Duration;

/// An adaptor struct for libp2p that spawns futures into the current
/// thread-local runtime.
struct GlobalSpawnTokioExecutor;

impl Executor for GlobalSpawnTokioExecutor {
    fn exec(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        tokio::spawn(future);
    }
}

pub fn new_swarm<B, F>(behaviour_fn: F) -> Swarm<B>
where
    B: NetworkBehaviour,
    <B as NetworkBehaviour>::OutEvent: Debug,
    B: NetworkBehaviour,
    F: FnOnce(PeerId, identity::Keypair) -> B,
{
    let identity = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(identity.public());

    let dh_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&identity)
        .expect("failed to create dh_keys");
    let noise = NoiseConfig::xx(dh_keys).into_authenticated();

    let transport = MemoryTransport::default()
        .or_transport(TokioTcpConfig::new())
        .upgrade(Version::V1)
        .authenticate(noise)
        .multiplex(SelectUpgrade::new(
            YamuxConfig::default(),
            MplexConfig::new(),
        ))
        .timeout(Duration::from_secs(5))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    SwarmBuilder::new(transport, behaviour_fn(peer_id, identity), peer_id)
        .executor(Box::new(GlobalSpawnTokioExecutor))
        .build()
}

fn get_rand_memory_address() -> Multiaddr {
    let address_port = rand::random::<u64>();

    format!("/memory/{}", address_port).parse().unwrap()
}

async fn get_local_tcp_address() -> Multiaddr {
    let random_port = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    };

    format!("/ip4/127.0.0.1/tcp/{}", random_port)
        .parse()
        .unwrap()
}

pub async fn await_events_or_timeout<A, B, E1, E2>(
    swarm_1: &mut (impl Stream<Item = SwarmEvent<A, E1>> + FusedStream + Unpin),
    swarm_2: &mut (impl Stream<Item = SwarmEvent<B, E2>> + FusedStream + Unpin),
) -> (SwarmEvent<A, E1>, SwarmEvent<B, E2>)
where
    SwarmEvent<A, E1>: Debug,
    SwarmEvent<B, E2>: Debug,
{
    tokio::time::timeout(
        Duration::from_secs(30),
        future::join(
            swarm_1
                .inspect(|event| tracing::debug!("Swarm1 emitted {:?}", event))
                .select_next_some(),
            swarm_2
                .inspect(|event| tracing::debug!("Swarm2 emitted {:?}", event))
                .select_next_some(),
        ),
    )
    .await
    .expect("network behaviours to emit an event within 10 seconds")
}

/// An extension trait for [`Swarm`] that makes it easier to set up a network of
/// [`Swarm`]s for tests.
#[async_trait]
pub trait SwarmExt {
    /// Establishes a connection to the given [`Swarm`], polling both of them
    /// until the connection is established.
    async fn block_on_connection<T>(&mut self, other: &mut Swarm<T>)
    where
        T: NetworkBehaviour,
        <T as NetworkBehaviour>::OutEvent: Debug;

    /// Listens on a random memory address, polling the [`Swarm`] until the
    /// transport is ready to accept connections.
    async fn listen_on_random_memory_address(&mut self) -> Multiaddr;

    /// Listens on a TCP port for localhost only, polling the [`Swarm`] until
    /// the transport is ready to accept connections.
    async fn listen_on_tcp_localhost(&mut self) -> Multiaddr;
}

#[async_trait]
impl<B> SwarmExt for Swarm<B>
where
    B: NetworkBehaviour,
    <B as NetworkBehaviour>::OutEvent: Debug,
{
    async fn block_on_connection<T>(&mut self, other: &mut Swarm<T>)
    where
        T: NetworkBehaviour,
        <T as NetworkBehaviour>::OutEvent: Debug,
    {
        let addr_to_dial = other.external_addresses().next().unwrap().addr.clone();
        let local_peer_id = *other.local_peer_id();

        self.dial(addr_to_dial).unwrap();

        let mut dialer_done = false;
        let mut listener_done = false;

        loop {
            let dialer_event_fut = self.select_next_some();

            tokio::select! {
                dialer_event = dialer_event_fut => {
                    match dialer_event {
                        SwarmEvent::ConnectionEstablished { .. } => {
                            dialer_done = true;
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id, error } if matches!(peer_id, Some(alice_peer_id) if alice_peer_id == local_peer_id) => {
                                panic!("Failed to dial address {}: {}", peer_id.unwrap(), error)
                        }
                        other => {
                            tracing::debug!("Ignoring {:?}", other);
                        }
                    }
                },
                listener_event = other.select_next_some() => {
                    match listener_event {
                        SwarmEvent::ConnectionEstablished { .. } => {
                            listener_done = true;
                        }
                        SwarmEvent::IncomingConnectionError { error, .. } => {
                            panic!("Failure in incoming connection {}", error);
                        }
                        other => {
                            tracing::debug!("Ignoring {:?}", other);
                        }
                    }
                }
            }

            if dialer_done && listener_done {
                return;
            }
        }
    }

    async fn listen_on_random_memory_address(&mut self) -> Multiaddr {
        let multiaddr = get_rand_memory_address();

        self.listen_on(multiaddr.clone()).unwrap();
        block_until_listening_on(self, &multiaddr).await;

        // Memory addresses are externally reachable because they all share the same
        // memory-space.
        self.add_external_address(multiaddr.clone(), AddressScore::Infinite);

        multiaddr
    }

    async fn listen_on_tcp_localhost(&mut self) -> Multiaddr {
        let multiaddr = get_local_tcp_address().await;

        self.listen_on(multiaddr.clone()).unwrap();
        block_until_listening_on(self, &multiaddr).await;

        multiaddr
    }
}

async fn block_until_listening_on<B>(swarm: &mut Swarm<B>, multiaddr: &Multiaddr)
where
    B: NetworkBehaviour,
    <B as NetworkBehaviour>::OutEvent: Debug,
{
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } if &address == multiaddr => {
                break;
            }
            other => {
                tracing::debug!(
                    "Ignoring {:?} while waiting for listening to succeed",
                    other
                );
            }
        }
    }
}
