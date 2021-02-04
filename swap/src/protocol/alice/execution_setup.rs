use crate::{
    bitcoin,
    bitcoin::{EncryptedSignature, Signature},
    monero,
    network::request_response::BUF_SIZE,
    protocol::{
        alice::{State0, State3},
        bob,
        bob::Message4,
    },
};
use anyhow::{Context, Error, Result};
use libp2p::PeerId;
use libp2p_async_await::BehaviourOutEvent;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message0 {
    pub(crate) A: bitcoin::PublicKey,
    pub(crate) S_a_monero: monero::PublicKey,
    pub(crate) S_a_bitcoin: bitcoin::PublicKey,
    pub(crate) dleq_proof_s_a: cross_curve_dleq::Proof,
    pub(crate) v_a: monero::PrivateViewKey,
    pub(crate) redeem_address: bitcoin::Address,
    pub(crate) punish_address: bitcoin::Address,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message1 {
    pub(crate) tx_cancel_sig: Signature,
    pub(crate) tx_refund_encsig: EncryptedSignature,
}

#[derive(Debug)]
pub enum OutEvent {
    Done(Result<State3>),
}

impl From<BehaviourOutEvent<State3, (), anyhow::Error>> for OutEvent {
    fn from(event: BehaviourOutEvent<State3, (), Error>) -> Self {
        match event {
            BehaviourOutEvent::Inbound(_, Ok(State3)) => OutEvent::Done(Ok(State3)),
            BehaviourOutEvent::Inbound(_, Err(e)) => OutEvent::Done(Err(e)),
            BehaviourOutEvent::Outbound(..) => unreachable!("Alice only supports inbound"),
        }
    }
}

#[derive(libp2p::NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
pub struct Behaviour {
    inner: libp2p_async_await::Behaviour<State3, (), anyhow::Error>,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            inner: libp2p_async_await::Behaviour::new(b"/execution_setup/1.0.0"),
        }
    }
}

impl Behaviour {
    pub fn run(&mut self, bob: PeerId, state0: State0) {
        self.inner
            .do_protocol_listener(bob, move |mut substream| async move {
                let alice_message0 = state0.next_message();

                let state1 = {
                    let bob_message0 = serde_cbor::from_slice::<bob::Message0>(
                        &substream.read_message(BUF_SIZE).await?,
                    )
                    .context("failed to deserialize message0")?;
                    state0.receive(bob_message0)?
                };

                substream
                    .write_message(
                        &serde_cbor::to_vec(&alice_message0)
                            .context("failed to serialize Message0")?,
                    )
                    .await?;

                let state2 = {
                    let bob_message1 = serde_cbor::from_slice::<bob::Message1>(
                        &substream.read_message(BUF_SIZE).await?,
                    )
                    .context("failed to deserialize message1")?;
                    state1.receive(bob_message1)
                };

                {
                    let alice_message2 = state2.next_message();
                    substream
                        .write_message(
                            &serde_cbor::to_vec(&alice_message2)
                                .context("failed to serialize Message2")?,
                        )
                        .await?;
                }

                let state3 = {
                    let message4 = serde_cbor::from_slice::<Message4>(
                        &substream.read_message(BUF_SIZE).await?,
                    )
                    .context("failed to deserialize message4")?;
                    state2.receive(message4)?
                };

                Ok(state3)
            })
    }
}
