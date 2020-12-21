use crate::{
    bob::event_loop::EventLoopHandle,
    state,
    state::{Bob, Swap},
    storage::Database,
    SwapAmounts,
};
use anyhow::{bail, Result};
use async_recursion::async_recursion;
use libp2p::{core::Multiaddr, PeerId};
use rand::{CryptoRng, RngCore};
use std::{convert::TryFrom, fmt, sync::Arc};
use tokio::select;
use tracing::info;
use uuid::Uuid;
use xmr_btc::{
    bob::{self, State2},
    Epoch,
};

#[derive(Debug, Clone)]
pub enum BobState {
    Started {
        state0: bob::State0,
        amounts: SwapAmounts,
        alice_peer_id: PeerId,
    },
    Negotiated(bob::State2, PeerId),
    BtcLocked(bob::State3, PeerId),
    XmrLocked(bob::State4, PeerId),
    EncSigSent(bob::State4),
    BtcRedeemed(bob::State5),
    T1Expired(bob::State4),
    Cancelled(bob::State4),
    BtcRefunded(bob::State4),
    XmrRedeemed,
    Punished,
    SafelyAborted,
}

impl fmt::Display for BobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BobState::Started { .. } => write!(f, "started"),
            BobState::Negotiated(..) => write!(f, "negotiated"),
            BobState::BtcLocked(..) => write!(f, "btc_locked"),
            BobState::XmrLocked(..) => write!(f, "xmr_locked"),
            BobState::EncSigSent(..) => write!(f, "encsig_sent"),
            BobState::BtcRedeemed(..) => write!(f, "btc_redeemed"),
            BobState::T1Expired(..) => write!(f, "t1_expired"),
            BobState::Cancelled(..) => write!(f, "cancelled"),
            BobState::BtcRefunded(..) => write!(f, "btc_refunded"),
            BobState::XmrRedeemed => write!(f, "xmr_redeemed"),
            BobState::Punished => write!(f, "punished"),
            BobState::SafelyAborted => write!(f, "safely_aborted"),
        }
    }
}

impl From<BobState> for state::Bob {
    fn from(bob_state: BobState) -> Self {
        match bob_state {
            BobState::Started { .. } => {
                // TODO: Do we want to resume just started swaps
                unimplemented!("Cannot save a swap that has just started")
            }
            BobState::Negotiated(state2, peer_id) => Bob::Negotiated { state2, peer_id },
            BobState::BtcLocked(state3, peer_id) => Bob::BtcLocked { state3, peer_id },
            BobState::XmrLocked(state4, peer_id) => Bob::XmrLocked { state4, peer_id },
            BobState::EncSigSent(state4) => Bob::EncSigSent { state4 },
            BobState::BtcRedeemed(state5) => Bob::BtcRedeemed(state5),
            BobState::T1Expired(state4) => Bob::T1Expired(state4),
            BobState::Cancelled(state4) => Bob::BtcCancelled(state4),
            BobState::BtcRefunded(_)
            | BobState::XmrRedeemed
            | BobState::Punished
            | BobState::SafelyAborted => Bob::SwapComplete,
        }
    }
}

impl TryFrom<state::Swap> for BobState {
    type Error = anyhow::Error;

    fn try_from(db_state: state::Swap) -> Result<Self, Self::Error> {
        if let Swap::Bob(state) = db_state {
            let bob_State = match state {
                Bob::Negotiated { state2, peer_id } => BobState::Negotiated(state2, peer_id),
                Bob::BtcLocked { state3, peer_id } => BobState::BtcLocked(state3, peer_id),
                Bob::XmrLocked { state4, peer_id } => BobState::XmrLocked(state4, peer_id),
                Bob::EncSigSent { state4 } => BobState::EncSigSent(state4),
                Bob::BtcRedeemed(state5) => BobState::BtcRedeemed(state5),
                Bob::T1Expired(state4) => BobState::T1Expired(state4),
                Bob::BtcCancelled(state4) => BobState::Cancelled(state4),
                Bob::SwapComplete => BobState::SafelyAborted,
            };

            Ok(bob_State)
        } else {
            bail!("Bob swap state expected.")
        }
    }
}

