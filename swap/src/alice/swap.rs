//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::{
    alice::{
        event_loop::EventLoopHandle,
        steps::{
            build_bitcoin_punish_transaction, build_bitcoin_redeem_transaction,
            extract_monero_private_key, lock_xmr, negotiate, publish_bitcoin_punish_transaction,
            publish_bitcoin_redeem_transaction, publish_cancel_transaction,
            wait_for_bitcoin_encrypted_signature, wait_for_bitcoin_refund, wait_for_locked_bitcoin,
        },
    },
    bitcoin,
    bitcoin::EncryptedSignature,
    network::request_response::AliceToBob,
    state,
    state::Alice,
    storage::Database,
    SwapAmounts,
};
use anyhow::{bail, Result};
use libp2p::request_response::ResponseChannel;
use rand::{CryptoRng, RngCore};
use std::{fmt, sync::Arc};
use tokio::select;
use tracing::info;
use uuid::Uuid;
use xmr_btc::{
    alice::{State0, State3},
    bitcoin::{TransactionBlockHeight, TxCancel, TxRefund, WatchForRawTransaction},
    config::Config,
    monero::CreateWalletForOutput,
    Epoch,
};

trait Rng: RngCore + CryptoRng + Send {}

impl<T> Rng for T where T: RngCore + CryptoRng + Send {}

#[allow(clippy::large_enum_variant)]
pub enum AliceState {
    Started {
        amounts: SwapAmounts,
        state0: State0,
    },
    Negotiated {
        channel: Option<ResponseChannel<AliceToBob>>,
        amounts: SwapAmounts,
        state3: State3,
    },
    BtcLocked {
        channel: Option<ResponseChannel<AliceToBob>>,
        amounts: SwapAmounts,
        state3: State3,
    },
    XmrLocked {
        state3: State3,
    },
    EncSignLearned {
        state3: State3,
        encrypted_signature: EncryptedSignature,
    },
    BtcRedeemed,
    BtcCancelled {
        state3: State3,
        tx_cancel: TxCancel,
    },
    BtcRefunded {
        spend_key: monero::PrivateKey,
        state3: State3,
    },
    BtcPunishable {
        tx_refund: TxRefund,
        state3: State3,
    },
    XmrRefunded,
    T1Expired {
        state3: State3,
    },
    Punished,
    SafelyAborted,
}

impl fmt::Display for AliceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AliceState::Started { .. } => write!(f, "started"),
            AliceState::Negotiated { .. } => write!(f, "negotiated"),
            AliceState::BtcLocked { .. } => write!(f, "btc_locked"),
            AliceState::XmrLocked { .. } => write!(f, "xmr_locked"),
            AliceState::EncSignLearned { .. } => write!(f, "encsig_learned"),
            AliceState::BtcRedeemed => write!(f, "btc_redeemed"),
            AliceState::BtcCancelled { .. } => write!(f, "btc_cancelled"),
            AliceState::BtcRefunded { .. } => write!(f, "btc_refunded"),
            AliceState::Punished => write!(f, "punished"),
            AliceState::SafelyAborted => write!(f, "safely_aborted"),
            AliceState::BtcPunishable { .. } => write!(f, "btc_punishable"),
            AliceState::XmrRefunded => write!(f, "xmr_refunded"),
            AliceState::T1Expired { .. } => write!(f, "t1 is expired"),
        }
    }
}

impl From<&AliceState> for state::Alice {
    fn from(alice_state: &AliceState) -> Self {
        match alice_state {
            AliceState::Negotiated { state3, .. } => Alice::Negotiated(state3.clone()),
            AliceState::BtcLocked { state3, .. } => Alice::BtcLocked(state3.clone()),
            AliceState::XmrLocked { state3 } => Alice::XmrLocked(state3.clone()),
            AliceState::EncSignLearned {
                state3,
                encrypted_signature,
            } => Alice::EncSignLearned {
                state: state3.clone(),
                encrypted_signature: encrypted_signature.clone(),
            },
            AliceState::BtcRedeemed => Alice::SwapComplete,
            AliceState::BtcCancelled { state3, .. } => Alice::BtcCancelled(state3.clone()),
            AliceState::BtcRefunded { .. } => Alice::SwapComplete,
            AliceState::BtcPunishable { state3, .. } => Alice::BtcPunishable(state3.clone()),
            AliceState::XmrRefunded => Alice::SwapComplete,
            AliceState::T1Expired { state3 } => Alice::T1Expired(state3.clone()),
            AliceState::Punished => Alice::SwapComplete,
            AliceState::SafelyAborted => Alice::SwapComplete,
            // TODO: Potentially add support to resume swaps that are not Negotiated
            AliceState::Started { .. } => {
                panic!("Alice attempted to save swap before being negotiated")
            }
        }
    }
}

