use crate::{
    bob::{OutEvent, Swarm},
    Cmd, Rsp, SwapAmounts, PUNISH_TIMELOCK, REFUND_TIMELOCK,
};
use anyhow::Result;
use rand::{CryptoRng, RngCore};
use std::sync::Arc;
use tokio::{stream::StreamExt, sync::mpsc};

use xmr_btc::bob::{State0, State2};

pub async fn negotiate<R>(
    state0: xmr_btc::bob::State0,
    amounts: SwapAmounts,
    swarm: &mut Swarm,
    mut rng: R,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
) -> Result<(SwapAmounts, State2)>
where
    R: RngCore + CryptoRng + Send,
{
    let (mut cmd_tx, _cmd_rx) = mpsc::channel(1);
    let (_rsp_tx, mut rsp_rx) = mpsc::channel(1);

    // todo: dial the swarm outside
    // libp2p::Swarm::dial_addr(&mut swarm, addr)?;
    let alice = match swarm.next().await {
        OutEvent::ConnectionEstablished(alice) => alice,
        other => panic!("unexpected event: {:?}", other),
    };

    swarm.request_amounts(alice.clone(), amounts.btc.as_sat());

    // todo: remove/refactor mspc channel
    let (btc, xmr) = match swarm.next().await {
        OutEvent::Amounts(amounts) => {
            let cmd = Cmd::VerifyAmounts(amounts);
            cmd_tx.try_send(cmd)?;
            let response = rsp_rx.next().await;
            if response == Some(Rsp::Abort) {
                panic!("abort response");
            }
            (amounts.btc, amounts.xmr)
        }
        other => panic!("unexpected event: {:?}", other),
    };

    let refund_address = bitcoin_wallet.as_ref().new_address().await?;

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
