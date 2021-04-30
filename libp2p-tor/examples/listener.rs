use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::upgrade::Version;
use libp2p::ping::{Ping, PingEvent, PingSuccess};
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::{identity, noise, yamux, Swarm, Transport};
use libp2p_tor::duplex;
use libp2p_tor::torut_ext::AuthenticatedConnectionExt;
use noise::NoiseConfig;
use rand::Rng;
use std::time::Duration;
use torut::control::AuthenticatedConn;
use torut::onion::TorSecretKeyV3;

#[tokio::main]
async fn main() {
    let wildcard_multiaddr =
        "/onion3/WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW:8080"
            .parse()
            .unwrap();

    let mut swarm = new_swarm().await;

    println!("Peer-ID: {}", swarm.local_peer_id());
    swarm.listen_on(wildcard_multiaddr).unwrap();

    loop {
        match swarm.next_event().await {
            SwarmEvent::NewListenAddr(addr) => {
                println!("Listening on {}", addr);
            }
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                println!(
                    "Connected to {} via {}",
                    peer_id,
                    endpoint.get_remote_address()
                );
            }
            SwarmEvent::Behaviour(PingEvent { result, peer }) => match result {
                Ok(PingSuccess::Pong) => {
                    println!("Got pong from {}", peer);
                }
                Ok(PingSuccess::Ping { rtt }) => {
                    println!("Pinged {} with rtt of {}s", peer, rtt.as_secs());
                }
                Err(failure) => {
                    println!("Failed to ping {}: {}", peer, failure)
                }
            },
            _ => {}
        }
    }
}

/// Builds a new swarm that is capable of listening and dialling on the Tor
/// network.
///
/// In particular, this swarm can create ephemeral hidden services on the
/// configured Tor node.
async fn new_swarm() -> Swarm<Ping> {
    let identity = identity::Keypair::generate_ed25519();

    SwarmBuilder::new(
        duplex::TorConfig::new(
            AuthenticatedConn::new(9051).await.unwrap(),
            random_onion_identity,
        )
        .await
        .unwrap()
        .upgrade(Version::V1)
        .authenticate(
            NoiseConfig::xx(
                noise::Keypair::<noise::X25519Spec>::new()
                    .into_authentic(&identity)
                    .unwrap(),
            )
            .into_authenticated(),
        )
        .multiplex(yamux::YamuxConfig::default())
        .timeout(Duration::from_secs(20))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed(),
        Ping::default(),
        identity.public().into_peer_id(),
    )
    .executor(Box::new(|f| {
        tokio::spawn(f);
    }))
    .build()
}

fn random_onion_identity() -> TorSecretKeyV3 {
    let mut onion_key_bytes = [0u8; 64];
    rand::thread_rng().fill(&mut onion_key_bytes);

    onion_key_bytes.into()
}