impl From<state::Alice> for AliceState {
    fn from(db_state: state::Alice) -> Self {
        use AliceState::*;
        match db_state {
            Alice::Negotiated(state3) => Negotiated {
                channel: None,
                amounts: SwapAmounts {
                    btc: state3.btc,
                    xmr: state3.xmr,
                },
                state3,
            },
            Alice::BtcLocked(state3) => BtcLocked {
                channel: None,
                amounts: SwapAmounts {
                    btc: state3.btc,
                    xmr: state3.xmr,
                },
                state3,
            },
            Alice::XmrLocked(state3) => XmrLocked { state3 },
            Alice::BtcRedeemable { .. } => panic!("BtcRedeemable state is unexpected"),
            Alice::EncSignLearned {
                state,
                encrypted_signature,
            } => EncSignLearned {
                state3: state,
                encrypted_signature,
            },
            Alice::T1Expired(state3) => AliceState::T1Expired { state3 },
            Alice::BtcCancelled(state) => {
                let tx_cancel = bitcoin::TxCancel::new(
                    &state.tx_lock,
                    state.refund_timelock,
                    state.a.public(),
                    state.B,
                );

                BtcCancelled {
                    state3: state,
                    tx_cancel,
                }
            }
            Alice::BtcPunishable(state) => {
                let tx_cancel = bitcoin::TxCancel::new(
                    &state.tx_lock,
                    state.refund_timelock,
                    state.a.public(),
                    state.B,
                );
                let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &state.refund_address);
                BtcPunishable {
                    tx_refund,
                    state3: state,
                }
            }
            Alice::BtcRefunded {
                state, spend_key, ..
            } => BtcRefunded {
                spend_key,
                state3: state,
            },
            Alice::SwapComplete => {
                // TODO(Franck): Better fine grain
                AliceState::SafelyAborted
            }
        }
    }
}

pub fn is_complete(state: &AliceState) -> bool {
    matches!(
        state,
        AliceState::XmrRefunded
            | AliceState::BtcRedeemed
            | AliceState::Punished
            | AliceState::SafelyAborted
    )
}

pub struct Swap {
    event_loop_handle: EventLoopHandle,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
    config: Config,
    swap_id: Uuid,
    db: Database,
}

impl Swap {
    pub fn new(
        event_loop_handle: EventLoopHandle,
        bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
        monero_wallet: Arc<crate::monero::Wallet>,
        config: Config,
        swap_id: Uuid,
        db: Database,
    ) -> Self {
        Self {
            event_loop_handle,
            bitcoin_wallet,
            monero_wallet,
            config,
            swap_id,
            db,
        }
    }

    pub async fn swap(self, state: AliceState) -> Result<AliceState> {
        self.run_until(state, is_complete).await
    }

    pub async fn resume_from_database(self) -> Result<AliceState> {
        if let state::Swap::Alice(db_state) = self.db.get_state(self.swap_id)? {
            self.swap(db_state.into()).await
        } else {
            bail!("Alice state expected.")
        }
    }

