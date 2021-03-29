use crate::network::quote::BidQuote;
use crate::network::{encrypted_signature, quote, spot_price, transfer_proof};
use crate::protocol::alice::{execution_setup, State3};
use anyhow::{anyhow, Error};
use libp2p::request_response::{RequestResponseEvent, RequestResponseMessage, ResponseChannel};
use libp2p::{NetworkBehaviour, PeerId};

#[derive(Debug)]
pub enum OutEvent {
    SpotPriceRequested {
        request: spot_price::Request,
        channel: ResponseChannel<spot_price::Response>,
        peer: PeerId,
    },
    QuoteRequested {
        channel: ResponseChannel<BidQuote>,
        peer: PeerId,
    },
    ExecutionSetupDone {
        bob_peer_id: PeerId,
        state3: Box<State3>,
    },
    TransferProofAcknowledged(PeerId),
    EncryptedSignatureReceived {
        msg: Box<encrypted_signature::Request>,
        channel: ResponseChannel<()>,
        peer: PeerId,
    },
    ResponseSent, // Same variant is used for all messages as no processing is done
    Failure {
        peer: PeerId,
        error: Error,
    },
}

impl OutEvent {
    fn unexpected_request(peer: PeerId) -> OutEvent {
        OutEvent::Failure {
            peer,
            error: anyhow!("Unexpected request received"),
        }
    }

    fn unexpected_response(peer: PeerId) -> OutEvent {
        OutEvent::Failure {
            peer,
            error: anyhow!("Unexpected response received"),
        }
    }
}

impl From<(PeerId, quote::Message)> for OutEvent {
    fn from((peer, message): (PeerId, quote::Message)) -> Self {
        match message {
            quote::Message::Request { channel, .. } => OutEvent::QuoteRequested { channel, peer },
            quote::Message::Response { .. } => OutEvent::unexpected_response(peer),
        }
    }
}

impl From<(PeerId, spot_price::Message)> for OutEvent {
    fn from((peer, message): (PeerId, spot_price::Message)) -> Self {
        match message {
            spot_price::Message::Request {
                request, channel, ..
            } => OutEvent::SpotPriceRequested {
                request,
                channel,
                peer,
            },
            spot_price::Message::Response { .. } => OutEvent::unexpected_response(peer),
        }
    }
}

impl From<(PeerId, transfer_proof::Message)> for OutEvent {
    fn from((peer, message): (PeerId, transfer_proof::Message)) -> Self {
        match message {
            transfer_proof::Message::Request { .. } => OutEvent::unexpected_request(peer),
            transfer_proof::Message::Response { .. } => OutEvent::TransferProofAcknowledged(peer),
        }
    }
}

impl From<(PeerId, encrypted_signature::Message)> for OutEvent {
    fn from((peer, message): (PeerId, encrypted_signature::Message)) -> Self {
        match message {
            encrypted_signature::Message::Request {
                request, channel, ..
            } => OutEvent::EncryptedSignatureReceived {
                msg: Box::new(request),
                channel,
                peer,
            },
            encrypted_signature::Message::Response { .. } => OutEvent::unexpected_response(peer),
        }
    }
}

impl From<spot_price::OutEvent> for OutEvent {
    fn from(event: spot_price::OutEvent) -> Self {
        map_rr_event_to_outevent(event)
    }
}

impl From<quote::OutEvent> for OutEvent {
    fn from(event: quote::OutEvent) -> Self {
        map_rr_event_to_outevent(event)
    }
}

impl From<transfer_proof::OutEvent> for OutEvent {
    fn from(event: transfer_proof::OutEvent) -> Self {
        map_rr_event_to_outevent(event)
    }
}

impl From<encrypted_signature::OutEvent> for OutEvent {
    fn from(event: encrypted_signature::OutEvent) -> Self {
        map_rr_event_to_outevent(event)
    }
}

fn map_rr_event_to_outevent<I, O>(event: RequestResponseEvent<I, O>) -> OutEvent
where
    OutEvent: From<(PeerId, RequestResponseMessage<I, O>)>,
{
    use RequestResponseEvent::*;

    match event {
        Message { message, peer, .. } => OutEvent::from((peer, message)),
        ResponseSent { .. } => OutEvent::ResponseSent,
        InboundFailure { peer, error, .. } => OutEvent::Failure {
            error: anyhow!("protocol failed due to {:?}", error),
            peer,
        },
        OutboundFailure { peer, error, .. } => OutEvent::Failure {
            error: anyhow!("protocol failed due to {:?}", error),
            peer,
        },
    }
}

impl From<execution_setup::OutEvent> for OutEvent {
    fn from(event: execution_setup::OutEvent) -> Self {
        use crate::protocol::alice::execution_setup::OutEvent::*;
        match event {
            Done {
                bob_peer_id,
                state3,
            } => OutEvent::ExecutionSetupDone {
                bob_peer_id,
                state3: Box::new(state3),
            },
            Failure { peer, error } => OutEvent::Failure { peer, error },
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pub quote: quote::Behaviour,
    pub spot_price: spot_price::Behaviour,
    pub execution_setup: execution_setup::Behaviour,
    pub transfer_proof: transfer_proof::Behaviour,
    pub encrypted_signature: encrypted_signature::Behaviour,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            quote: quote::alice(),
            spot_price: spot_price::alice(),
            execution_setup: Default::default(),
            transfer_proof: transfer_proof::alice(),
            encrypted_signature: encrypted_signature::alice(),
        }
    }
}