// TODO(Franck): Make this a method on a struct
#[allow(clippy::too_many_arguments)]
pub async fn swap<R>(
    state: BobState,
    mut event_loop_handle: EventLoopHandle,
    db: Database,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
    rng: R,
    swap_id: Uuid,
    alice_peer_id: PeerId,
    alice_addr: Multiaddr,
) -> Result<BobState>
where
    R: RngCore + CryptoRng + Send,
{
    event_loop_handle
        .add_address(alice_peer_id, alice_addr)
        .await?;

    run_until(
        state,
        is_complete,
        event_loop_handle,
        db,
        bitcoin_wallet,
        monero_wallet,
        rng,
        swap_id,
    )
    .await
}

pub fn is_complete(state: &BobState) -> bool {
    matches!(
        state,
        BobState::BtcRefunded(..)
            | BobState::XmrRedeemed
            | BobState::Punished
            | BobState::SafelyAborted
    )
}

pub fn is_btc_locked(state: &BobState) -> bool {
    matches!(state, BobState::BtcLocked(..))
}

pub fn is_xmr_locked(state: &BobState) -> bool {
    matches!(state, BobState::XmrLocked(..))
}

pub fn is_encsig_sent(state: &BobState) -> bool {
    matches!(state, BobState::EncSigSent(..))
}

