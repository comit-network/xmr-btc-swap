//! Run an XMR/BTC swap in the role of Bob.
//! Bob holds BTC and wishes receive XMR.
use anyhow::Result;
use async_trait::async_trait;
use backoff::{backoff::Constant as ConstantBackoff, future::FutureOperation as _};
use futures::{
    channel::mpsc::{Receiver, Sender},
    FutureExt, StreamExt,
};
use genawaiter::GeneratorState;
use libp2p::{core::identity::Keypair, Multiaddr, NetworkBehaviour, PeerId};
use rand::rngs::OsRng;
use std::{process, sync::Arc, time::Duration};
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
    bitcoin::{self, TX_LOCK_MINE_TIMEOUT},
    monero,
    network::{
        peer_tracker::{self, PeerTracker},
        transport::SwapTransport,
        TokioExecutor,
    },
    state,
    storage::Database,
    Cmd, Rsp, SwapAmounts, PUNISH_TIMELOCK, REFUND_TIMELOCK,
};
use xmr_btc::{
    alice,
    bitcoin::{BroadcastSignedTransaction, EncryptedSignature, SignTxLock},
    bob::{self, action_generator, ReceiveTransferProof, State0},
    monero::CreateWalletForOutput,
};

#[allow(clippy::too_many_arguments)]
pub async fn swap(
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    db: Database,
    btc: u64,
    addr: Multiaddr,
    mut cmd_tx: Sender<Cmd>,
    mut rsp_rx: Receiver<Rsp>,
    transport: SwapTransport,
    behaviour: Bob,
) -> Result<()> {
    struct Network(Swarm);

    // TODO: For retry, use `backoff::ExponentialBackoff` in production as opposed
    // to `ConstantBackoff`.

    #[async_trait]
    impl ReceiveTransferProof for Network {
        async fn receive_transfer_proof(&mut self) -> monero::TransferProof {
            #[derive(Debug)]
            struct UnexpectedMessage;

            let future = self.0.next().shared();

            let proof = (|| async {
                let proof = match future.clone().await {
                    OutEvent::Message2(msg) => msg.tx_lock_proof,
                    other => {
                        warn!("Expected transfer proof, got: {:?}", other);
                        return Err(backoff::Error::Transient(UnexpectedMessage));
                    }
                };

                Result::<_, backoff::Error<UnexpectedMessage>>::Ok(proof)
            })
            .retry(ConstantBackoff::new(Duration::from_secs(1)))
            .await
            .expect("transient errors to be retried");

            info!("Received transfer proof");

            proof
        }
    }

    let mut swarm = new_swarm(transport, behaviour)?;

    libp2p::Swarm::dial_addr(&mut swarm, addr)?;
    let alice = match swarm.next().await {
        OutEvent::ConnectionEstablished(alice) => alice,
        other => panic!("unexpected event: {:?}", other),
    };
    info!("Connection established with: {}", alice);

    swarm.request_amounts(alice.clone(), btc);

    let (btc, xmr) = match swarm.next().await {
        OutEvent::Amounts(amounts) => {
            info!("Got amounts from Alice: {:?}", amounts);
            let cmd = Cmd::VerifyAmounts(amounts);
            cmd_tx.try_send(cmd)?;
            let response = rsp_rx.next().await;
            if response == Some(Rsp::Abort) {
                info!("User rejected amounts proposed by Alice, aborting...");
                process::exit(0);
            }

            info!("User accepted amounts proposed by Alice");
            (amounts.btc, amounts.xmr)
        }
        other => panic!("unexpected event: {:?}", other),
    };

    let refund_address = bitcoin_wallet.new_address().await?;

    // TODO: Pass this in using <R: RngCore + CryptoRng>
    let rng = &mut OsRng;
    let state0 = State0::new(
        rng,
        btc,
        xmr,
        REFUND_TIMELOCK,
        PUNISH_TIMELOCK,
        refund_address,
    );

    info!("Commencing handshake");

    swarm.send_message0(alice.clone(), state0.next_message(rng));
    let state1 = match swarm.next().await {
        OutEvent::Message0(msg) => state0.receive(bitcoin_wallet.as_ref(), msg).await?,
        other => panic!("unexpected event: {:?}", other),
    };

    swarm.send_message1(alice.clone(), state1.next_message());
    let state2 = match swarm.next().await {
        OutEvent::Message1(msg) => {
            state1.receive(msg)? // TODO: Same as above.
        }
        other => panic!("unexpected event: {:?}", other),
    };

    let swap_id = Uuid::new_v4();
    db.insert_latest_state(swap_id, state::Bob::Handshaken(state2.clone()).into())
        .await?;

    swarm.send_message2(alice.clone(), state2.next_message());

    info!("Handshake complete");

    let network = Arc::new(Mutex::new(Network(swarm)));

    let mut action_generator = action_generator(
        network.clone(),
        monero_wallet.clone(),
        bitcoin_wallet.clone(),
        state2.clone(),
        TX_LOCK_MINE_TIMEOUT,
    );

    loop {
        let state = action_generator.async_resume().await;

        info!("Resumed execution of generator, got: {:?}", state);

        // TODO: Protect against transient errors
        // TODO: Ignore transaction-already-in-block-chain errors

        match state {
            GeneratorState::Yielded(bob::Action::LockBtc(tx_lock)) => {
                let signed_tx_lock = bitcoin_wallet.sign_tx_lock(tx_lock).await?;
                let _ = bitcoin_wallet
                    .broadcast_signed_transaction(signed_tx_lock)
                    .await?;
                db.insert_latest_state(swap_id, state::Bob::BtcLocked(state2.clone()).into())
                    .await?;
            }
            GeneratorState::Yielded(bob::Action::SendBtcRedeemEncsig(tx_redeem_encsig)) => {
                db.insert_latest_state(swap_id, state::Bob::XmrLocked(state2.clone()).into())
                    .await?;

                let mut guard = network.as_ref().lock().await;
                guard.0.send_message3(alice.clone(), tx_redeem_encsig);
                info!("Sent Bitcoin redeem encsig");

                // FIXME: Having to wait for Alice's response here is a big problem, because
                // we're stuck if she doesn't send her response back. I believe this is
                // currently necessary, so we may have to rework this and/or how we use libp2p
                match guard.0.next().shared().await {
                    OutEvent::Message3 => {
                        debug!("Got Message3 empty response");
                    }
                    other => panic!("unexpected event: {:?}", other),
                };
            }
            GeneratorState::Yielded(bob::Action::CreateXmrWalletForOutput {
                spend_key,
                view_key,
            }) => {
                db.insert_latest_state(swap_id, state::Bob::BtcRedeemed(state2.clone()).into())
                    .await?;

                monero_wallet
                    .create_and_load_wallet_for_output(spend_key, view_key)
                    .await?;
            }
            GeneratorState::Yielded(bob::Action::CancelBtc(tx_cancel)) => {
                db.insert_latest_state(swap_id, state::Bob::BtcRefundable(state2.clone()).into())
                    .await?;

                let _ = bitcoin_wallet
                    .broadcast_signed_transaction(tx_cancel)
                    .await?;
            }
            GeneratorState::Yielded(bob::Action::RefundBtc(tx_refund)) => {
                db.insert_latest_state(swap_id, state::Bob::BtcRefundable(state2.clone()).into())
                    .await?;

                let _ = bitcoin_wallet
                    .broadcast_signed_transaction(tx_refund)
                    .await?;
            }
            GeneratorState::Complete(()) => {
                db.insert_latest_state(swap_id, state::Bob::SwapComplete.into())
                    .await?;

                return Ok(());
            }
        }
    }
}

