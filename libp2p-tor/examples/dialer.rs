use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::upgrade::{Version, SelectUpgrade};
use libp2p::ping::{Ping, PingEvent, PingSuccess};
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::{identity, noise, yamux, Multiaddr, Swarm, Transport};
use libp2p_tor::dial_only;
use std::time::Duration;
use libp2p::mplex::MplexConfig;
use anyhow::{anyhow, bail, Result};
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() -> Result<()> {

    let _guard = tracing_subscriber::fmt()
        .with_env_filter("debug,libp2p_tor=debug") // add `reqwest::connect::verbose=trace` if you want to logs of the RPC clients
        .with_test_writer()
        .set_default();

    let proxy = reqwest::Proxy::all("socks5h://127.0.0.1:9050")
        .map_err(|_| anyhow!("tor proxy should be there"))?;
    let client = reqwest::Client::builder().proxy(proxy).build()?;

    let res = client.get("https://check.torproject.org").send().await?;
    let text = res.text().await?;

    if !text.contains("Congratulations. This browser is configured to use Tor.") {
        bail!("Tor is currently not running")
    }


    let addr_to_dial = "/onion3/jpclybnowuibjexya3qggzvzkoeruuav4nyjlxpnkrosldsvykfbn6qd:7654/p2p/12D3KooWHKqGyK4hVtf5BQY8GpbY6fSGKDZ8eBXMQ3H2RsdnKVzC"
        .parse::<Multiaddr>()
        .unwrap();

    let mut swarm = new_swarm();

    println!("Peer-ID: {}", swarm.local_peer_id());

    println!("Dialing {}", addr_to_dial);
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
            event => {
                println!("Swarm event: {:?}", event)
            }
        }
    }
}

/// Builds a new swarm that is capable of dialling onion address.
fn new_swarm() -> Swarm<Ping> {
    let identity = identity::Keypair::generate_ed25519();
    let peer_id = identity.public().into_peer_id();

    println!("peer id upon swarm setup: {}", peer_id);

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
            .multiplex(SelectUpgrade::new(
                yamux::YamuxConfig::default(),
                MplexConfig::new(),
            ))
            .timeout(Duration::from_secs(20))
            .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
            .boxed(),
        Ping::default(),
        peer_id,
    )
    .executor(Box::new(|f| {
        tokio::spawn(f);
    }))
    .build()
}