    // State machine driver for swap execution
    pub async fn run_until(
        mut self,
        mut state: AliceState,
        is_target_state: fn(&AliceState) -> bool,
    ) -> Result<AliceState> {
        loop {
            info!("Current state:{}", state);
            if is_target_state(&state) {
                break Ok(state);
            } else {
                match state {
                    AliceState::Started { amounts, state0 } => {
                        let (channel, state3) =
                            negotiate(state0, amounts, &mut self.event_loop_handle, self.config)
                                .await?;
                        state = AliceState::Negotiated {
                            channel: Some(channel),
                            amounts,
                            state3,
                        };

                        self.db
                            .insert_latest_state(self.swap_id, state::Swap::Alice((&state).into()))
                            .await?;
                    }
                    AliceState::Negotiated {
                        state3,
                        channel,
                        amounts,
                    } => {
                        state = match channel {
                            Some(channel) => {
                                let _ = wait_for_locked_bitcoin(
                                    state3.tx_lock.txid(),
                                    self.bitcoin_wallet.clone(),
                                    self.config,
                                )
                                .await?;

                                AliceState::BtcLocked {
                                    channel: Some(channel),
                                    amounts,
                                    state3,
                                }
                            }
                            None => {
                                tracing::info!(
                                    "Cannot resume swap from negotiated state, aborting"
                                );

                                // Alice did not lock Xmr yet
                                AliceState::SafelyAborted
                            }
                        };

                        self.db
                            .insert_latest_state(self.swap_id, state::Swap::Alice((&state).into()))
                            .await?;
                    }
                    AliceState::BtcLocked {
                        channel,
                        amounts,
                        state3,
                    } => {
                        state = match channel {
                            Some(channel) => {
                                lock_xmr(
                                    channel,
                                    amounts,
                                    state3.clone(),
                                    &mut self.event_loop_handle,
                                    self.monero_wallet.clone(),
                                )
                                .await?;

                                AliceState::XmrLocked { state3 }
                            }
                            None => {
                                tracing::info!(
                                    "Cannot resume swap from BTC locked state, aborting"
                                );

                                // Alice did not lock Xmr yet
                                AliceState::SafelyAborted
                            }
                        };

                        self.db
                            .insert_latest_state(self.swap_id, state::Swap::Alice((&state).into()))
                            .await?;
                    }
                    AliceState::XmrLocked { state3 } => {
                        state = match state3.current_epoch(self.bitcoin_wallet.as_ref()).await? {
                            Epoch::T0 => {
                                let wait_for_enc_sig = wait_for_bitcoin_encrypted_signature(
                                    &mut self.event_loop_handle,
                                    self.config.monero_max_finality_time,
                                );
                                let state3_clone = state3.clone();
                                let t1_timeout =
                                    state3_clone.wait_for_t1(self.bitcoin_wallet.as_ref());

                                select! {
                                    enc_sig = wait_for_enc_sig => {
                                        AliceState::EncSignLearned {
                                            state3,
                                            encrypted_signature: enc_sig?,
                                        }
                                    },
                                    _t1_expired = t1_timeout => {
                                        AliceState::T1Expired { state3 }
                                    }
                                }
                            }
                            _ => {
                                AliceState::T1Expired { state3 }
                            }
                        };

                        self.db
                            .insert_latest_state(self.swap_id, state::Swap::Alice((&state).into()))
                            .await?;
                    }
                    AliceState::EncSignLearned {
                        state3,
                        encrypted_signature,
                    } => {
                        let signed_tx_redeem = match build_bitcoin_redeem_transaction(
                            encrypted_signature,
                            &state3.tx_lock,
                            state3.a.clone(),
                            state3.s_a,
                            state3.B,
                            &state3.redeem_address,
                        ) {
                            Ok(tx) => tx,
                            Err(_) => {
                                state3.wait_for_t1(self.bitcoin_wallet.as_ref()).await?;

                                state = AliceState::T1Expired { state3 };
                                self.db
                                    .insert_latest_state(
                                        self.swap_id,
                                        state::Swap::Alice((&state).into()),
                                    )
                                    .await?;
                                continue;
                            }
                        };

                        // TODO(Franck): Error handling is delicate here.
                        // If Bob sees this transaction he can redeem Monero
                        // e.g. If the Bitcoin node is down then the user needs to take action.
                        publish_bitcoin_redeem_transaction(
                            signed_tx_redeem,
                            self.bitcoin_wallet.clone(),
                            self.config,
                        )
                        .await?;

                        state = AliceState::BtcRedeemed;

                        self.db
                            .insert_latest_state(self.swap_id, state::Swap::Alice((&state).into()))
                            .await?;
                    }
                    AliceState::T1Expired { state3 } => {
                        let tx_cancel = publish_cancel_transaction(
                            state3.tx_lock.clone(),
                            state3.a.clone(),
                            state3.B,
                            state3.refund_timelock,
                            state3.tx_cancel_sig_bob.clone(),
                            self.bitcoin_wallet.clone(),
                        )
                        .await?;

                        state = AliceState::BtcCancelled { state3, tx_cancel };
                        self.db
                            .insert_latest_state(self.swap_id, state::Swap::Alice((&state).into()))
                            .await?;
                    }
                    AliceState::BtcCancelled { state3, tx_cancel } => {
                        let tx_cancel_height = self
                            .bitcoin_wallet
                            .transaction_block_height(tx_cancel.txid())
                            .await;

                        let (tx_refund, published_refund_tx) = wait_for_bitcoin_refund(
                            &tx_cancel,
                            tx_cancel_height,
                            state3.punish_timelock,
                            &state3.refund_address,
                            self.bitcoin_wallet.clone(),
                        )
                        .await?;

                        // TODO(Franck): Review error handling
                        state = match published_refund_tx {
                            None => AliceState::BtcPunishable { tx_refund, state3 },
                            Some(published_refund_tx) => {
                                let spend_key = extract_monero_private_key(
                                    published_refund_tx,
                                    tx_refund,
                                    state3.s_a,
                                    state3.a.clone(),
                                    state3.S_b_bitcoin,
                                )?;

                                AliceState::BtcRefunded { spend_key, state3 }
                            }
                        };

                        self.db
                            .insert_latest_state(self.swap_id, state::Swap::Alice((&state).into()))
                            .await?;
                    }
                    AliceState::BtcRefunded { spend_key, state3 } => {
                        let view_key = state3.v;

                        self.monero_wallet
                            .create_and_load_wallet_for_output(spend_key, view_key)
                            .await?;

                        state = AliceState::XmrRefunded;
                        self.db
                            .insert_latest_state(self.swap_id, state::Swap::Alice((&state).into()))
                            .await?;
                    }
                    AliceState::BtcPunishable { tx_refund, state3 } => {
                        let signed_tx_punish = build_bitcoin_punish_transaction(
                            &state3.tx_lock,
                            state3.refund_timelock,
                            &state3.punish_address,
                            state3.punish_timelock,
                            state3.tx_punish_sig_bob.clone(),
                            state3.a.clone(),
                            state3.B,
                        )?;

                        let punish_tx_finalised = publish_bitcoin_punish_transaction(
                            signed_tx_punish,
                            self.bitcoin_wallet.clone(),
                            self.config,
                        );

                        let bitcoin_wallet_clone = self.bitcoin_wallet.clone();

                        let refund_tx_seen =
                            bitcoin_wallet_clone.watch_for_raw_transaction(tx_refund.txid());


                        state = select! {
                            _ = punish_tx_finalised => {
                                AliceState::Punished
                            },
                            published_refund_tx = refund_tx_seen => {
                                let spend_key = extract_monero_private_key(
                                published_refund_tx,
                                tx_refund,
                                state3.s_a,
                                state3.a.clone(),
                                state3.S_b_bitcoin)?;
                                AliceState::BtcRefunded { spend_key, state3 }
                            }
                        };

                        self.db
                            .insert_latest_state(self.swap_id, state::Swap::Alice((&state).into()))
                            .await?;
                    }
                    AliceState::XmrRefunded => {
                        break Ok(AliceState::XmrRefunded);
                    }
                    AliceState::BtcRedeemed => {
                        break Ok(AliceState::BtcRedeemed);
                    }
                    AliceState::Punished => {
                        break Ok(AliceState::Punished);
                    }
                    AliceState::SafelyAborted => {
                        break Ok(AliceState::SafelyAborted);
                    }
                }
            }
        }
    }
}
