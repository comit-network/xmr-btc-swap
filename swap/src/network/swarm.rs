use crate::network::transport;
use crate::protocol::alice::event_loop::LatestRate;
use crate::protocol::{alice, bob};
use crate::seed::Seed;
use crate::{env, monero, tor};
use anyhow::Result;
use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
use libp2p::{PeerId, Swarm};
use std::fmt::Debug;

#[allow(clippy::too_many_arguments)]
pub fn asb<LR>(
    seed: &Seed,
    balance: monero::Amount,
    lock_fee: monero::Amount,
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    latest_rate: LR,
    resume_only: bool,
    env_config: env::Config,
) -> Result<Swarm<alice::Behaviour<LR>>>
where
    LR: LatestRate + Send + 'static + Debug,
{
    with_clear_net(
        seed,
        alice::Behaviour::new(
            balance,
            lock_fee,
            min_buy,
            max_buy,
            latest_rate,
            resume_only,
            env_config,
        ),
    )
}

pub async fn cli(
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
    tracing::info!("All connections will go through clear net");
    let identity = seed.derive_libp2p_identity();
    let transport = transport::build_clear_net(&identity)?;
    let peer_id = identity.public().into_peer_id();
    tracing::debug!(%peer_id, "Our peer-id");

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
    tracing::info!("All connections will go through Tor socks5 proxy");
    let identity = seed.derive_libp2p_identity();
    let transport = transport::build_tor(&identity, tor_socks5_port)?;
    let peer_id = identity.public().into_peer_id();
    tracing::debug!(%peer_id, "Our peer-id");

    let swarm = SwarmBuilder::new(transport, behaviour, peer_id)
        .executor(Box::new(|f| {
            tokio::spawn(f);
        }))
        .build();

    Ok(swarm)
}
