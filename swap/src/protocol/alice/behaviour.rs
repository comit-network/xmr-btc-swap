use crate::env::Config;
use crate::network::quote::BidQuote;
use crate::network::{peer_tracker, quote, spot_price};
use crate::protocol::alice::{
    encrypted_signature, execution_setup, transfer_proof, State0, State3, TransferProof,
};
use crate::protocol::bob::EncryptedSignature;
use crate::{bitcoin, monero};
use anyhow::{anyhow, Error, Result};
use libp2p::request_response::{RequestResponseMessage, ResponseChannel};
use libp2p::{NetworkBehaviour, PeerId};
use rand::{CryptoRng, RngCore};
use tracing::debug;

#[derive(Debug)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
    SpotPriceRequested {
        msg: spot_price::Request,
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
    EncryptedSignature {
        msg: Box<EncryptedSignature>,
        channel: ResponseChannel<()>,
        peer: PeerId,
    },
    ResponseSent, // Same variant is used for all messages as no processing is done
    Failure {
        peer: PeerId,
        error: Error,
    },
}

impl From<peer_tracker::OutEvent> for OutEvent {
    fn from(event: peer_tracker::OutEvent) -> Self {
        match event {
            peer_tracker::OutEvent::ConnectionEstablished(id) => {
                OutEvent::ConnectionEstablished(id)
            }
        }
    }
}

impl From<spot_price::OutEvent> for OutEvent {
    fn from(event: spot_price::OutEvent) -> Self {
        match event {
            spot_price::OutEvent::Message {
                peer,
                message:
                    RequestResponseMessage::Request {
                        channel,
                        request: msg,
                        ..
                    },
            } => OutEvent::SpotPriceRequested { msg, channel, peer },
            spot_price::OutEvent::Message {
                message: RequestResponseMessage::Response { .. },
                peer,
            } => OutEvent::Failure {
                error: anyhow!("Alice is only meant to hand out spot prices, not receive them"),
                peer,
            },
            spot_price::OutEvent::ResponseSent { .. } => OutEvent::ResponseSent,
            spot_price::OutEvent::InboundFailure { peer, error, .. } => OutEvent::Failure {
                error: anyhow!("spot_price protocol failed due to {:?}", error),
                peer,
            },
            spot_price::OutEvent::OutboundFailure { peer, error, .. } => OutEvent::Failure {
                error: anyhow!("spot_price protocol failed due to {:?}", error),
                peer,
            },
        }
    }
}

impl From<quote::OutEvent> for OutEvent {
    fn from(event: quote::OutEvent) -> Self {
        match event {
            quote::OutEvent::Message {
                peer,
                message: RequestResponseMessage::Request { channel, .. },
            } => OutEvent::QuoteRequested { channel, peer },
            quote::OutEvent::Message {
                message: RequestResponseMessage::Response { .. },
                peer,
            } => OutEvent::Failure {
                error: anyhow!("Alice is only meant to hand out quotes, not receive them"),
                peer,
            },
            quote::OutEvent::ResponseSent { .. } => OutEvent::ResponseSent,
            quote::OutEvent::InboundFailure { peer, error, .. } => OutEvent::Failure {
                error: anyhow!("quote protocol failed due to {:?}", error),
                peer,
            },
            quote::OutEvent::OutboundFailure { peer, error, .. } => OutEvent::Failure {
                error: anyhow!("quote protocol failed due to {:?}", error),
                peer,
            },
        }
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

impl From<transfer_proof::OutEvent> for OutEvent {
    fn from(event: transfer_proof::OutEvent) -> Self {
        use crate::protocol::alice::transfer_proof::OutEvent::*;
        match event {
            Acknowledged(peer) => OutEvent::TransferProofAcknowledged(peer),
            Failure { peer, error } => OutEvent::Failure {
                peer,
                error: error.context("Failure with Transfer Proof"),
            },
        }
    }
}

impl From<encrypted_signature::OutEvent> for OutEvent {
    fn from(event: encrypted_signature::OutEvent) -> Self {
        use crate::protocol::alice::encrypted_signature::OutEvent::*;
        match event {
            MsgReceived { msg, channel, peer } => OutEvent::EncryptedSignature {
                msg: Box::new(msg),
                channel,
                peer,
            },
            AckSent => OutEvent::ResponseSent,
            Failure { peer, error } => OutEvent::Failure {
                peer,
                error: error.context("Failure with Encrypted Signature"),
            },
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pt: peer_tracker::Behaviour,
    quote: quote::Behaviour,
    spot_price: spot_price::Behaviour,
    execution_setup: execution_setup::Behaviour,
    transfer_proof: transfer_proof::Behaviour,
    encrypted_signature: encrypted_signature::Behaviour,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            pt: Default::default(),
            quote: quote::alice(),
            spot_price: spot_price::alice(),
            execution_setup: Default::default(),
            transfer_proof: Default::default(),
            encrypted_signature: Default::default(),
        }
    }
}

impl Behaviour {
    pub fn send_quote(
        &mut self,
        channel: ResponseChannel<BidQuote>,
        response: BidQuote,
    ) -> Result<()> {
        self.quote
            .send_response(channel, response)
            .map_err(|_| anyhow!("Failed to respond with quote"))?;

        Ok(())
    }

    pub fn send_spot_price(
        &mut self,
        channel: ResponseChannel<spot_price::Response>,
        response: spot_price::Response,
    ) -> Result<()> {
        self.spot_price
            .send_response(channel, response)
            .map_err(|_| anyhow!("Failed to respond with spot price"))?;

        Ok(())
    }

    pub async fn start_execution_setup(
        &mut self,
        peer: PeerId,
        btc: bitcoin::Amount,
        xmr: monero::Amount,
        env_config: Config,
        bitcoin_wallet: &bitcoin::Wallet,
        rng: &mut (impl RngCore + CryptoRng),
    ) -> Result<()> {
        let state0 = State0::new(btc, xmr, env_config, bitcoin_wallet, rng).await?;

        tracing::info!(
            %peer,
            "Starting execution setup to sell {} for {}",
            xmr, btc,
        );

        self.execution_setup.run(peer, state0);

        Ok(())
    }

    /// Send Transfer Proof to Bob.
    pub fn send_transfer_proof(&mut self, bob: PeerId, msg: TransferProof) {
        self.transfer_proof.send(bob, msg);
        debug!("Sent Transfer Proof");
    }

    pub fn send_encrypted_signature_ack(&mut self, channel: ResponseChannel<()>) -> Result<()> {
        self.encrypted_signature.send_ack(channel)
    }
}