pub type Swarm = libp2p::Swarm<Bob>;

fn new_swarm(transport: SwapTransport, behaviour: Bob) -> Result<Swarm> {
    let local_peer_id = behaviour.peer_id();

    let swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, local_peer_id.clone())
        .executor(Box::new(TokioExecutor {
            handle: tokio::runtime::Handle::current(),
        }))
        .build();

    info!("Initialized swarm with identity {}", local_peer_id);

    Ok(swarm)
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
    Amounts(SwapAmounts),
    Message0(alice::Message0),
    Message1(alice::Message1),
    Message2(alice::Message2),
    Message3,
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
        match event {
            amounts::OutEvent::Amounts(amounts) => OutEvent::Amounts(amounts),
        }
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
            message1::OutEvent::Msg(msg) => OutEvent::Message1(msg),
        }
    }
}

impl From<message2::OutEvent> for OutEvent {
    fn from(event: message2::OutEvent) -> Self {
        match event {
            message2::OutEvent::Msg(msg) => OutEvent::Message2(msg),
        }
    }
}

impl From<message3::OutEvent> for OutEvent {
    fn from(event: message3::OutEvent) -> Self {
        match event {
            message3::OutEvent::Msg => OutEvent::Message3,
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Bob.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Bob {
    pt: PeerTracker,
    amounts: Amounts,
    message0: Message0,
    message1: Message1,
    message2: Message2,
    message3: Message3,
    #[behaviour(ignore)]
    identity: Keypair,
}

impl Bob {
    pub fn identity(&self) -> Keypair {
        self.identity.clone()
    }

    pub fn peer_id(&self) -> PeerId {
        PeerId::from(self.identity.public())
    }

    /// Sends a message to Alice to get current amounts based on `btc`.
    pub fn request_amounts(&mut self, alice: PeerId, btc: u64) {
        let btc = ::bitcoin::Amount::from_sat(btc);
        let _id = self.amounts.request_amounts(alice.clone(), btc);
        info!("Requesting amounts from: {}", alice);
    }

    /// Sends Bob's first message to Alice.
    pub fn send_message0(&mut self, alice: PeerId, msg: bob::Message0) {
        self.message0.send(alice, msg);
        debug!("Sent Message0");
    }

    /// Sends Bob's second message to Alice.
    pub fn send_message1(&mut self, alice: PeerId, msg: bob::Message1) {
        self.message1.send(alice, msg);
        debug!("Sent Message1");
    }

    /// Sends Bob's third message to Alice.
    pub fn send_message2(&mut self, alice: PeerId, msg: bob::Message2) {
        self.message2.send(alice, msg);
        debug!("Sent Message2");
    }

    /// Sends Bob's fourth message to Alice.
    pub fn send_message3(&mut self, alice: PeerId, tx_redeem_encsig: EncryptedSignature) {
        let msg = bob::Message3 { tx_redeem_encsig };
        self.message3.send(alice, msg);
        debug!("Sent Message3");
    }

    /// Returns Alice's peer id if we are connected.
    pub fn peer_id_of_alice(&self) -> Option<PeerId> {
        self.pt.counterparty_peer_id()
    }
}

impl Default for Bob {
    fn default() -> Bob {
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
