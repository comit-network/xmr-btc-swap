use async_trait::async_trait;
use futures::StreamExt;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::upgrade::Version;
use libp2p::core::transport::MemoryTransport;
use libp2p::core::{Multiaddr, Transport};
use libp2p::identity;
use libp2p::noise;
use libp2p::swarm::dial_opts::DialOpts;
use libp2p::swarm::{NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::tcp;
use libp2p::yamux;
use libp2p::SwarmBuilder;
use std::fmt::Debug;
use std::time::Duration;

pub fn new_swarm<B, F>(behaviour_fn: F) -> Swarm<B>
where
    B: NetworkBehaviour,
    <B as NetworkBehaviour>::ToSwarm: Debug,
    B: NetworkBehaviour,
    F: FnOnce(identity::Keypair) -> B,
{
    let identity = identity::Keypair::generate_ed25519();
    let noise = noise::Config::new(&identity).unwrap();
    let tcp = tcp::tokio::Transport::new(tcp::Config::new());

    let transport = MemoryTransport::new()
        .or_transport(tcp)
        .upgrade(Version::V1)
        .authenticate(noise)
        .multiplex(yamux::Config::default())
        .timeout(Duration::from_secs(5))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    SwarmBuilder::with_existing_identity(identity)
        .with_tokio()
        .with_other_transport(|_| Ok(transport))
        .unwrap()
        .with_behaviour(|keypair| Ok(behaviour_fn(keypair.clone())))
        .unwrap()
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

/// An extension trait for [`Swarm`] that makes it easier to set up a network of
/// [`Swarm`]s for tests.
#[async_trait]
pub trait SwarmExt {
    /// Establishes a connection to the given [`Swarm`], polling both of them
    /// until the connection is established.
    async fn block_on_connection<T>(&mut self, other: &mut Swarm<T>)
    where
        T: NetworkBehaviour + Send,
        <T as NetworkBehaviour>::ToSwarm: Debug;

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
    B: NetworkBehaviour + Send,
    <B as NetworkBehaviour>::ToSwarm: Debug,
{
    async fn block_on_connection<T>(&mut self, other: &mut Swarm<T>)
    where
        T: NetworkBehaviour + Send,
        <T as NetworkBehaviour>::ToSwarm: Debug,
    {
        let addr_to_dial = other.external_addresses().next().unwrap().clone();
        let local_peer_id = *other.local_peer_id();

        self.dial(
            DialOpts::peer_id(local_peer_id)
                .addresses(vec![addr_to_dial])
                .extend_addresses_through_behaviour()
                .build(),
        )
        .unwrap();

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
                        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } if matches!(peer_id, Some(alice_peer_id) if alice_peer_id == local_peer_id) => {
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
        self.add_external_address(multiaddr.clone());

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
    <B as NetworkBehaviour>::ToSwarm: Debug,
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
