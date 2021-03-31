use crate::network::transport;
use crate::protocol::{alice, bob};
use crate::seed::Seed;
use anyhow::Result;
use libp2p::core::network::ConnectionLimits;
use libp2p::core::PublicKey;
use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
use libp2p::Swarm;

pub fn alice(seed: &Seed) -> Result<Swarm<alice::Behaviour>> {
    let connection_limits = ConnectionLimits::default()
        .with_max_established_outgoing(Some(10))
        .with_max_pending_outgoing(Some(5));

    new(seed, connection_limits)
}

pub fn bob(seed: &Seed) -> Result<Swarm<bob::Behaviour>> {
    new(seed, ConnectionLimits::default())
}

fn new<B>(seed: &Seed, connection_limits: ConnectionLimits) -> Result<Swarm<B>>
where
    B: NetworkBehaviour + From<PublicKey>,
{
    let identity = seed.derive_libp2p_identity();
    let public_key = identity.public();
    let local_peer_id = public_key.clone().into_peer_id();

    let behaviour = B::from(public_key);
    let transport = transport::build(&identity)?;

    let swarm = SwarmBuilder::new(transport, behaviour, local_peer_id)
        .executor(Box::new(|f| {
            tokio::spawn(f);
        }))
        .connection_limits(connection_limits)
        .build();

    Ok(swarm)
}
