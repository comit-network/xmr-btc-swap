//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use anyhow::Result;
use async_trait::async_trait;
use backoff::{backoff::Constant as ConstantBackoff, future::FutureOperation as _};
use genawaiter::GeneratorState;
use libp2p::{
    core::{identity::Keypair, Multiaddr},
    request_response::ResponseChannel,
    NetworkBehaviour, PeerId,
};
use rand::rngs::OsRng;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use uuid::Uuid;

mod amounts;
mod message0;
mod message1;
mod message2;
mod message3;

use self::{amounts::*, message0::*, message1::*, message2::*, message3::*};
use crate::{
    bitcoin,
    bitcoin::TX_LOCK_MINE_TIMEOUT,
    monero,
    network::{
        peer_tracker::{self, PeerTracker},
        request_response::AliceToBob,
        transport::SwapTransport,
        TokioExecutor,
    },
    state,
    storage::Database,
    SwapAmounts, PUNISH_TIMELOCK, REFUND_TIMELOCK,
};
use xmr_btc::{
    alice::{self, action_generator, Action, ReceiveBitcoinRedeemEncsig, State0},
    bitcoin::BroadcastSignedTransaction,
    bob,
    monero::{CreateWalletForOutput, Transfer},
};

pub async fn swap(
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    db: Database,
    listen: Multiaddr,
    transport: SwapTransport,
    behaviour: Alice,
) -> Result<()> {
    struct Network {
        swarm: Arc<Mutex<Swarm>>,
        channel: Option<ResponseChannel<AliceToBob>>,
    }

    impl Network {
        pub async fn send_message2(&mut self, proof: monero::TransferProof) {
            match self.channel.take() {
                None => warn!("Channel not found, did you call this twice?"),
                Some(channel) => {
                    let mut guard = self.swarm.lock().await;
                    guard.send_message2(channel, alice::Message2 {
                        tx_lock_proof: proof,
                    });
                    info!("Sent transfer proof");
                }
            }
        }
    }

    // TODO: For retry, use `backoff::ExponentialBackoff` in production as opposed
    // to `ConstantBackoff`.
    #[async_trait]
    impl ReceiveBitcoinRedeemEncsig for Network {
        async fn receive_bitcoin_redeem_encsig(&mut self) -> bitcoin::EncryptedSignature {
            #[derive(Debug)]
            struct UnexpectedMessage;

            let encsig = (|| async {
                let mut guard = self.swarm.lock().await;
                let encsig = match guard.next().await {
                    OutEvent::Message3(msg) => msg.tx_redeem_encsig,
                    other => {
                        warn!("Expected Bob's Bitcoin redeem encsig, got: {:?}", other);
                        return Err(backoff::Error::Transient(UnexpectedMessage));
                    }
                };

                Result::<_, backoff::Error<UnexpectedMessage>>::Ok(encsig)
            })
            .retry(ConstantBackoff::new(Duration::from_secs(1)))
            .await
            .expect("transient errors to be retried");

            info!("Received Bitcoin redeem encsig");

            encsig
        }
    }

    let mut swarm = new_swarm(listen, transport, behaviour)?;
    let message0: bob::Message0;
    let mut state0: Option<alice::State0> = None;
    let mut last_amounts: Option<SwapAmounts> = None;

    // TODO: This loop is a neat idea for local development, as it allows us to keep
    // Alice up and let Bob keep trying to connect, request amounts and/or send the
    // first message of the handshake, but it comes at the cost of needing to handle
    // mutable state, which has already been the source of a bug at one point. This
    // is an obvious candidate for refactoring
    loop {
        match swarm.next().await {
            OutEvent::ConnectionEstablished(bob) => {
                info!("Connection established with: {}", bob);
            }
            OutEvent::Request(amounts::OutEvent::Btc { btc, channel }) => {
                let amounts = calculate_amounts(btc);
                last_amounts = Some(amounts);
                swarm.send_amounts(channel, amounts);

                let SwapAmounts { btc, xmr } = amounts;

                let redeem_address = bitcoin_wallet.as_ref().new_address().await?;
                let punish_address = redeem_address.clone();

                // TODO: Pass this in using <R: RngCore + CryptoRng>
                let rng = &mut OsRng;
                let state = State0::new(
                    rng,
                    btc,
                    xmr,
                    REFUND_TIMELOCK,
                    PUNISH_TIMELOCK,
                    redeem_address,
                    punish_address,
                );

                info!("Commencing handshake");
                swarm.set_state0(state.clone());

                state0 = Some(state)
            }
            OutEvent::Message0(msg) => {
                // We don't want Bob to be able to crash us by sending an out of
                // order message. Keep looping if Bob has not requested amounts.
                if last_amounts.is_some() {
                    // TODO: We should verify the amounts and notify Bob if they have changed.
                    message0 = msg;
                    break;
                }
            }
            other => panic!("Unexpected event: {:?}", other),
        };
    }

    let state1 = state0.expect("to be set").receive(message0)?;

    let (state2, channel) = match swarm.next().await {
        OutEvent::Message1 { msg, channel } => {
            let state2 = state1.receive(msg);
            (state2, channel)
        }
        other => panic!("Unexpected event: {:?}", other),
    };

    let msg = state2.next_message();
    swarm.send_message1(channel, msg);

    let (state3, channel) = match swarm.next().await {
        OutEvent::Message2 { msg, channel } => {
            let state3 = state2.receive(msg)?;
            (state3, channel)
        }
        other => panic!("Unexpected event: {:?}", other),
    };

    let swap_id = Uuid::new_v4();
    db.insert_latest_state(swap_id, state::Alice::Handshaken(state3.clone()).into())
        .await?;

    info!("Handshake complete, we now have State3 for Alice.");

    let network = Arc::new(Mutex::new(Network {
        swarm: Arc::new(Mutex::new(swarm)),
        channel: Some(channel),
    }));

    let mut action_generator = action_generator(
        network.clone(),
        bitcoin_wallet.clone(),
        state3.clone(),
        TX_LOCK_MINE_TIMEOUT,
    );

    loop {
        let state = action_generator.async_resume().await;

        tracing::info!("Resumed execution of generator, got: {:?}", state);

        match state {
            GeneratorState::Yielded(Action::LockXmr {
                amount,
                public_spend_key,
                public_view_key,
            }) => {
                db.insert_latest_state(swap_id, state::Alice::BtcLocked(state3.clone()).into())
                    .await?;

                let (transfer_proof, _) = monero_wallet
                    .transfer(public_spend_key, public_view_key, amount)
                    .await?;

                db.insert_latest_state(swap_id, state::Alice::XmrLocked(state3.clone()).into())
                    .await?;

                let mut guard = network.as_ref().lock().await;
                guard.send_message2(transfer_proof).await;
                info!("Sent transfer proof");
            }

            GeneratorState::Yielded(Action::RedeemBtc(tx)) => {
                db.insert_latest_state(
                    swap_id,
                    state::Alice::BtcRedeemable {
                        state: state3.clone(),
                        redeem_tx: tx.clone(),
                    }
                    .into(),
                )
                .await?;

                let _ = bitcoin_wallet.broadcast_signed_transaction(tx).await?;
            }
            GeneratorState::Yielded(Action::CancelBtc(tx)) => {
                let _ = bitcoin_wallet.broadcast_signed_transaction(tx).await?;
            }
            GeneratorState::Yielded(Action::PunishBtc(tx)) => {
                db.insert_latest_state(swap_id, state::Alice::BtcPunishable(state3.clone()).into())
                    .await?;

                let _ = bitcoin_wallet.broadcast_signed_transaction(tx).await?;
            }
            GeneratorState::Yielded(Action::CreateMoneroWalletForOutput {
                spend_key,
                view_key,
            }) => {
                db.insert_latest_state(
                    swap_id,
                    state::Alice::BtcRefunded {
                        state: state3.clone(),
                        spend_key,
                        view_key,
                    }
                    .into(),
                )
                .await?;

                monero_wallet
                    .create_and_load_wallet_for_output(spend_key, view_key)
                    .await?;
            }
            GeneratorState::Complete(()) => {
                db.insert_latest_state(swap_id, state::Alice::SwapComplete.into())
                    .await?;

                return Ok(());
            }
        }
    }
}

