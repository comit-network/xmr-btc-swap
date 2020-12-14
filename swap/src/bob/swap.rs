use crate::{
    bob::{event_loop::EventLoopHandle, negotiate::negotiate},
    state,
    state::Bob,
    storage::Database,
    SwapAmounts,
};
use anyhow::Result;
use async_recursion::async_recursion;
use libp2p::{core::Multiaddr, PeerId};
use rand::{CryptoRng, RngCore};
use std::{fmt, sync::Arc};
use tracing::info;
use uuid::Uuid;
use xmr_btc::{
    bob::{self},
    Epoch,
};

// The same data structure is used for swap execution and recovery.
// This allows for a seamless transition from a failed swap to recovery.
#[derive(Debug, Clone)]
pub enum BobState {
    Started {
        state0: bob::State0,
        amounts: SwapAmounts,
        addr: Multiaddr,
    },
    Negotiated(bob::State2, PeerId),
    BtcLocked(bob::State3, PeerId),
    XmrLocked(bob::State4, PeerId),
    EncSigSent(bob::State4, PeerId),
    BtcRedeemed(bob::State5),
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
            BobState::EncSigSent(state4, peer_id) => Bob::EncSigSent { state4, peer_id },
            BobState::BtcRedeemed(state5) => Bob::BtcRedeemed(state5),
            BobState::Cancelled(state4) => Bob::BtcCancelled(state4),
            BobState::BtcRefunded(_)
            | BobState::XmrRedeemed
            | BobState::Punished
            | BobState::SafelyAborted => Bob::SwapComplete,
        }
    }
}

impl From<state::Bob> for BobState {
    fn from(bob: Bob) -> Self {
        match bob {
            Bob::Negotiated { state2, peer_id } => BobState::Negotiated(state2, peer_id),
            Bob::BtcLocked { state3, peer_id } => BobState::BtcLocked(state3, peer_id),
            Bob::XmrLocked { state4, peer_id } => BobState::XmrLocked(state4, peer_id),
            Bob::EncSigSent { state4, peer_id } => BobState::EncSigSent(state4, peer_id),
            Bob::BtcRedeemed(state5) => BobState::BtcRedeemed(state5),
            Bob::BtcCancelled(state4) => BobState::Cancelled(state4),
            Bob::SwapComplete => BobState::SafelyAborted,
        }
    }
}

pub async fn swap<R>(
    state: BobState,
    event_loop_handle: EventLoopHandle,
    db: Database,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
    rng: R,
    swap_id: Uuid,
) -> Result<BobState>
where
    R: RngCore + CryptoRng + Send,
{
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
                addr,
            } => {
                let (state2, alice_peer_id) = negotiate(
                    state0,
                    amounts,
                    &mut event_loop_handle,
                    addr,
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
                // todo: watch until t1, not indefinetely
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
                // Alice has locked Xmr
                // Bob sends Alice his key
                let tx_redeem_encsig = state.tx_redeem_encsig();
                // Do we have to wait for a response?
                // What if Alice fails to receive this? Should we always resend?
                // todo: If we cannot dial Alice we should go to EncSigSent. Maybe dialing
                // should happen in this arm?
                event_loop_handle
                    .send_message3(alice_peer_id.clone(), tx_redeem_encsig)
                    .await?;

                let state = BobState::EncSigSent(state, alice_peer_id);
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
            BobState::EncSigSent(state, ..) => {
                // Watch for redeem
                let redeem_watcher = state.watch_for_redeem_btc(bitcoin_wallet.as_ref());
                let t1_timeout = state.wait_for_t1(bitcoin_wallet.as_ref());

                // TODO(Franck): Check if db save and run_until can be factorized
                tokio::select! {
                    val = redeem_watcher => {
                        let state = BobState::BtcRedeemed(val?);
                        let db_state = state.clone().into();
                        db.insert_latest_state(swap_id, state::Swap::Bob(db_state)).await?;
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
                    _ = t1_timeout => {
                        // Check whether TxCancel has been published.
                        // We should not fail if the transaction is already on the blockchain
                        if state.check_for_tx_cancel(bitcoin_wallet.as_ref()).await.is_err() {
                            state.submit_tx_cancel(bitcoin_wallet.as_ref()).await?;
                        }

                        let state = BobState::Cancelled(state);
                        let db_state = state.clone().into();
                        db.insert_latest_state(swap_id, state::Swap::Bob(db_state)).await?;
                        run_until(
                            state,
                            is_target_state,
                            event_loop_handle,
                            db,
                            bitcoin_wallet,
                            monero_wallet,
                            rng,
                            swap_id
                        )
                        .await

                    }
                }
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
