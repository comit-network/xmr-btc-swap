use crate::{
    bitcoin::Signature,
    network::request_response::BUF_SIZE,
    protocol::{
        alice,
        bob::{State0, State2},
    },
};
use anyhow::{Context, Error, Result};
use libp2p::PeerId;
use libp2p_async_await::BehaviourOutEvent;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message0 {
    pub(crate) B: crate::bitcoin::PublicKey,
    pub(crate) S_b_monero: monero::PublicKey,
    pub(crate) S_b_bitcoin: crate::bitcoin::PublicKey,
    pub(crate) dleq_proof_s_b: cross_curve_dleq::Proof,
    pub(crate) v_b: crate::monero::PrivateViewKey,
    pub(crate) refund_address: bitcoin::Address,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message1 {
    pub(crate) tx_lock: crate::bitcoin::TxLock,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message2 {
    pub(crate) tx_punish_sig: Signature,
    pub(crate) tx_cancel_sig: Signature,
}

#[derive(Debug)]
pub enum OutEvent {
    Done(Result<State2>),
}

impl From<BehaviourOutEvent<(), State2, anyhow::Error>> for OutEvent {
    fn from(event: BehaviourOutEvent<(), State2, Error>) -> Self {
        match event {
            BehaviourOutEvent::Outbound(_, Ok(State2)) => OutEvent::Done(Ok(State2)),
            BehaviourOutEvent::Outbound(_, Err(e)) => OutEvent::Done(Err(e)),
            BehaviourOutEvent::Inbound(..) => unreachable!("Bob only supports outbound"),
        }
    }
}

#[derive(libp2p::NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
pub struct Behaviour {
    inner: libp2p_async_await::Behaviour<(), State2, anyhow::Error>,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            inner: libp2p_async_await::Behaviour::new(b"/execution_setup/1.0.0"),
        }
    }
}

impl Behaviour {
    pub fn run(
        &mut self,
        alice: PeerId,
        state0: State0,
        bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    ) {
        self.inner
            .do_protocol_dialer(alice, move |mut substream| async move {
                let bob_message0 = state0.next_message();

                substream
                    .write_message(
                        &serde_cbor::to_vec(&bob_message0)
                            .context("failed to serialize message0")?,
                    )
                    .await?;

                let alice_message0 = serde_cbor::from_slice::<alice::Message0>(
                    &substream.read_message(BUF_SIZE).await?,
                )
                .context("failed to deserialize message0")?;

                let state1 = state0
                    .receive(bitcoin_wallet.as_ref(), alice_message0)
                    .await?;
                {
                    let bob_message1 = state1.next_message();
                    substream
                        .write_message(
                            &serde_cbor::to_vec(&bob_message1)
                                .context("failed to serialize Message1")?,
                        )
                        .await?;
                }

                let alice_message1 = serde_cbor::from_slice::<alice::Message1>(
                    &substream.read_message(BUF_SIZE).await?,
                )
                .context("failed to deserialize message1")?;
                let state2 = state1.receive(alice_message1)?;

                {
                    let bob_message2 = state2.next_message();
                    substream
                        .write_message(
                            &serde_cbor::to_vec(&bob_message2)
                                .context("failed to serialize Message2")?,
                        )
                        .await?;
                }

                Ok(state2)
            })
    }
}
