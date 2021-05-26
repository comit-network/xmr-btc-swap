use crate::network::quote::BidQuote;
use crate::network::{encrypted_signature, quote, transfer_proof};
use crate::protocol::alice::event_loop::LatestRate;
use crate::protocol::alice::{execution_setup, spot_price, State3};
use crate::{env, monero};
use anyhow::{anyhow, Error};
use libp2p::ping::{Ping, PingEvent};
use libp2p::request_response::{RequestId, ResponseChannel};
use libp2p::{NetworkBehaviour, PeerId};
use uuid::Uuid;

#[derive(Debug)]
pub enum OutEvent {
    SwapRequestDeclined {
        peer: PeerId,
        error: spot_price::Error,
    },
    ExecutionSetupStart {
        peer: PeerId,
        btc: bitcoin::Amount,
        xmr: monero::Amount,
    },
    QuoteRequested {
        channel: ResponseChannel<BidQuote>,
        peer: PeerId,
    },
    ExecutionSetupDone {
        bob_peer_id: PeerId,
        swap_id: Uuid,
        state3: Box<State3>,
    },
    TransferProofAcknowledged {
        peer: PeerId,
        id: RequestId,
    },
    EncryptedSignatureReceived {
        msg: Box<encrypted_signature::Request>,
        channel: ResponseChannel<()>,
        peer: PeerId,
    },
    Failure {
        peer: PeerId,
        error: Error,
    },
    /// "Fallback" variant that allows the event mapping code to swallow certain
    /// events that we don't want the caller to deal with.
    Other,
}

impl OutEvent {
    pub fn unexpected_request(peer: PeerId) -> OutEvent {
        OutEvent::Failure {
            peer,
            error: anyhow!("Unexpected request received"),
        }
    }

    pub fn unexpected_response(peer: PeerId) -> OutEvent {
        OutEvent::Failure {
            peer,
            error: anyhow!("Unexpected response received"),
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour<LR>
where
    LR: LatestRate + Send + 'static,
{
    pub quote: quote::Behaviour,
    pub spot_price: spot_price::Behaviour<LR>,
    pub execution_setup: execution_setup::Behaviour,
    pub transfer_proof: transfer_proof::Behaviour,
    pub encrypted_signature: encrypted_signature::Behaviour,

    /// Ping behaviour that ensures that the underlying network connection is
    /// still alive. If the ping fails a connection close event will be
    /// emitted that is picked up as swarm event.
    ping: Ping,
}

impl<LR> Behaviour<LR>
where
    LR: LatestRate + Send + 'static,
{
    pub fn new(
        balance: monero::Amount,
        lock_fee: monero::Amount,
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
        latest_rate: LR,
        resume_only: bool,
        env_config: env::Config,
    ) -> Self {
        Self {
            quote: quote::alice(),
            spot_price: spot_price::Behaviour::new(
                balance,
                lock_fee,
                min_buy,
                max_buy,
                env_config,
                latest_rate,
                resume_only,
            ),
            execution_setup: Default::default(),
            transfer_proof: transfer_proof::alice(),
            encrypted_signature: encrypted_signature::alice(),
            ping: Ping::default(),
        }
    }
}

impl From<PingEvent> for OutEvent {
    fn from(_: PingEvent) -> Self {
        OutEvent::Other
    }
}
