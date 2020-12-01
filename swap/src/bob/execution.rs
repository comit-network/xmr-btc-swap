use crate::{
    bob::{OutEvent, Swarm},
    Cmd, Rsp, SwapAmounts,
};
use anyhow::Result;
use libp2p::core::Multiaddr;
use rand::{CryptoRng, RngCore};
use std::sync::Arc;
use tokio::{stream::StreamExt, sync::mpsc};
use xmr_btc::bob::State2;

pub async fn negotiate<R>(
    state0: xmr_btc::bob::State0,
    amounts: SwapAmounts,
    swarm: &mut Swarm,
    addr: Multiaddr,
    mut rng: R,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
) -> Result<(SwapAmounts, State2)>
where
    R: RngCore + CryptoRng + Send,
{
    libp2p::Swarm::dial_addr(swarm, addr)?;

    let alice = match swarm.next().await {
        OutEvent::ConnectionEstablished(alice) => alice,
        other => panic!("unexpected event: {:?}", other),
    };

    swarm.request_amounts(alice.clone(), amounts.btc.as_sat());

    // todo: see if we can remove
    let (btc, xmr) = match swarm.next().await {
        OutEvent::Amounts(amounts) => (amounts.btc, amounts.xmr),
        other => panic!("unexpected event: {:?}", other),
    };

    swarm.send_message0(alice.clone(), state0.next_message(&mut rng));
    let state1 = match swarm.next().await {
        OutEvent::Message0(msg) => state0.receive(bitcoin_wallet.as_ref(), msg).await?,
        other => panic!("unexpected event: {:?}", other),
    };

    swarm.send_message1(alice.clone(), state1.next_message());
    let state2 = match swarm.next().await {
        OutEvent::Message1(msg) => state1.receive(msg)?,
        other => panic!("unexpected event: {:?}", other),
    };

    swarm.send_message2(alice.clone(), state2.next_message());

    Ok((amounts, state2))
}
