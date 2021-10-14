use crate::asb::LatestRate;
use crate::libp2p_ext::MultiAddrExt;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::seed::Seed;
use crate::{asb, bitcoin, cli, env, tor};
use anyhow::{Context, Result};
use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
use libp2p::{identity, Multiaddr, Swarm};
use std::fmt::Debug;

#[allow(clippy::too_many_arguments)]
pub fn asb<LR>(
    seed: &Seed,
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    latest_rate: LR,
    resume_only: bool,
    env_config: env::Config,
    rendezvous_params: Option<(Multiaddr, XmrBtcNamespace)>,
) -> Result<Swarm<asb::Behaviour<LR>>>
where
    LR: LatestRate + Send + 'static + Debug + Clone,
{
    let identity = seed.derive_libp2p_identity();

    let rendezvous_params = if let Some((address, namespace)) = rendezvous_params {
        let peer_id = address
            .extract_peer_id()
            .context("Rendezvous node address must contain peer ID")?;

        Some((identity.clone(), peer_id, address, namespace))
    } else {
        None
    };

    let behaviour = asb::Behaviour::new(
        min_buy,
        max_buy,
        latest_rate,
        resume_only,
        env_config,
        rendezvous_params,
    );

    let transport = asb::transport::new(&identity)?;
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
