use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::upgrade::Version;
use libp2p::ping::{Ping, PingEvent, PingSuccess};
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::{identity, noise, yamux, Swarm, Transport, Multiaddr};
use libp2p_tor::duplex;
use libp2p_tor::torut_ext::AuthenticatedConnectionExt;
use noise::NoiseConfig;
use rand::Rng;
use std::time::Duration;
use torut::control::AuthenticatedConn;
use torut::onion::TorSecretKeyV3;
use std::str::FromStr;

#[tokio::main]
async fn main() {

    let key = random_onion_identity();
    let onion_address = key.public().get_onion_address().get_address_without_dot_onion();
    let onion_port = 7654;

    let mut swarm = new_swarm(key).await;
    let peer_id = *swarm.local_peer_id();

    println!("Peer-ID: {}", peer_id);
    // TODO: Figure out what to with the port, we could also set it to 0 and then imply it from the assigned port
    swarm.listen_on(Multiaddr::from_str(format!("/onion3/{}:{}", onion_address, onion_port).as_str()).unwrap()).unwrap();

    loop {
        match swarm.next_event().await {
            SwarmEvent::NewListenAddr(addr) => {
                println!("Listening on {}", addr);
                println!("Connection string: {}/p2p/{}", addr, peer_id)
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
            event => {
                println!("Swarm event: {:?}", event)
            }
        }
    }
}

/// Builds a new swarm that is capable of listening and dialling on the Tor
/// network.
///
/// In particular, this swarm can create ephemeral hidden services on the
/// configured Tor node.
async fn new_swarm(key: TorSecretKeyV3) -> Swarm<Ping> {
    let identity = identity::Keypair::generate_ed25519();

    SwarmBuilder::new(
        duplex::TorConfig::new(
            AuthenticatedConn::new(9051).await.unwrap(),
            key,
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
