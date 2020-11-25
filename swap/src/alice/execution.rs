use crate::{
    alice::{amounts, OutEvent, Swarm},
    network::request_response::AliceToBob,
    SwapAmounts, PUNISH_TIMELOCK, REFUND_TIMELOCK,
};
use anyhow::{bail, Result};
use libp2p::request_response::ResponseChannel;
use std::sync::Arc;
use xmr_btc::{
    alice,
    alice::{State0, State3},
    cross_curve_dleq,
    monero::Transfer,
};

// TODO(Franck): Make all methods here idempotent using db

pub async fn negotiate(
    amounts: SwapAmounts,
    a: crate::bitcoin::SecretKey,
    s_a: cross_curve_dleq::Scalar,
    v_a: crate::monero::PrivateViewKey,
    swarm: &mut Swarm,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
) -> Result<(ResponseChannel<AliceToBob>, SwapAmounts, State3)> {
    // Bob dials us
    match swarm.next().await {
        OutEvent::ConnectionEstablished(_bob_peer_id) => {}
        other => bail!("Unexpected event received: {:?}", other),
    };

    // Bob sends us a request
    let (btc, channel) = match swarm.next().await {
        OutEvent::Request(amounts::OutEvent::Btc { btc, channel }) => (btc, channel),
        other => bail!("Unexpected event received: {:?}", other),
    };

    if btc != amounts.btc {
        bail!(
            "Bob proposed a different amount; got {}, expected: {}",
            btc,
            amounts.btc
        );
    }
    swarm.send_amounts(channel, amounts);

    let SwapAmounts { btc, xmr } = amounts;

    let redeem_address = bitcoin_wallet.as_ref().new_address().await?;
    let punish_address = redeem_address.clone();

    let state0 = State0::new(
        a,
        s_a,
        v_a,
        btc,
        xmr,
        REFUND_TIMELOCK,
        PUNISH_TIMELOCK,
        redeem_address,
        punish_address,
    );

    // Bob sends us message0
    let message0 = match swarm.next().await {
        OutEvent::Message0(msg) => msg,
        other => bail!("Unexpected event received: {:?}", other),
    };

    let state1 = state0.receive(message0)?;

    let (state2, channel) = match swarm.next().await {
        OutEvent::Message1 { msg, channel } => {
            let state2 = state1.receive(msg);
            (state2, channel)
        }
        other => bail!("Unexpected event: {:?}", other),
    };

    let message1 = state2.next_message();
    swarm.send_message1(channel, message1);

    let (state3, channel) = match swarm.next().await {
        OutEvent::Message2 { msg, channel } => {
            let state3 = state2.receive(msg)?;
            (state3, channel)
        }
        other => bail!("Unexpected event: {:?}", other),
    };

    Ok((channel, amounts, state3))
}

pub async fn lock_xmr(
    channel: ResponseChannel<AliceToBob>,
    amounts: SwapAmounts,
    state3: State3,
    swarm: &mut Swarm,
    monero_wallet: Arc<crate::monero::Wallet>,
) -> Result<()> {
    let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey {
        scalar: state3.s_a.into_ed25519(),
    });

    let public_spend_key = S_a + state3.S_b_monero;
    let public_view_key = state3.v.public();

    let (transfer_proof, _) = monero_wallet
        .transfer(public_spend_key, public_view_key, amounts.xmr)
        .await?;

    swarm.send_message2(channel, alice::Message2 {
        tx_lock_proof: transfer_proof,
    });

    // TODO(Franck): Wait for Monero to be mined/finalised

    Ok(())
}
