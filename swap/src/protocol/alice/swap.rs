//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use anyhow::Result;
use async_recursion::async_recursion;
use futures::{
    future::{select, Either},
    pin_mut,
};
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::{
    bitcoin,
    bitcoin::TransactionBlockHeight,
    config::Config,
    database::{Database, Swap},
    monero,
    monero::CreateWalletForOutput,
    protocol::alice::{
        event_loop::EventLoopHandle,
        steps::{
            build_bitcoin_punish_transaction, build_bitcoin_redeem_transaction,
            extract_monero_private_key, lock_xmr, negotiate, publish_bitcoin_punish_transaction,
            publish_bitcoin_redeem_transaction, publish_cancel_transaction,
            wait_for_bitcoin_encrypted_signature, wait_for_bitcoin_refund, wait_for_locked_bitcoin,
            watch_for_tx_refund,
        },
        AliceState,
    },
    ExpiredTimelocks,
};

pub struct AliceActor {
    event_loop_handle: EventLoopHandle,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    db: Database,
    config: Config,
    swap_id: Uuid,
}

impl AliceActor {
    pub fn new(
        event_loop_handle: EventLoopHandle,
        bitcoin_wallet: Arc<bitcoin::Wallet>,
        monero_wallet: Arc<monero::Wallet>,
        db: Database,
        config: Config,
        swap_id: Uuid,
    ) -> Self {
        Self {
            event_loop_handle,
            bitcoin_wallet,
            monero_wallet,
            db,
            config,
            swap_id,
        }
    }

    // TODO: Make a swap abstraction that contains the state and swap id
    pub async fn swap(self, start_state: AliceState) -> Result<AliceState> {
        self.run_until(start_state, is_complete).await
    }

