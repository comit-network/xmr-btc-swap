use crate::{bob::event_loop::EventLoopHandle, state, state::Bob, storage::Database, SwapAmounts};
use anyhow::{bail, Result};
use libp2p::{core::Multiaddr, PeerId};
use std::{fmt, sync::Arc};
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
        addr: Multiaddr,
    },
    Negotiated(bob::State2, PeerId),
    BtcLocked(bob::State3, PeerId),
    XmrLocked(bob::State4, PeerId),
    EncSigSent(bob::State4, PeerId),
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
            BobState::EncSigSent(state4, peer_id) => Bob::EncSigSent { state4, peer_id },
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

impl From<state::Bob> for BobState {
    fn from(db_state: state::Bob) -> Self {
        match db_state {
            Bob::Negotiated { state2, peer_id } => BobState::Negotiated(state2, peer_id),
            Bob::BtcLocked { state3, peer_id } => BobState::BtcLocked(state3, peer_id),
            Bob::XmrLocked { state4, peer_id } => BobState::XmrLocked(state4, peer_id),
            Bob::EncSigSent { state4, peer_id } => BobState::EncSigSent(state4, peer_id),
            Bob::BtcRedeemed(state5) => BobState::BtcRedeemed(state5),
            Bob::T1Expired(state4) => BobState::T1Expired(state4),
            Bob::BtcCancelled(state4) => BobState::Cancelled(state4),
            Bob::SwapComplete => BobState::SafelyAborted,
        }
    }
}

pub struct Swap {
    event_loop_handle: EventLoopHandle,
    db: Database,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
    swap_id: Uuid,
}

impl Swap {
    pub fn new(
        event_loop_handle: EventLoopHandle,
        db: Database,
        bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
        monero_wallet: Arc<crate::monero::Wallet>,
        swap_id: Uuid,
    ) -> Self {
        Self {
            event_loop_handle,
            db,
            bitcoin_wallet,
            monero_wallet,
            swap_id,
        }
    }

    pub async fn swap(self, state: BobState) -> Result<BobState> {
        self.run_until(state, is_complete).await
    }

    pub async fn resume_from_database(self) -> Result<BobState> {
        if let state::Swap::Bob(db_state) = self.db.get_state(self.swap_id)? {
            self.swap(db_state.into()).await
        } else {
            bail!("Bob state expected.")
        }
    }

