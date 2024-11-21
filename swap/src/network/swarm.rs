use crate::asb::{LatestRate, RendezvousNode};
use crate::libp2p_ext::MultiAddrExt;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::seed::Seed;
use crate::{asb, bitcoin, cli, env};
use anyhow::Result;
use arti_client::TorClient;
use libp2p::swarm::NetworkBehaviour;
use libp2p::SwarmBuilder;
use libp2p::{identity, Multiaddr, Swarm};
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tor_rtcompat::tokio::TokioRustlsRuntime;

#[allow(clippy::too_many_arguments)]
pub fn asb<LR>(
    seed: &Seed,
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    latest_rate: LR,
    resume_only: bool,
    env_config: env::Config,
    namespace: XmrBtcNamespace,
    rendezvous_addrs: &[Multiaddr],
) -> Result<Swarm<asb::Behaviour<LR>>>
where
    LR: LatestRate + Send + 'static + Debug + Clone,
{
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

    let transport = asb::transport::new(&identity)?;

    let swarm = SwarmBuilder::with_existing_identity(identity)
        .with_tokio()
        .with_other_transport(|_| transport)?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::MAX))
        .build();

    Ok(swarm)
}

pub async fn cli<T>(
    identity: identity::Keypair,
    maybe_tor_client: Option<Arc<TorClient<TokioRustlsRuntime>>>,
    behaviour: T,
) -> Result<Swarm<T>>
where
    T: NetworkBehaviour,
{
    let transport = cli::transport::new(&identity, maybe_tor_client)?;

    let swarm = SwarmBuilder::with_existing_identity(identity)
        .with_tokio()
        .with_other_transport(|_| transport)?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::MAX))
        .build();

    Ok(swarm)
}
