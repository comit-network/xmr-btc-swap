use crate::monero::{Scalar, TransferProof};
use crate::network::cooperative_xmr_redeem_after_punish::CooperativeXmrRedeemRejectReason;
use crate::network::quote::BidQuote;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::network::swap_setup::bob;
use crate::network::{
    cooperative_xmr_redeem_after_punish, encrypted_signature, quote, redial, transfer_proof,
};
use crate::protocol::bob::State2;
use crate::{bitcoin, env};
use anyhow::{anyhow, Error, Result};
use libp2p::request_response::{
    InboundFailure, InboundRequestId, OutboundFailure, OutboundRequestId, ResponseChannel,
};
use libp2p::swarm::NetworkBehaviour;
use libp2p::{identify, identity, ping, PeerId};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
pub enum OutEvent {
    QuoteReceived {
        id: OutboundRequestId,
        response: BidQuote,
    },
    SwapSetupCompleted(Box<Result<State2>>),
    TransferProofReceived {
        msg: Box<transfer_proof::Request>,
        channel: ResponseChannel<()>,
        peer: PeerId,
    },
    EncryptedSignatureAcknowledged {
        id: OutboundRequestId,
    },
    CooperativeXmrRedeemFulfilled {
        id: OutboundRequestId,
        s_a: Scalar,
        swap_id: uuid::Uuid,
        lock_transfer_proof: TransferProof,
    },
    CooperativeXmrRedeemRejected {
        id: OutboundRequestId,
        reason: CooperativeXmrRedeemRejectReason,
        swap_id: uuid::Uuid,
    },
    Failure {
        peer: PeerId,
        error: Error,
    },
    OutboundRequestResponseFailure {
        peer: PeerId,
        error: OutboundFailure,
        request_id: OutboundRequestId,
        protocol: String,
    },
    InboundRequestResponseFailure {
        peer: PeerId,
        error: InboundFailure,
        request_id: InboundRequestId,
        protocol: String,
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

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Bob.
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "OutEvent")]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pub quote: quote::Behaviour,
    pub swap_setup: bob::Behaviour,
    pub transfer_proof: transfer_proof::Behaviour,
    pub cooperative_xmr_redeem: cooperative_xmr_redeem_after_punish::Behaviour,
    pub encrypted_signature: encrypted_signature::Behaviour,
    pub redial: redial::Behaviour,
    pub identify: identify::Behaviour,

    /// Ping behaviour that ensures that the underlying network connection is
    /// still alive. If the ping fails a connection close event will be
    /// emitted that is picked up as swarm event.
    ping: ping::Behaviour,
}

impl Behaviour {
    pub fn new(
        alice: PeerId,
        env_config: env::Config,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        identify_params: (identity::Keypair, XmrBtcNamespace),
    ) -> Self {
        let agentVersion = format!("cli/{} ({})", env!("CARGO_PKG_VERSION"), identify_params.1);
        let protocolVersion = "/comit/xmr/btc/1.0.0".to_string();

        let identifyConfig = identify::Config::new(protocolVersion, identify_params.0.public())
            .with_agent_version(agentVersion);

        let pingConfig = ping::Config::new().with_timeout(Duration::from_secs(60));

        Self {
            quote: quote::cli(),
            swap_setup: bob::Behaviour::new(env_config, bitcoin_wallet),
            transfer_proof: transfer_proof::bob(),
            encrypted_signature: encrypted_signature::bob(),
            cooperative_xmr_redeem: cooperative_xmr_redeem_after_punish::bob(),
            redial: redial::Behaviour::new(
                alice,
                Duration::from_secs(2),
                Duration::from_secs(5 * 60),
            ),
            ping: ping::Behaviour::new(pingConfig),
            identify: identify::Behaviour::new(identifyConfig),
        }
    }
}

impl From<ping::Event> for OutEvent {
    fn from(_: ping::Event) -> Self {
        OutEvent::Other
    }
}

impl From<identify::Event> for OutEvent {
    fn from(_: identify::Event) -> Self {
        OutEvent::Other
    }
}
