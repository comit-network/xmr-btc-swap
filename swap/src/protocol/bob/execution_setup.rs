use crate::bitcoin::Signature;
use crate::network::request_response::BUF_SIZE;
use crate::protocol::alice::{Message1, Message3};
use crate::protocol::bob::{State0, State2};
use anyhow::{Context, Error, Result};
use libp2p::PeerId;
use libp2p_async_await::BehaviourOutEvent;
use serde::{Deserialize, Serialize};
use sigma_fun::ext::dl_secp256k1_ed25519_eq::CrossCurveDLEQProof;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message0 {
    pub(crate) B: crate::bitcoin::PublicKey,
    pub(crate) S_b_monero: monero::PublicKey,
    pub(crate) S_b_bitcoin: crate::bitcoin::PublicKey,
    pub(crate) dleq_proof_s_b: CrossCurveDLEQProof,
    pub(crate) v_b: crate::monero::PrivateViewKey,
    pub(crate) refund_address: bitcoin::Address,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message2 {
    pub(crate) tx_lock: crate::bitcoin::TxLock,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message4 {
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
            inner: libp2p_async_await::Behaviour::new(b"/comit/xmr/btc/execution_setup/1.0.0"),
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
                tracing::debug!("Starting execution setup with {}", alice);

                substream
                    .write_message(
                        &serde_cbor::to_vec(&state0.next_message())
                            .context("Failed to serialize message0")?,
                    )
                    .await?;

                let message1 =
                    serde_cbor::from_slice::<Message1>(&substream.read_message(BUF_SIZE).await?)
                        .context("Failed to deserialize message1")?;
                let state1 = state0.receive(bitcoin_wallet.as_ref(), message1).await?;

                substream
                    .write_message(
                        &serde_cbor::to_vec(&state1.next_message())
                            .context("Failed to serialize message2")?,
                    )
                    .await?;

                let message3 =
                    serde_cbor::from_slice::<Message3>(&substream.read_message(BUF_SIZE).await?)
                        .context("Failed to deserialize message3")?;
                let state2 = state1.receive(message3)?;

                substream
                    .write_message(
                        &serde_cbor::to_vec(&state2.next_message())
                            .context("Failed to serialize message4")?,
                    )
                    .await?;

                Ok(state2)
            })
    }
}