// State machine driver for swap execution
#[allow(clippy::too_many_arguments)]
#[async_recursion]
pub async fn run_until<R>(
    state: BobState,
    is_target_state: fn(&BobState) -> bool,
    mut event_loop_handle: EventLoopHandle,
    db: Database,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
    mut rng: R,
    swap_id: Uuid,
) -> Result<BobState>
where
    R: RngCore + CryptoRng + Send,
{
    info!("Current state: {}", state);
    if is_target_state(&state) {
        Ok(state)
    } else {
        match state {
            BobState::Started {
                state0,
                amounts,
                alice_peer_id,
            } => {
                event_loop_handle.dial(alice_peer_id.clone()).await?;

                let (state2, alice_peer_id) = negotiate(
                    state0,
                    amounts,
                    &mut event_loop_handle,
                    alice_peer_id.clone(),
                    &mut rng,
                    bitcoin_wallet.clone(),
                )
                .await?;

                let state = BobState::Negotiated(state2, alice_peer_id);
                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, state::Swap::Bob(db_state))
                    .await?;
                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    rng,
                    swap_id,
                )
                .await
            }
            BobState::Negotiated(state2, alice_peer_id) => {
                // Do not lock Bitcoin if not connected to Alice.
                event_loop_handle.dial(alice_peer_id.clone()).await?;
                // Alice and Bob have exchanged info
                let state3 = state2.lock_btc(bitcoin_wallet.as_ref()).await?;

                let state = BobState::BtcLocked(state3, alice_peer_id);
                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, state::Swap::Bob(db_state))
                    .await?;
                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    rng,
                    swap_id,
                )
                .await
            }
            // Bob has locked Btc
            // Watch for Alice to Lock Xmr or for t1 to elapse
            BobState::BtcLocked(state3, alice_peer_id) => {
                // TODO(Franck): Refund if cannot connect to Alice.
                event_loop_handle.dial(alice_peer_id.clone()).await?;

                // todo: watch until t1, not indefinitely
                let msg2 = event_loop_handle.recv_message2().await?;
                let state4 = state3
                    .watch_for_lock_xmr(monero_wallet.as_ref(), msg2)
                    .await?;

                let state = BobState::XmrLocked(state4, alice_peer_id);
                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, state::Swap::Bob(db_state))
                    .await?;
                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    rng,
                    swap_id,
                )
                .await
            }
            BobState::XmrLocked(state, alice_peer_id) => {
                // TODO(Franck): Refund if cannot connect to Alice.
                event_loop_handle.dial(alice_peer_id.clone()).await?;

                let state = if let Epoch::T0 = state.current_epoch(bitcoin_wallet.as_ref()).await? {
                    // Alice has locked Xmr
                    // Bob sends Alice his key
                    let tx_redeem_encsig = state.tx_redeem_encsig();

                    let state4_clone = state.clone();
                    // TODO(Franck): Refund if message cannot be sent.
                    let enc_sig_sent_watcher =
                        event_loop_handle.send_message3(alice_peer_id.clone(), tx_redeem_encsig);
                    let bitcoin_wallet = bitcoin_wallet.clone();
                    let t1_timeout = state4_clone.wait_for_t1(bitcoin_wallet.as_ref());

                    select! {
                        _ = enc_sig_sent_watcher => {
                            BobState::EncSigSent(state)
                        },
                        _ = t1_timeout => {
                            BobState::T1Expired(state)
                        }
                    }
                } else {
                    BobState::T1Expired(state)
                };
                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, state::Swap::Bob(db_state))
                    .await?;
                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    rng,
                    swap_id,
                )
                .await
            }
            BobState::EncSigSent(state) => {
                let state = if let Epoch::T0 = state.current_epoch(bitcoin_wallet.as_ref()).await? {
                    let state_clone = state.clone();
                    let redeem_watcher = state_clone.watch_for_redeem_btc(bitcoin_wallet.as_ref());
                    let t1_timeout = state_clone.wait_for_t1(bitcoin_wallet.as_ref());

                    select! {
                        state5 = redeem_watcher => {
                            BobState::BtcRedeemed(state5?)
                        },
                        _ = t1_timeout => {
                            BobState::T1Expired(state)
                        }
                    }
                } else {
                    BobState::T1Expired(state)
                };

                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, state::Swap::Bob(db_state))
                    .await?;
                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet.clone(),
                    monero_wallet,
                    rng,
                    swap_id,
                )
                .await
            }
            BobState::BtcRedeemed(state) => {
                // Bob redeems XMR using revealed s_a
                state.claim_xmr(monero_wallet.as_ref()).await?;

                let state = BobState::XmrRedeemed;
                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, state::Swap::Bob(db_state))
                    .await?;
                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    rng,
                    swap_id,
                )
                .await
            }
            BobState::T1Expired(state4) => {
                if state4
                    .check_for_tx_cancel(bitcoin_wallet.as_ref())
                    .await
                    .is_err()
                {
                    state4.submit_tx_cancel(bitcoin_wallet.as_ref()).await?;
                }

                let state = BobState::Cancelled(state4);
                db.insert_latest_state(swap_id, state::Swap::Bob(state.clone().into()))
                    .await?;

                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    rng,
                    swap_id,
                )
                .await
            }
            BobState::Cancelled(state) => {
                // TODO
                // Bob has cancelled the swap
                let state = match state.current_epoch(bitcoin_wallet.as_ref()).await? {
                    Epoch::T0 => panic!("Cancelled before t1??? Something is really wrong"),
                    Epoch::T1 => {
                        state.refund_btc(bitcoin_wallet.as_ref()).await?;
                        BobState::BtcRefunded(state)
                    }
                    Epoch::T2 => BobState::Punished,
                };

                let db_state = state.clone().into();
                db.insert_latest_state(swap_id, state::Swap::Bob(db_state))
                    .await?;
                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    rng,
                    swap_id,
                )
                .await
            }
            BobState::BtcRefunded(state4) => Ok(BobState::BtcRefunded(state4)),
            BobState::Punished => Ok(BobState::Punished),
            BobState::SafelyAborted => Ok(BobState::SafelyAborted),
            BobState::XmrRedeemed => Ok(BobState::XmrRedeemed),
        }
    }
}

pub async fn negotiate<R>(
    state0: xmr_btc::bob::State0,
    amounts: SwapAmounts,
    swarm: &mut EventLoopHandle,
    alice_peer_id: PeerId,
    mut rng: R,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
) -> Result<(State2, PeerId)>
where
    R: RngCore + CryptoRng + Send,
{
    tracing::trace!("Starting negotiate");
    swarm
        .request_amounts(alice_peer_id.clone(), amounts.btc)
        .await?;

    swarm
        .send_message0(alice_peer_id.clone(), state0.next_message(&mut rng))
        .await?;
    let msg0 = swarm.recv_message0().await?;
    let state1 = state0.receive(bitcoin_wallet.as_ref(), msg0).await?;

    swarm
        .send_message1(alice_peer_id.clone(), state1.next_message())
        .await?;
    let msg1 = swarm.recv_message1().await?;
    let state2 = state1.receive(msg1)?;

    swarm
        .send_message2(alice_peer_id.clone(), state2.next_message())
        .await?;

    Ok((state2, alice_peer_id))
}
