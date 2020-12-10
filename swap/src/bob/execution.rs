use crate::{bob::event_loop::EventLoopHandle, SwapAmounts};
use anyhow::Result;
use libp2p::{core::Multiaddr, PeerId};
use rand::{CryptoRng, RngCore};
use std::sync::Arc;
use xmr_btc::bob::State2;

pub async fn negotiate<R>(
    state0: xmr_btc::bob::State0,
    amounts: SwapAmounts,
    swarm: &mut EventLoopHandle,
    addr: Multiaddr,
    mut rng: R,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
) -> Result<(State2, PeerId)>
where
    R: RngCore + CryptoRng + Send,
{
    tracing::trace!("Starting negotiate");
    swarm.dial_alice(addr).await?;

    let alice_peer_id = swarm.recv_conn_established().await?;

    swarm
        .request_amounts(alice_peer_id.clone(), amounts.btc)
        .await?;

    swarm
        .send_message0(alice_peer_id.clone(), state0.next_message(&mut rng))
        .await?;
    let msg0 = swarm.recv_message0().await?;
    let state1 = state0.receive(bitcoin_wallet.as_ref(), msg0).await?;

    swarm
        .send_message1(alice_peer_id.clone(), state1.next_message())
        .await?;
    let msg1 = swarm.recv_message1().await?;
    let state2 = state1.receive(msg1)?;

    swarm
        .send_message2(alice_peer_id.clone(), state2.next_message())
        .await?;

    Ok((state2, alice_peer_id))
}