pub type Swarm = libp2p::Swarm<Alice>;

fn new_swarm(listen: Multiaddr, transport: SwapTransport, behaviour: Alice) -> Result<Swarm> {
    use anyhow::Context as _;

    let local_peer_id = behaviour.peer_id();

    let mut swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, local_peer_id.clone())
        .executor(Box::new(TokioExecutor {
            handle: tokio::runtime::Handle::current(),
        }))
        .build();

    Swarm::listen_on(&mut swarm, listen.clone())
        .with_context(|| format!("Address is not supported: {:#}", listen))?;

    tracing::info!("Initialized swarm: {}", local_peer_id);

    Ok(swarm)
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
    Request(amounts::OutEvent), // Not-uniform with Bob on purpose, ready for adding Xmr event.
    Message0(bob::Message0),
    Message1 {
        msg: bob::Message1,
        channel: ResponseChannel<AliceToBob>,
    },
    Message2 {
        msg: bob::Message2,
        channel: ResponseChannel<AliceToBob>,
    },
    Message3(bob::Message3),
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

impl From<amounts::OutEvent> for OutEvent {
    fn from(event: amounts::OutEvent) -> Self {
        OutEvent::Request(event)
    }
}

impl From<message0::OutEvent> for OutEvent {
    fn from(event: message0::OutEvent) -> Self {
        match event {
            message0::OutEvent::Msg(msg) => OutEvent::Message0(msg),
        }
    }
}

