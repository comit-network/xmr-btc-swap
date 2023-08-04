use crate::asb::{LatestRate, RendezvousNode};
use crate::libp2p_ext::MultiAddrExt;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::seed::Seed;
use crate::{asb, bitcoin, cli, env, tor};
use anyhow::Result;
use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
use libp2p::{identity, Multiaddr, Swarm};
use std::fmt::Debug;

#[allow(clippy::too_many_arguments)]
pub async fn asb<LR>(
    seed: &Seed,
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    latest_rate: LR,
    resume_only: bool,
    env_config: env::Config,
    namespace: XmrBtcNamespace,
    rendezvous_addrs: &[Multiaddr],
    tor_socks5_port: u16
) -> Result<Swarm<asb::Behaviour<LR>>>
where
    LR: LatestRate + Send + 'static + Debug + Clone,
{
    let maybe_tor_socks5_port = match tor::Client::new(tor_socks5_port).assert_tor_running().await {
        Ok(()) => Some(tor_socks5_port),
        Err(_) => None,
    };

    let identity = seed.derive_libp2p_identity();

    let rendezvous_nodes = rendezvous_addrs
        .iter()
        .map(|addr| {
            let peer_id = addr
                .extract_peer_id()
                .expect("Rendezvous node address must contain peer ID");

            RendezvousNode::new(addr, peer_id, namespace, None)
        })
        .collect();

    let behaviour = asb::Behaviour::new(
        min_buy,
        max_buy,
        latest_rate,
        resume_only,
        env_config,
        (identity.clone(), namespace),
        rendezvous_nodes,
    );

    let transport = asb::transport::new(&identity, maybe_tor_socks5_port)?;
    let peer_id = identity.public().into();

    let swarm = SwarmBuilder::new(transport, behaviour, peer_id)
        .executor(Box::new(|f| {
            tokio::spawn(f);
        }))
        .build();

    Ok(swarm)
}

pub async fn cli<T>(
    identity: identity::Keypair,
    tor_socks5_port: u16,
    behaviour: T,
) -> Result<Swarm<T>>
where
    T: NetworkBehaviour,
{
    let maybe_tor_socks5_port = match tor::Client::new(tor_socks5_port).assert_tor_running().await {
        Ok(()) => Some(tor_socks5_port),
        Err(_) => None,
    };

    let transport = cli::transport::new(&identity, maybe_tor_socks5_port)?;
    let peer_id = identity.public().into();

    let swarm = SwarmBuilder::new(transport, behaviour, peer_id)
        .executor(Box::new(|f| {
            tokio::spawn(f);
        }))
        .build();

    Ok(swarm)
}
