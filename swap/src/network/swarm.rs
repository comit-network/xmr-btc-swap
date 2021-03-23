use crate::network::transport;
use crate::seed::Seed;
use anyhow::Result;
use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
use libp2p::Swarm;

pub fn new<B>(seed: &Seed) -> Result<Swarm<B>>
where
    B: NetworkBehaviour + Default,
{
    let identity = seed.derive_libp2p_identity();

    let behaviour = B::default();
    let transport = transport::build(&identity)?;

    let swarm = SwarmBuilder::new(transport, behaviour, identity.public().into_peer_id())
        .executor(Box::new(|f| {
            tokio::spawn(f);
        }))
        .build();

    Ok(swarm)
}
