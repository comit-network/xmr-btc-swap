use crate::network::transport;
use crate::protocol::{alice, bob};
use crate::seed::Seed;
use crate::tor;
use anyhow::Result;
use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
use libp2p::{PeerId, Swarm};

pub fn alice(seed: &Seed) -> Result<Swarm<alice::Behaviour>> {
    with_clear_net(seed, alice::Behaviour::default())
}

pub async fn bob(
    seed: &Seed,
    alice: PeerId,
    tor_socks5_port: u16,
) -> Result<Swarm<bob::Behaviour>> {
    let client = tor::Client::new(tor_socks5_port);
    if client.assert_tor_running().await.is_ok() {
        return with_tor(seed, bob::Behaviour::new(alice), tor_socks5_port).await;
    }
    with_clear_net(seed, bob::Behaviour::new(alice))
}

fn with_clear_net<B>(seed: &Seed, behaviour: B) -> Result<Swarm<B>>
where
    B: NetworkBehaviour,
{
    tracing::info!("All connections will go through clear net.");
    let identity = seed.derive_libp2p_identity();
    let transport = transport::build_clear_net(&identity)?;
    let peer_id = identity.public().into_peer_id();
    tracing::debug!("Our peer-id: {}", peer_id);

    let swarm = SwarmBuilder::new(transport, behaviour, peer_id)
        .executor(Box::new(|f| {
            tokio::spawn(f);
        }))
        .build();

    Ok(swarm)
}

async fn with_tor<B>(seed: &Seed, behaviour: B, tor_socks5_port: u16) -> Result<Swarm<B>>
where
    B: NetworkBehaviour,
{
    tracing::info!("All connections will go through Tor socks5 proxy.");
    let identity = seed.derive_libp2p_identity();
    let transport = transport::build_tor(&identity, tor_socks5_port)?;
    let peer_id = identity.public().into_peer_id();
    tracing::debug!("Our peer-id: {}", peer_id);

    let swarm = SwarmBuilder::new(transport, behaviour, peer_id)
        .executor(Box::new(|f| {
            tokio::spawn(f);
        }))
        .build();

    Ok(swarm)
}
