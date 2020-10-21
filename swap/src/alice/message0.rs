use anyhow::{bail, Result};
use libp2p::{
    request_response::{
        handler::RequestProtocol, ProtocolSupport, RequestResponse, RequestResponseConfig,
        RequestResponseEvent, RequestResponseMessage,
    },
    swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters},
    NetworkBehaviour,
};
use rand::rngs::OsRng;
use std::{
    collections::VecDeque,
    task::{Context, Poll},
    time::Duration,
};
use tracing::error;

use crate::network::request_response::{AliceToBob, BobToAlice, Codec, Protocol};
use xmr_btc::{alice::State0, bob};

#[derive(Debug)]
pub enum OutEvent {
    Msg(bob::Message0),
}

/// A `NetworkBehaviour` that represents getting the amounts of an XMR/BTC swap.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", poll_method = "poll")]
#[allow(missing_debug_implementations)]
pub struct Message0 {
    rr: RequestResponse<Codec>,
    #[behaviour(ignore)]
    events: VecDeque<OutEvent>,
    #[behaviour(ignore)]
    state: Option<State0>,
}

impl Message0 {
    pub fn new(timeout: Duration) -> Self {
        let mut config = RequestResponseConfig::default();
        config.set_request_timeout(timeout);

        Self {
            rr: RequestResponse::new(
                Codec::default(),
                vec![(Protocol, ProtocolSupport::Full)],
                config,
            ),
            events: Default::default(),
            state: None,
        }
    }

    pub fn set_state(&mut self, state: State0) -> Result<()> {
        if self.state.is_some() {
            bail!("Trying to set state a second time");
        }
        self.state = Some(state);

        Ok(())
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<RequestProtocol<Codec>, OutEvent>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }

        Poll::Pending
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<BobToAlice, AliceToBob>> for Message0 {
    fn inject_event(&mut self, event: RequestResponseEvent<BobToAlice, AliceToBob>) {
        match event {
            RequestResponseEvent::Message {
                peer: _,
                message:
                    RequestResponseMessage::Request {
                        request,
                        request_id: _,
                        channel,
                    },
            } => match request {
                BobToAlice::Message0(msg) => {
                    let response = match self.state {
                        None => panic!("No state, did you forget to set it?"),
                        Some(state) => {
                            // TODO: Get OsRng from somewhere?
                            AliceToBob::Message0(state.next_message(&mut OsRng))
                        }
                    };
                    self.rr.send_response(channel, response);
                    self.events.push_back(OutEvent::Msg(msg));
                }
                _ => panic!("unexpected request"),
            },
            RequestResponseEvent::Message {
                peer: _,
                message:
                    RequestResponseMessage::Response {
                        response: _,
                        request_id: _,
                    },
            } => panic!("unexpected response"),
            RequestResponseEvent::InboundFailure {
                peer: _,
                request_id: _,
                error,
            } => {
                error!("Inbound failure: {:?}", error);
            }
            RequestResponseEvent::OutboundFailure {
                peer: _,
                request_id: _,
                error,
            } => {
                error!("Outbound failure: {:?}", error);
            }
        }
    }
}