    // State machine driver for swap execution
    pub async fn run_until(
        mut self,
        mut state: BobState,
        is_target_state: fn(&BobState) -> bool,
    ) -> Result<BobState> {
        loop {
            info!("Current state: {}", state);
            if is_target_state(&state) {
                break Ok(state);
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
                            &mut self.event_loop_handle,
                            addr,
                            self.bitcoin_wallet.clone(),
                        )
                        .await?;

                        state = BobState::Negotiated(state2, alice_peer_id);
                        self.db
                            .insert_latest_state(
                                self.swap_id,
                                state::Swap::Bob(state.clone().into()),
                            )
                            .await?;
                    }
                    BobState::Negotiated(state2, alice_peer_id) => {
                        // Alice and Bob have exchanged info
                        let state3 = state2.lock_btc(self.bitcoin_wallet.as_ref()).await?;

                        state = BobState::BtcLocked(state3, alice_peer_id);
                        self.db
                            .insert_latest_state(
                                self.swap_id,
                                state::Swap::Bob(state.clone().into()),
                            )
                            .await?;
                    }
                    // Bob has locked Btc
                    // Watch for Alice to Lock Xmr or for t1 to elapse
                    BobState::BtcLocked(state3, alice_peer_id) => {
                        // todo: watch until t1, not indefinetely
                        let msg2 = self.event_loop_handle.recv_message2().await?;
                        let state4 = state3
                            .watch_for_lock_xmr(self.monero_wallet.as_ref(), msg2)
                            .await?;

                        state = BobState::XmrLocked(state4, alice_peer_id);
                        self.db
                            .insert_latest_state(
                                self.swap_id,
                                state::Swap::Bob(state.clone().into()),
                            )
                            .await?;
                    }
                    BobState::XmrLocked(state4, alice_peer_id) => {
                        state = if let Epoch::T0 =
                            state4.current_epoch(self.bitcoin_wallet.as_ref()).await?
                        {
                            // Alice has locked Xmr
                            // Bob sends Alice his key
                            let tx_redeem_encsig = state4.tx_redeem_encsig();

                            let state4_clone = state4.clone();
                            let enc_sig_sent_watcher = self
                                .event_loop_handle
                                .send_message3(alice_peer_id.clone(), tx_redeem_encsig);
                            let bitcoin_wallet = self.bitcoin_wallet.clone();
                            let t1_timeout = state4_clone.wait_for_t1(bitcoin_wallet.as_ref());

                            select! {
                                _ = enc_sig_sent_watcher => {
                                    BobState::EncSigSent(state4, alice_peer_id)
                                },
                                _ = t1_timeout => {
                                    BobState::T1Expired(state4)
                                }
                            }
                        } else {
                            BobState::T1Expired(state4)
                        };

                        self.db
                            .insert_latest_state(
                                self.swap_id,
                                state::Swap::Bob(state.clone().into()),
                            )
                            .await?;
                    }
                    BobState::EncSigSent(state4, ..) => {
                        state = if let Epoch::T0 =
                            state4.current_epoch(self.bitcoin_wallet.as_ref()).await?
                        {
                            let state4_clone = state4.clone();
                            let bitcoin_wallet = self.bitcoin_wallet.clone();
                            let redeem_watcher =
                                state4_clone.watch_for_redeem_btc(bitcoin_wallet.as_ref());

                            let bitcoin_wallet = self.bitcoin_wallet.clone();
                            let t1_timeout = state4_clone.wait_for_t1(bitcoin_wallet.as_ref());

                            select! {
                                state5 = redeem_watcher=> {
                                    BobState::BtcRedeemed(state5?)
                                },
                                _t1_expired = t1_timeout => {
                                    BobState::T1Expired(state4)
                                }
                            }
                        } else {
                            BobState::T1Expired(state4)
                        };

                        self.db
                            .insert_latest_state(
                                self.swap_id,
                                state::Swap::Bob(state.clone().into()),
                            )
                            .await?;
                    }
                    BobState::BtcRedeemed(state5) => {
                        // Bob redeems XMR using revealed s_a
                        state5.claim_xmr(self.monero_wallet.as_ref()).await?;

                        state = BobState::XmrRedeemed;
                        self.db
                            .insert_latest_state(
                                self.swap_id,
                                state::Swap::Bob(state.clone().into()),
                            )
                            .await?;
                    }
                    BobState::T1Expired(state4) => {
                        if state4
                            .check_for_tx_cancel(self.bitcoin_wallet.as_ref())
                            .await
                            .is_err()
                        {
                            state4
                                .submit_tx_cancel(self.bitcoin_wallet.as_ref())
                                .await?;
                        }

                        state = BobState::Cancelled(state4);
                        self.db
                            .insert_latest_state(
                                self.swap_id,
                                state::Swap::Bob(state.clone().into()),
                            )
                            .await?;
                    }
                    BobState::Cancelled(state4) => {
                        // TODO
                        // Bob has cancelled the swap
                        state = match state4.current_epoch(self.bitcoin_wallet.as_ref()).await? {
                            Epoch::T0 => panic!("Cancelled before t1??? Something is really wrong"),
                            Epoch::T1 => {
                                state4.refund_btc(self.bitcoin_wallet.as_ref()).await?;
                                BobState::BtcRefunded(state4)
                            }
                            Epoch::T2 => BobState::Punished,
                        };

                        self.db
                            .insert_latest_state(
                                self.swap_id,
                                state::Swap::Bob(state.clone().into()),
                            )
                            .await?;
                    }
                    BobState::BtcRefunded(state4) => {
                        break Ok(BobState::BtcRefunded(state4));
                    }
                    BobState::Punished => {
                        break Ok(BobState::Punished);
                    }
                    BobState::SafelyAborted => {
                        break Ok(BobState::SafelyAborted);
                    }
                    BobState::XmrRedeemed => {
                        break Ok(BobState::XmrRedeemed);
                    }
                }
            }
        }
    }
}

pub async fn negotiate(
    state0: xmr_btc::bob::State0,
    amounts: SwapAmounts,
    swarm: &mut EventLoopHandle,
    addr: Multiaddr,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
) -> Result<(State2, PeerId)> {
    tracing::trace!("Starting negotiate");
    swarm.dial_alice(addr).await?;

    let alice_peer_id = swarm.recv_conn_established().await?;

    swarm
        .request_amounts(alice_peer_id.clone(), amounts.btc)
        .await?;

    swarm
        .send_message0(alice_peer_id.clone(), state0.next_message())
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

pub fn is_complete(state: &BobState) -> bool {
    matches!(
        state,
        BobState::BtcRefunded(..)
            | BobState::XmrRedeemed
            | BobState::Punished
            | BobState::SafelyAborted
    )
}
