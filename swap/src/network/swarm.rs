use crate::network::transport;
use crate::protocol::{alice, bob};
use crate::seed::Seed;
use anyhow::Result;
use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
use libp2p::{PeerId, Swarm};

pub fn alice(seed: &Seed) -> Result<Swarm<alice::Behaviour>> {
    new(seed, alice::Behaviour::default())
}

pub fn bob(seed: &Seed, alice: PeerId) -> Result<Swarm<bob::Behaviour>> {
    new(seed, bob::Behaviour::new(alice))
}

fn new<B>(seed: &Seed, behaviour: B) -> Result<Swarm<B>>
where
    B: NetworkBehaviour,
{
    let identity = seed.derive_libp2p_identity();
    let transport = transport::build(&identity)?;
    let peer_id = identity.public().into_peer_id();
    tracing::debug!("Our peer-id: {}", peer_id);

    let swarm = SwarmBuilder::new(transport, behaviour, peer_id)
        .executor(Box::new(|f| {
            tokio::spawn(f);
        }))
        .build();

    Ok(swarm)
}
