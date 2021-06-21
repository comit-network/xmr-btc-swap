use crate::protocol::alice::event_loop::LatestRate;
use crate::protocol::{alice, bob};
use crate::seed::Seed;
use crate::{asb, cli, env, monero, tor};
use anyhow::Result;
use libp2p::swarm::SwarmBuilder;
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
    let behaviour = alice::Behaviour::new(
        balance,
        lock_fee,
        min_buy,
        max_buy,
        latest_rate,
        resume_only,
        env_config,
    );

    let identity = seed.derive_libp2p_identity();
    let transport = asb::transport::new(&identity)?;
    let peer_id = identity.public().into_peer_id();

    let swarm = SwarmBuilder::new(transport, behaviour, peer_id)
        .executor(Box::new(|f| {
            tokio::spawn(f);
        }))
        .build();

    Ok(swarm)
}

pub async fn cli(
    seed: &Seed,
    alice: PeerId,
    tor_socks5_port: u16,
) -> Result<Swarm<bob::Behaviour>> {
    let maybe_tor_socks5_port = match tor::Client::new(tor_socks5_port).assert_tor_running().await {
        Ok(()) => Some(tor_socks5_port),
        Err(_) => None,
    };

    let behaviour = bob::Behaviour::new(alice);

    let identity = seed.derive_libp2p_identity();
    let transport = cli::transport::new(&identity, maybe_tor_socks5_port)?;
    let peer_id = identity.public().into_peer_id();

    let swarm = SwarmBuilder::new(transport, behaviour, peer_id)
        .executor(Box::new(|f| {
            tokio::spawn(f);
        }))
        .build();

    Ok(swarm)
}
