use crate::seed::Seed;
use anyhow::Result;
use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
use libp2p::{Swarm, Transport, yamux};
use libp2p::relay::{RelayConfig, new_transport_and_behaviour, Relay};
use libp2p::tcp::TokioTcpConfig;
use libp2p::dns::TokioDnsConfig;
use libp2p::core::upgrade::{Version, SelectUpgrade};
use libp2p::mplex::MplexConfig;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::noise::{self, NoiseConfig, X25519Spec};

pub fn new<B>(seed: &Seed) -> Result<Swarm<B>>
where
    B: NetworkBehaviour + From<Relay>,
{
    let identity = seed.derive_libp2p_identity();

    let dh_keys = noise::Keypair::<X25519Spec>::new().into_authentic(&identity)?;
    let noise = NoiseConfig::xx(dh_keys).into_authenticated();

    let tcp = TokioTcpConfig::new().nodelay(true);
    let dns = TokioDnsConfig::system(tcp)?;

    let (relay_transport, relay_behaviour) = new_transport_and_behaviour(RelayConfig::default(), dns);

    let transport = relay_transport
        .upgrade(Version::V1)
        .authenticate(noise)
        .multiplex(SelectUpgrade::new(
            yamux::YamuxConfig::default(),
            MplexConfig::new(),
        ))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    let behaviour = B::from(relay_behaviour);
    let swarm = SwarmBuilder::new(transport, behaviour, identity.public().into_peer_id())
        .executor(Box::new(|f| {
            tokio::spawn(f);
        }))
        .build();

    Ok(swarm)
}
