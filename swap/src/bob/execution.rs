use crate::{bob::swarm_driver::SwarmDriverHandle, SwapAmounts};
use anyhow::Result;
use libp2p::core::Multiaddr;
use rand::{CryptoRng, RngCore};
use std::sync::Arc;
use xmr_btc::bob::State2;

pub async fn negotiate<R>(
    state0: xmr_btc::bob::State0,
    amounts: SwapAmounts,
    swarm: &mut SwarmDriverHandle,
    addr: Multiaddr,
    mut rng: R,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
) -> Result<State2>
where
    R: RngCore + CryptoRng + Send,
{
    tracing::trace!("Starting negotiate");
    swarm.dial_alice(addr).await?;

    let alice = swarm.recv_conn_established().await?;

    swarm.request_amounts(alice.clone(), amounts.btc).await?;

    swarm
        .send_message0(alice.clone(), state0.next_message(&mut rng))
        .await?;
    let msg0 = swarm.recv_message0().await?;
    let state1 = state0.receive(bitcoin_wallet.as_ref(), msg0).await?;

    swarm
        .send_message1(alice.clone(), state1.next_message())
        .await?;
    let msg1 = swarm.recv_message1().await?;
    let state2 = state1.receive(msg1)?;

    swarm
        .send_message2(alice.clone(), state2.next_message())
        .await?;

    Ok(state2)
}
