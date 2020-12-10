use crate::{
    bob::{event_loop::EventLoopHandle, execution::negotiate},
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
use xmr_btc::bob::{self};

// The same data structure is used for swap execution and recovery.
// This allows for a seamless transition from a failed swap to recovery.
pub enum BobState {
    Started {
        state0: bob::State0,
        amounts: SwapAmounts,
        peer_id: PeerId,
        addr: Multiaddr,
    },
    Negotiated(bob::State2, PeerId),
    BtcLocked(bob::State3, PeerId),
    XmrLocked(bob::State4, PeerId),
    EncSigSent(bob::State4, PeerId),
    BtcRedeemed(bob::State5),
    Cancelled(bob::State4),
    BtcRefunded,
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
            BobState::BtcRedeemed(_) => write!(f, "btc_redeemed"),
            BobState::Cancelled(_) => write!(f, "cancelled"),
            BobState::BtcRefunded => write!(f, "btc_refunded"),
            BobState::XmrRedeemed => write!(f, "xmr_redeemed"),
            BobState::Punished => write!(f, "punished"),
            BobState::SafelyAborted => write!(f, "safely_aborted"),
        }
    }
}

pub async fn swap<R>(
    state: BobState,
    swarm: EventLoopHandle,
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
        swarm,
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
        BobState::BtcRefunded
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
    mut swarm: EventLoopHandle,
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
                peer_id,
                addr,
            } => {
                let state2 = negotiate(
                    state0,
                    amounts,
                    &mut swarm,
                    addr,
                    &mut rng,
                    bitcoin_wallet.clone(),
                )
                .await?;
                run_until(
                    BobState::Negotiated(state2, peer_id),
                    is_target_state,
                    swarm,
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
                // db.insert_latest_state(state);
                run_until(
                    BobState::BtcLocked(state3, alice_peer_id),
                    is_target_state,
                    swarm,
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
                let msg2 = swarm.recv_message2().await?;
                let state4 = state3
                    .watch_for_lock_xmr(monero_wallet.as_ref(), msg2)
                    .await?;

                run_until(
                    BobState::XmrLocked(state4, alice_peer_id),
                    is_target_state,
                    swarm,
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
                swarm
                    .send_message3(alice_peer_id.clone(), tx_redeem_encsig)
                    .await?;

                run_until(
                    BobState::EncSigSent(state, alice_peer_id),
                    is_target_state,
                    swarm,
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

                tokio::select! {
                    val = redeem_watcher => {
                        run_until(
                            BobState::BtcRedeemed(val?),
                                 is_target_state,
                            swarm,
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

                        run_until(
                            BobState::Cancelled(state),
                                 is_target_state,
                            swarm,
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
                run_until(
                    BobState::XmrRedeemed,
                    is_target_state,
                    swarm,
                    db,
                    bitcoin_wallet,
                    monero_wallet,
                    rng,
                    swap_id,
                )
                .await
            }
            BobState::Cancelled(_state) => {
                // Bob has cancelled the swap
                // If <t2 Bob refunds
                // if unimplemented!("<t2") {
                //     // Submit TxRefund
                //     abort(BobState::BtcRefunded, io).await
                // } else {
                //     // Bob failed to refund in time and has been punished
                //     abort(BobState::Punished, io).await
                // }
                Ok(BobState::BtcRefunded)
            }
            BobState::BtcRefunded => Ok(BobState::BtcRefunded),
            BobState::Punished => Ok(BobState::Punished),
            BobState::SafelyAborted => Ok(BobState::SafelyAborted),
            BobState::XmrRedeemed => Ok(BobState::XmrRedeemed),
        }
    }
}
