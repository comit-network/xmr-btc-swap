use crate::network::cbor_request_response::BUF_SIZE;
use crate::protocol::alice::{State0, State3};
use crate::protocol::{alice, Message0, Message2, Message4};
use anyhow::{Context, Error};
use libp2p::PeerId;
use libp2p_async_await::BehaviourOutEvent;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug)]
pub enum OutEvent {
    Done {
        bob_peer_id: PeerId,
        swap_id: Uuid,
        state3: State3,
    },
    Failure {
        peer: PeerId,
        error: Error,
    },
}

impl From<BehaviourOutEvent<(PeerId, (Uuid, State3)), (), Error>> for OutEvent {
    fn from(event: BehaviourOutEvent<(PeerId, (Uuid, State3)), (), Error>) -> Self {
        match event {
            BehaviourOutEvent::Inbound(_, Ok((bob_peer_id, (swap_id, state3)))) => OutEvent::Done {
                bob_peer_id,
                swap_id,
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
    inner: libp2p_async_await::Behaviour<(PeerId, (Uuid, State3)), (), anyhow::Error>,
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
        self.inner.do_protocol_listener(bob, move |mut substream| {
            let protocol = async move { Ok((bob, (swap_id, state3))) };

            async move { tokio::time::timeout(Duration::from_secs(60), protocol).await? }
        });
    }
}

impl From<OutEvent> for alice::OutEvent {
    fn from(event: OutEvent) -> Self {
        match event {
            OutEvent::Done {
                bob_peer_id,
                state3,
                swap_id,
            } => Self::SwapSetupCompleted {
                peer_id: bob_peer_id,
                state3: Box::new(state3),
                swap_id,
            },
            OutEvent::Failure { peer, error } => Self::Failure { peer, error },
        }
    }
}
