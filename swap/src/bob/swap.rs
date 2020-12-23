use crate::{
    bob::event_loop::EventLoopHandle,
    database::{state, Database},
    SwapAmounts,
};
use anyhow::{bail, Result};
use async_recursion::async_recursion;
use rand::{CryptoRng, RngCore};
use std::{fmt, sync::Arc};
use tokio::select;
use tracing::info;
use uuid::Uuid;
use xmr_btc::{
    bob::{self, State2},
    ExpiredTimelocks,
};

#[derive(Debug, Clone)]
pub enum BobState {
    Started {
        state0: bob::State0,
        amounts: SwapAmounts,
    },
    Negotiated(bob::State2),
    BtcLocked(bob::State3),
    XmrLocked(bob::State4),
    EncSigSent(bob::State4),
    BtcRedeemed(bob::State5),
    CancelTimelockExpired(bob::State4),
    BtcCancelled(bob::State4),
    BtcRefunded(bob::State4),
    XmrRedeemed,
    BtcPunished,
    SafelyAborted,
}

impl fmt::Display for BobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BobState::Started { .. } => write!(f, "started"),
            BobState::Negotiated(..) => write!(f, "negotiated"),
            BobState::BtcLocked(..) => write!(f, "btc is locked"),
            BobState::XmrLocked(..) => write!(f, "xmr is locked"),
            BobState::EncSigSent(..) => write!(f, "encrypted signature is sent"),
            BobState::BtcRedeemed(..) => write!(f, "btc is redeemed"),
            BobState::CancelTimelockExpired(..) => write!(f, "cancel timelock is expired"),
            BobState::BtcCancelled(..) => write!(f, "btc is cancelled"),
            BobState::BtcRefunded(..) => write!(f, "btc is refunded"),
            BobState::XmrRedeemed => write!(f, "xmr is redeemed"),
            BobState::BtcPunished => write!(f, "btc is punished"),
            BobState::SafelyAborted => write!(f, "safely aborted"),
        }
    }
}

// TODO(Franck): Make this a method on a struct
#[allow(clippy::too_many_arguments)]
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
            | BobState::BtcPunished
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
            BobState::Started { state0, amounts } => {
                event_loop_handle.dial().await?;

                let state2 = negotiate(
                    state0,
                    amounts,
                    &mut event_loop_handle,
                    &mut rng,
                    bitcoin_wallet.clone(),
                )
                .await?;

                let state = BobState::Negotiated(state2);
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
            BobState::Negotiated(state2) => {
                // Do not lock Bitcoin if not connected to Alice.
                event_loop_handle.dial().await?;
                // Alice and Bob have exchanged info
                let state3 = state2.lock_btc(bitcoin_wallet.as_ref()).await?;

                let state = BobState::BtcLocked(state3);
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
            // Watch for Alice to Lock Xmr or for cancel timelock to elapse
            BobState::BtcLocked(state3) => {
                let state = if let ExpiredTimelocks::None =
                    state3.current_epoch(bitcoin_wallet.as_ref()).await?
                {
                    event_loop_handle.dial().await?;

                    let msg2_watcher = event_loop_handle.recv_message2();
                    let cancel_timelock_expires =
                        state3.wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref());

                    select! {
                        msg2 = msg2_watcher => {

                            let xmr_lock_watcher = state3.clone()
                                .watch_for_lock_xmr(monero_wallet.as_ref(), msg2?);
                            let cancel_timelock_expires = state3.wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref());

                            select! {
                                state4 = xmr_lock_watcher => {
                                    BobState::XmrLocked(state4?)
                                },
                                _ = cancel_timelock_expires => {
                                    let state4 = state3.state4();
                                    BobState::CancelTimelockExpired(state4)
                                }
                            }

                        },
                        _ = cancel_timelock_expires => {
                            let state4 = state3.state4();
                            BobState::CancelTimelockExpired(state4)
                        }
                    }
                } else {
                    let state4 = state3.state4();
                    BobState::CancelTimelockExpired(state4)
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
            BobState::XmrLocked(state) => {
                let state = if let ExpiredTimelocks::None =
                    state.expired_timelock(bitcoin_wallet.as_ref()).await?
                {
                    event_loop_handle.dial().await?;
                    // Alice has locked Xmr
                    // Bob sends Alice his key
                    let tx_redeem_encsig = state.tx_redeem_encsig();

                    let state4_clone = state.clone();
                    // TODO(Franck): Refund if message cannot be sent.
                    let enc_sig_sent_watcher = event_loop_handle.send_message3(tx_redeem_encsig);
                    let bitcoin_wallet = bitcoin_wallet.clone();
                    let cancel_timelock_expires =
                        state4_clone.wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref());

                    select! {
                        _ = enc_sig_sent_watcher => {
                            BobState::EncSigSent(state)
                        },
                        _ = cancel_timelock_expires => {
                            BobState::CancelTimelockExpired(state)
                        }
                    }
                } else {
                    BobState::CancelTimelockExpired(state)
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
                let state = if let ExpiredTimelocks::None =
                    state.expired_timelock(bitcoin_wallet.as_ref()).await?
                {
                    let state_clone = state.clone();
                    let redeem_watcher = state_clone.watch_for_redeem_btc(bitcoin_wallet.as_ref());
                    let cancel_timelock_expires =
                        state_clone.wait_for_cancel_timelock_to_expire(bitcoin_wallet.as_ref());

                    select! {
                        state5 = redeem_watcher => {
                            BobState::BtcRedeemed(state5?)
                        },
                        _ = cancel_timelock_expires => {
                            BobState::CancelTimelockExpired(state)
                        }
                    }
                } else {
                    BobState::CancelTimelockExpired(state)
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
            BobState::CancelTimelockExpired(state4) => {
                if state4
                    .check_for_tx_cancel(bitcoin_wallet.as_ref())
                    .await
                    .is_err()
                {
                    state4.submit_tx_cancel(bitcoin_wallet.as_ref()).await?;
                }

                let state = BobState::BtcCancelled(state4);
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
            BobState::BtcCancelled(state) => {
                // Bob has cancelled the swap
                let state = match state.expired_timelock(bitcoin_wallet.as_ref()).await? {
                    ExpiredTimelocks::None => {
                        bail!("Internal error: canceled state reached before cancel timelock was expired");
                    }
                    ExpiredTimelocks::Cancel => {
                        state.refund_btc(bitcoin_wallet.as_ref()).await?;
                        BobState::BtcRefunded(state)
                    }
                    ExpiredTimelocks::Punish => BobState::BtcPunished,
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
            BobState::BtcPunished => Ok(BobState::BtcPunished),
            BobState::SafelyAborted => Ok(BobState::SafelyAborted),
            BobState::XmrRedeemed => Ok(BobState::XmrRedeemed),
        }
    }
}

pub async fn negotiate<R>(
    state0: xmr_btc::bob::State0,
    amounts: SwapAmounts,
    swarm: &mut EventLoopHandle,
    mut rng: R,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
) -> Result<State2>
where
    R: RngCore + CryptoRng + Send,
{
    tracing::trace!("Starting negotiate");
    swarm.request_amounts(amounts.btc).await?;

    swarm.send_message0(state0.next_message(&mut rng)).await?;
    let msg0 = swarm.recv_message0().await?;
    let state1 = state0.receive(bitcoin_wallet.as_ref(), msg0).await?;

    swarm.send_message1(state1.next_message()).await?;
    let msg1 = swarm.recv_message1().await?;
    let state2 = state1.receive(msg1)?;

    swarm.send_message2(state2.next_message()).await?;

    Ok(state2)
}