    // State machine driver for swap execution
    #[async_recursion]
    pub async fn run_until(
        mut self,
        start_state: AliceState,
        is_target_state: fn(&AliceState) -> bool,
    ) -> Result<AliceState> {
        info!("Current state:{}", start_state);
        if is_target_state(&start_state) {
            Ok(start_state)
        } else {
            match start_state {
                AliceState::Started { amounts, state0 } => {
                    let (channel, state3) =
                        negotiate(state0, amounts, &mut self.event_loop_handle, self.config)
                            .await?;

                    let state = AliceState::Negotiated {
                        channel: Some(channel),
                        amounts,
                        state3: Box::new(state3),
                    };

                    let db_state = (&state).into();
                    self.db
                        .insert_latest_state(self.swap_id, Swap::Alice(db_state))
                        .await?;
                    self.run_until(state, is_target_state).await
                }
                AliceState::Negotiated {
                    state3,
                    channel,
                    amounts,
                } => {
                    let state = match channel {
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
                            tracing::info!("Cannot resume swap from negotiated state, aborting");

                            // Alice did not lock Xmr yet
                            AliceState::SafelyAborted
                        }
                    };

                    let db_state = (&state).into();
                    self.db
                        .insert_latest_state(self.swap_id, Swap::Alice(db_state))
                        .await?;
                    self.run_until(state, is_target_state).await
                }
                AliceState::BtcLocked {
                    channel,
                    amounts,
                    state3,
                } => {
                    let state = match channel {
                        Some(channel) => {
                            lock_xmr(
                                channel,
                                amounts,
                                *state3.clone(),
                                &mut self.event_loop_handle,
                                self.monero_wallet.clone(),
                            )
                            .await?;

                            AliceState::XmrLocked { state3 }
                        }
                        None => {
                            tracing::info!("Cannot resume swap from BTC locked state, aborting");

                            // Alice did not lock Xmr yet
                            AliceState::SafelyAborted
                        }
                    };

                    let db_state = (&state).into();
                    self.db
                        .insert_latest_state(self.swap_id, Swap::Alice(db_state))
                        .await?;
                    self.run_until(state, is_target_state).await
                }
                AliceState::XmrLocked { state3 } => {
                    // todo: match statement and wait for cancel timelock to expire can probably be
                    // expressed more cleanly
                    let state = match state3
                        .expired_timelocks(self.bitcoin_wallet.as_ref())
                        .await?
                    {
                        ExpiredTimelocks::None => {
                            let wait_for_enc_sig = wait_for_bitcoin_encrypted_signature(
                                &mut self.event_loop_handle,
                                self.config.monero_max_finality_time,
                            );
                            let state3_clone = state3.clone();
                            let cancel_timelock_expires = state3_clone
                                .wait_for_cancel_timelock_to_expire(self.bitcoin_wallet.as_ref());

                            pin_mut!(wait_for_enc_sig);
                            pin_mut!(cancel_timelock_expires);

                            match select(cancel_timelock_expires, wait_for_enc_sig).await {
                                Either::Left(_) => AliceState::CancelTimelockExpired { state3 },
                                Either::Right((enc_sig, _)) => AliceState::EncSigLearned {
                                    state3,
                                    encrypted_signature: enc_sig?,
                                },
                            }
                        }
                        _ => AliceState::CancelTimelockExpired { state3 },
                    };

                    let db_state = (&state).into();
                    self.db
                        .insert_latest_state(self.swap_id, Swap::Alice(db_state))
                        .await?;
                    self.run_until(state, is_target_state).await
                }
                AliceState::EncSigLearned {
                    state3,
                    encrypted_signature,
                } => {
                    // TODO: Evaluate if it is correct for Alice to Redeem no matter what.
                    //  If cancel timelock expired she should potentially not try redeem. (The
                    // implementation  gives her an advantage.)

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
                            state3
                                .wait_for_cancel_timelock_to_expire(self.bitcoin_wallet.as_ref())
                                .await?;

                            let state = AliceState::CancelTimelockExpired { state3 };
                            let db_state = (&state).into();
                            self.db
                                .insert_latest_state(self.swap_id, Swap::Alice(db_state))
                                .await?;
                            return self.run_until(state, is_target_state).await;
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

                    let state = AliceState::BtcRedeemed;
                    let db_state = (&state).into();
                    self.db
                        .insert_latest_state(self.swap_id, Swap::Alice(db_state))
                        .await?;
                    self.run_until(state, is_target_state).await
                }
                AliceState::CancelTimelockExpired { state3 } => {
                    let tx_cancel = publish_cancel_transaction(
                        state3.tx_lock.clone(),
                        state3.a.clone(),
                        state3.B,
                        state3.cancel_timelock,
                        state3.tx_cancel_sig_bob.clone(),
                        self.bitcoin_wallet.clone(),
                    )
                    .await?;

                    let state = AliceState::BtcCancelled { state3, tx_cancel };
                    let db_state = (&state).into();
                    self.db
                        .insert_latest_state(self.swap_id, Swap::Alice(db_state))
                        .await?;
                    self.run_until(state, is_target_state).await
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
                    match published_refund_tx {
                        None => {
                            let state = AliceState::BtcPunishable { tx_refund, state3 };
                            let db_state = (&state).into();
                            self.db
                                .insert_latest_state(self.swap_id, Swap::Alice(db_state))
                                .await?;
                            self.swap(state).await
                        }
                        Some(published_refund_tx) => {
                            let spend_key = extract_monero_private_key(
                                published_refund_tx,
                                tx_refund,
                                state3.s_a,
                                state3.a.clone(),
                                state3.S_b_bitcoin,
                            )?;

                            let state = AliceState::BtcRefunded { spend_key, state3 };
                            let db_state = (&state).into();
                            self.db
                                .insert_latest_state(self.swap_id, Swap::Alice(db_state))
                                .await?;
                            self.run_until(state, is_target_state).await
                        }
                    }
                }
                AliceState::BtcRefunded { spend_key, state3 } => {
                    let view_key = state3.v;

                    self.monero_wallet
                        .create_and_load_wallet_for_output(spend_key, view_key)
                        .await?;

                    let state = AliceState::XmrRefunded;
                    let db_state = (&state).into();
                    self.db
                        .insert_latest_state(self.swap_id, Swap::Alice(db_state))
                        .await?;
                    Ok(state)
                }
                AliceState::BtcPunishable { tx_refund, state3 } => {
                    let signed_tx_punish = build_bitcoin_punish_transaction(
                        &state3.tx_lock,
                        state3.cancel_timelock,
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

                    let refund_tx_seen =
                        watch_for_tx_refund(tx_refund.txid(), self.bitcoin_wallet.clone());

                    pin_mut!(punish_tx_finalised);
                    pin_mut!(refund_tx_seen);

                    let state = match select(punish_tx_finalised, refund_tx_seen).await {
                        Either::Left(_) => AliceState::BtcPunished,
                        Either::Right((published_refund_tx, _)) => {
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
                    let db_state = (&state).into();
                    self.db
                        .insert_latest_state(self.swap_id, Swap::Alice(db_state))
                        .await?;
                    self.run_until(state, is_target_state).await
                }
                AliceState::XmrRefunded => Ok(AliceState::XmrRefunded),
                AliceState::BtcRedeemed => Ok(AliceState::BtcRedeemed),
                AliceState::BtcPunished => Ok(AliceState::BtcPunished),
                AliceState::SafelyAborted => Ok(AliceState::SafelyAborted),
            }
        }
    }
}

pub fn is_complete(state: &AliceState) -> bool {
    matches!(
        state,
        AliceState::XmrRefunded
            | AliceState::BtcRedeemed
            | AliceState::BtcPunished
            | AliceState::SafelyAborted
    )
}

pub fn is_xmr_locked(state: &AliceState) -> bool {
    matches!(
        state,
        AliceState::XmrLocked{..}
    )
}

pub fn is_encsig_learned(state: &AliceState) -> bool {
    matches!(
        state,
        AliceState::EncSigLearned{..}
    )
}
