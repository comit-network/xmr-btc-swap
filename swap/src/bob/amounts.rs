use anyhow::Result;
use libp2p::{
    request_response::{
        handler::RequestProtocol, ProtocolSupport, RequestId, RequestResponse,
        RequestResponseConfig, RequestResponseEvent, RequestResponseMessage,
    },
    swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters},
    NetworkBehaviour, PeerId,
};
use std::{
    collections::VecDeque,
    task::{Context, Poll},
    time::Duration,
};
use tracing::{debug, error};

use crate::{
    network::request_response::{AliceToBob, AmountsProtocol, BobToAlice, Codec, TIMEOUT},
    SwapAmounts,
};

#[derive(Copy, Clone, Debug)]
pub enum OutEvent {
    Amounts(SwapAmounts),
}

/// A `NetworkBehaviour` that represents getting the amounts of an XMR/BTC swap.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", poll_method = "poll")]
#[allow(missing_debug_implementations)]
pub struct Amounts {
    rr: RequestResponse<Codec<AmountsProtocol>>,
    #[behaviour(ignore)]
    events: VecDeque<OutEvent>,
}

impl Amounts {
    pub fn request_amounts(&mut self, alice: PeerId, btc: ::bitcoin::Amount) -> Result<RequestId> {
        let msg = BobToAlice::AmountsFromBtc(btc);
        let id = self.rr.send_request(&alice, msg);

        Ok(id)
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<RequestProtocol<Codec<AmountsProtocol>>, OutEvent>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }

        Poll::Pending
    }
}

impl Default for Amounts {
    fn default() -> Self {
        let timeout = Duration::from_secs(TIMEOUT);

        let mut config = RequestResponseConfig::default();
        config.set_request_timeout(timeout);

        Self {
            rr: RequestResponse::new(
                Codec::default(),
                vec![(AmountsProtocol, ProtocolSupport::Full)],
                config,
            ),
            events: Default::default(),
        }
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<BobToAlice, AliceToBob>> for Amounts {
    fn inject_event(&mut self, event: RequestResponseEvent<BobToAlice, AliceToBob>) {
        match event {
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Request { .. },
                ..
            } => panic!("Bob should never get a request from Alice"),
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Response { response, .. },
                ..
            } => {
                if let AliceToBob::Amounts(p) = response {
                    debug!("Received amounts response");
                    self.events.push_back(OutEvent::Amounts(p));
                }
            }
            RequestResponseEvent::InboundFailure { error, .. } => {
                error!("Inbound failure: {:?}", error);
            }
            RequestResponseEvent::OutboundFailure { error, .. } => {
                error!("Outbound failure: {:?}", error);
            }
        }
    }
}
