use crate::network::cbor_request_response::BUF_SIZE;
use crate::protocol::alice::{State0, State3};
use crate::protocol::{Message0, Message2, Message4};
use anyhow::{Context, Error};
use libp2p::PeerId;
use libp2p_async_await::BehaviourOutEvent;

#[derive(Debug)]
pub enum OutEvent {
    Done { bob_peer_id: PeerId, state3: State3 },
    Failure { peer: PeerId, error: Error },
}

impl From<BehaviourOutEvent<(PeerId, State3), (), Error>> for OutEvent {
    fn from(event: BehaviourOutEvent<(PeerId, State3), (), Error>) -> Self {
        match event {
            BehaviourOutEvent::Inbound(_, Ok((bob_peer_id, state3))) => OutEvent::Done {
                bob_peer_id,
                state3,
            },
            BehaviourOutEvent::Inbound(peer, Err(e)) => OutEvent::Failure { peer, error: e },
            BehaviourOutEvent::Outbound(..) => unreachable!("Alice only supports inbound"),
        }
    }
}

#[derive(libp2p::NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
pub struct Behaviour {
    inner: libp2p_async_await::Behaviour<(PeerId, State3), (), anyhow::Error>,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            inner: libp2p_async_await::Behaviour::new(b"/comit/xmr/btc/execution_setup/1.0.0"),
        }
    }
}

impl Behaviour {
    pub fn run(&mut self, bob: PeerId, state0: State0) {
        self.inner
            .do_protocol_listener(bob, move |mut substream| async move {
                let message0 =
                    serde_cbor::from_slice::<Message0>(&substream.read_message(BUF_SIZE).await?)
                        .context("Failed to deserialize message0")?;
                let state1 = state0.receive(message0)?;

                substream
                    .write_message(
                        &serde_cbor::to_vec(&state1.next_message())
                            .context("Failed to serialize message1")?,
                    )
                    .await?;

                let message2 =
                    serde_cbor::from_slice::<Message2>(&substream.read_message(BUF_SIZE).await?)
                        .context("Failed to deserialize message2")?;
                let state2 = state1
                    .receive(message2)
                    .context("Failed to receive Message2")?;

                substream
                    .write_message(
                        &serde_cbor::to_vec(&state2.next_message())
                            .context("Failed to serialize message3")?,
                    )
                    .await?;

                let message4 =
                    serde_cbor::from_slice::<Message4>(&substream.read_message(BUF_SIZE).await?)
                        .context("Failed to deserialize message4")?;
                let state3 = state2.receive(message4)?;

                Ok((bob, state3))
            })
    }
}
