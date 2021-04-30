use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::upgrade::Version;
use libp2p::ping::{Ping, PingEvent, PingSuccess};
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::{identity, noise, yamux, Multiaddr, Swarm, Transport};
use libp2p_tor::dial_only;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let addr_to_dial = std::env::args()
        .next()
        .unwrap()
        .parse::<Multiaddr>()
        .unwrap();

    let mut swarm = new_swarm();

    println!("Peer-ID: {}", swarm.local_peer_id());
    swarm.dial_addr(addr_to_dial).unwrap();

    loop {
        match swarm.next_event().await {
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

/// Builds a new swarm that is capable of dialling onion address.
fn new_swarm() -> Swarm<Ping> {
    let identity = identity::Keypair::generate_ed25519();

    SwarmBuilder::new(
        dial_only::TorConfig::new(9050)
            .upgrade(Version::V1)
            .authenticate(
                noise::NoiseConfig::xx(
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