impl From<message1::OutEvent> for OutEvent {
    fn from(event: message1::OutEvent) -> Self {
        match event {
            message1::OutEvent::Msg { msg, channel } => OutEvent::Message1 { msg, channel },
        }
    }
}

impl From<message2::OutEvent> for OutEvent {
    fn from(event: message2::OutEvent) -> Self {
        match event {
            message2::OutEvent::Msg { msg, channel } => OutEvent::Message2 { msg, channel },
        }
    }
}

impl From<message3::OutEvent> for OutEvent {
    fn from(event: message3::OutEvent) -> Self {
        match event {
            message3::OutEvent::Msg(msg) => OutEvent::Message3(msg),
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Alice {
    pt: PeerTracker,
    amounts: Amounts,
    message0: Message0,
    message1: Message1,
    message2: Message2,
    message3: Message3,
    #[behaviour(ignore)]
    identity: Keypair,
}

impl Alice {
    pub fn identity(&self) -> Keypair {
        self.identity.clone()
    }

    pub fn peer_id(&self) -> PeerId {
        PeerId::from(self.identity.public())
    }

    /// Alice always sends her messages as a response to a request from Bob.
    pub fn send_amounts(&mut self, channel: ResponseChannel<AliceToBob>, amounts: SwapAmounts) {
        let msg = AliceToBob::Amounts(amounts);
        self.amounts.send(channel, msg);
        info!("Sent amounts response");
    }

    /// Message0 gets sent within the network layer using this state0.
    pub fn set_state0(&mut self, state: State0) {
        debug!("Set state 0");
        let _ = self.message0.set_state(state);
    }

    /// Send Message1 to Bob in response to receiving his Message1.
    pub fn send_message1(
        &mut self,
        channel: ResponseChannel<AliceToBob>,
        msg: xmr_btc::alice::Message1,
    ) {
        self.message1.send(channel, msg);
        debug!("Sent Message1");
    }

    /// Send Message2 to Bob in response to receiving his Message2.
    pub fn send_message2(
        &mut self,
        channel: ResponseChannel<AliceToBob>,
        msg: xmr_btc::alice::Message2,
    ) {
        self.message2.send(channel, msg);
        debug!("Sent Message2");
    }
}

impl Default for Alice {
    fn default() -> Self {
        let identity = Keypair::generate_ed25519();

        Self {
            pt: PeerTracker::default(),
            amounts: Amounts::default(),
            message0: Message0::default(),
            message1: Message1::default(),
            message2: Message2::default(),
            message3: Message3::default(),
            identity,
        }
    }
}

fn calculate_amounts(btc: ::bitcoin::Amount) -> SwapAmounts {
    // TODO: Get this from an exchange.
    // This value corresponds to 100 XMR per BTC
    const PICONERO_PER_SAT: u64 = 1_000_000;

    let picos = btc.as_sat() * PICONERO_PER_SAT;
    let xmr = monero::Amount::from_piconero(picos);

    SwapAmounts { btc, xmr }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_BTC: u64 = 100_000_000;
    const HUNDRED_XMR: u64 = 100_000_000_000_000;

    #[test]
    fn one_bitcoin_equals_a_hundred_moneroj() {
        let btc = ::bitcoin::Amount::from_sat(ONE_BTC);
        let want = monero::Amount::from_piconero(HUNDRED_XMR);

        let SwapAmounts { xmr: got, .. } = calculate_amounts(btc);
        assert_eq!(got, want);
    }
}
