//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::{
    alice::{
        execution::{
            build_bitcoin_punish_transaction, build_bitcoin_redeem_transaction,
            extract_monero_private_key, lock_xmr, negotiate, publish_bitcoin_punish_transaction,
            publish_bitcoin_redeem_transaction, publish_cancel_transaction,
            wait_for_bitcoin_encrypted_signature, wait_for_bitcoin_refund, wait_for_locked_bitcoin,
        },
        Swarm,
    },
    bitcoin,
    bitcoin::EncryptedSignature,
    monero,
    network::request_response::AliceToBob,
    state,
    state::{Alice, Swap},
    storage::Database,
    SwapAmounts,
};
use anyhow::{bail, Result};
use async_recursion::async_recursion;
use futures::{
    future::{select, Either},
    pin_mut,
};
use libp2p::request_response::ResponseChannel;
use rand::{CryptoRng, RngCore};
use std::{
    convert::{TryFrom, TryInto},
    sync::Arc,
};
use uuid::Uuid;
use xmr_btc::{
    alice::State3,
    bitcoin::{TransactionBlockHeight, TxCancel, TxRefund, WatchForRawTransaction},
    cross_curve_dleq,
    monero::CreateWalletForOutput,
};

trait Rng: RngCore + CryptoRng + Send {}

impl<T> Rng for T where T: RngCore + CryptoRng + Send {}

// The same data structure is used for swap execution and recovery.
// This allows for a seamless transition from a failed swap to recovery.
#[allow(clippy::large_enum_variant)]
pub enum AliceState {
    Started {
        amounts: SwapAmounts,
        a: bitcoin::SecretKey,
        s_a: cross_curve_dleq::Scalar,
        v_a: monero::PrivateViewKey,
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
    WaitingToCancel {
        state3: State3,
    },
    Punished,
    SafelyAborted,
}

impl TryFrom<&AliceState> for state::Swap {
    type Error = anyhow::Error;

    fn try_from(alice_state: &AliceState) -> Result<Self> {
        use state::{Alice::*, Swap::Alice};

        let state = match alice_state {
            AliceState::Started { .. } => bail!("Does not support storing `Started state."),
            AliceState::Negotiated { state3, .. } => Negotiated(state3.clone()),
            AliceState::BtcLocked { state3, .. } => BtcLocked(state3.clone()),
            AliceState::XmrLocked { state3 } => XmrLocked(state3.clone()),
            AliceState::EncSignLearned {
                state3,
                encrypted_signature,
            } => EncSignLearned {
                state: state3.clone(),
                encrypted_signature: encrypted_signature.clone(),
            },
            AliceState::BtcRedeemed => SwapComplete,
            AliceState::BtcCancelled { state3, .. } => BtcCancelled(state3.clone()),
            AliceState::BtcRefunded { .. } => SwapComplete,
            AliceState::BtcPunishable { state3, .. } => BtcPunishable(state3.clone()),
            AliceState::XmrRefunded => SwapComplete,
            // TODO(Franck): it may be more efficient to store the fact that we already want to
            // abort
            AliceState::WaitingToCancel { state3 } => XmrLocked(state3.clone()),
            AliceState::Punished => SwapComplete,
            AliceState::SafelyAborted => SwapComplete,
        };

        Ok(Alice(state))
    }
}

impl TryFrom<state::Swap> for AliceState {
    type Error = anyhow::Error;

    fn try_from(db_state: Swap) -> Result<Self, Self::Error> {
        use AliceState::*;
        if let Swap::Alice(state) = db_state {
            let alice_state = match state {
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
                Alice::BtcRedeemable { .. } => bail!("BtcRedeemable state is unexpected"),
                Alice::EncSignLearned {
                    state,
                    encrypted_signature,
                } => EncSignLearned {
                    state3: state,
                    encrypted_signature,
                },
                Alice::BtcCancelled(state) => {
                    let tx_cancel = bitcoin::TxCancel::new(
                        &state.tx_lock,
                        state.refund_timelock,
                        state.a.public(),
                        state.B.clone(),
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
                        state.B.clone(),
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
            };
            Ok(alice_state)
        } else {
            bail!("Alice swap state expected.")
        }
    }
}

// State machine driver for swap execution
#[async_recursion]
pub async fn swap(
    state: AliceState,
    mut swarm: Swarm,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
    swap_id: Uuid,
    db: Database,
) -> Result<AliceState> {
    match state {
        AliceState::Started {
            amounts,
            a,
            s_a,
            v_a,
        } => {
            let (channel, state3) =
                negotiate(amounts, a, s_a, v_a, &mut swarm, bitcoin_wallet.clone()).await?;

            let state = AliceState::Negotiated {
                channel: Some(channel),
                amounts,
                state3,
            };

            let db_state = (&state).try_into()?;
            db.insert_latest_state(swap_id, db_state).await?;
            swap(state, swarm, bitcoin_wallet, monero_wallet, swap_id, db).await
        }
        AliceState::Negotiated {
            state3,
            channel,
            amounts,
        } => {
            let state = match channel {
                Some(channel) => {
                    let _ = wait_for_locked_bitcoin(state3.tx_lock.txid(), bitcoin_wallet.clone())
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

            let db_state = (&state).try_into()?;
            db.insert_latest_state(swap_id, db_state).await?;
            swap(state, swarm, bitcoin_wallet, monero_wallet, swap_id, db).await
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
                        state3.clone(),
                        &mut swarm,
                        monero_wallet.clone(),
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

            let db_state = (&state).try_into()?;
            db.insert_latest_state(swap_id, db_state).await?;
            swap(state, swarm, bitcoin_wallet, monero_wallet, swap_id, db).await
        }
        AliceState::XmrLocked { state3 } => {
            // Our Monero is locked, we need to go through the cancellation process if this
            // step fails
            let state = match wait_for_bitcoin_encrypted_signature(&mut swarm).await {
                Ok(encrypted_signature) => AliceState::EncSignLearned {
                    state3,
                    encrypted_signature,
                },
                Err(_) => AliceState::WaitingToCancel { state3 },
            };

            let db_state = (&state).try_into()?;
            db.insert_latest_state(swap_id, db_state).await?;
            swap(state, swarm, bitcoin_wallet, monero_wallet, swap_id, db).await
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
                state3.B.clone(),
                &state3.redeem_address,
            ) {
                Ok(tx) => tx,
                Err(_) => {
                    let state = AliceState::WaitingToCancel { state3 };
                    let db_state = (&state).try_into()?;
                    db.insert_latest_state(swap_id, db_state).await?;
                    return swap(state, swarm, bitcoin_wallet, monero_wallet, swap_id, db).await;
                }
            };

            // TODO(Franck): Error handling is delicate here.
            // If Bob sees this transaction he can redeem Monero
            // e.g. If the Bitcoin node is down then the user needs to take action.
            publish_bitcoin_redeem_transaction(signed_tx_redeem, bitcoin_wallet.clone()).await?;

            let state = AliceState::BtcRedeemed;
            let db_state = (&state).try_into()?;
            db.insert_latest_state(swap_id, db_state).await?;
            swap(state, swarm, bitcoin_wallet, monero_wallet, swap_id, db).await
        }
        AliceState::WaitingToCancel { state3 } => {
            let tx_cancel = publish_cancel_transaction(
                state3.tx_lock.clone(),
                state3.a.clone(),
                state3.B.clone(),
                state3.refund_timelock,
                state3.tx_cancel_sig_bob.clone(),
                bitcoin_wallet.clone(),
            )
            .await?;

            let state = AliceState::BtcCancelled { state3, tx_cancel };
            let db_state = (&state).try_into()?;
            db.insert_latest_state(swap_id, db_state).await?;
            swap(state, swarm, bitcoin_wallet, monero_wallet, swap_id, db).await
        }
        AliceState::BtcCancelled { state3, tx_cancel } => {
            let tx_cancel_height = bitcoin_wallet
                .transaction_block_height(tx_cancel.txid())
                .await;

            let (tx_refund, published_refund_tx) = wait_for_bitcoin_refund(
                &tx_cancel,
                tx_cancel_height,
                state3.punish_timelock,
                &state3.refund_address,
                bitcoin_wallet.clone(),
            )
            .await?;

            // TODO(Franck): Review error handling
            match published_refund_tx {
                None => {
                    let state = AliceState::BtcPunishable { tx_refund, state3 };
                    let db_state = (&state).try_into()?;
                    db.insert_latest_state(swap_id, db_state).await?;
                    swap(
                        state,
                        swarm,
                        bitcoin_wallet.clone(),
                        monero_wallet,
                        swap_id,
                        db,
                    )
                    .await
                }
                Some(published_refund_tx) => {
                    let spend_key = extract_monero_private_key(
                        published_refund_tx,
                        tx_refund,
                        state3.s_a,
                        state3.a.clone(),
                        state3.S_b_bitcoin.clone(),
                    )?;

                    let state = AliceState::BtcRefunded { spend_key, state3 };
                    let db_state = (&state).try_into()?;
                    db.insert_latest_state(swap_id, db_state).await?;
                    swap(
                        state,
                        swarm,
                        bitcoin_wallet.clone(),
                        monero_wallet,
                        swap_id,
                        db,
                    )
                    .await
                }
            }
        }
        AliceState::BtcRefunded { spend_key, state3 } => {
            let view_key = state3.v;

            monero_wallet
                .create_and_load_wallet_for_output(spend_key, view_key)
                .await?;

            let state = AliceState::XmrRefunded;
            let db_state = (&state).try_into()?;
            db.insert_latest_state(swap_id, db_state).await?;
            Ok(state)
        }
        AliceState::BtcPunishable { tx_refund, state3 } => {
            let signed_tx_punish = build_bitcoin_punish_transaction(
                &state3.tx_lock,
                state3.refund_timelock,
                &state3.punish_address,
                state3.punish_timelock,
                state3.tx_punish_sig_bob.clone(),
                state3.a.clone(),
                state3.B.clone(),
            )?;

            let punish_tx_finalised =
                publish_bitcoin_punish_transaction(signed_tx_punish, bitcoin_wallet.clone());

            let refund_tx_seen = bitcoin_wallet.watch_for_raw_transaction(tx_refund.txid());

            pin_mut!(punish_tx_finalised);
            pin_mut!(refund_tx_seen);

            match select(punish_tx_finalised, refund_tx_seen).await {
                Either::Left(_) => {
                    let state = AliceState::Punished;
                    let db_state = (&state).try_into()?;
                    db.insert_latest_state(swap_id, db_state).await?;
                    swap(
                        state,
                        swarm,
                        bitcoin_wallet.clone(),
                        monero_wallet,
                        swap_id,
                        db,
                    )
                    .await
                }
                Either::Right((published_refund_tx, _)) => {
                    let spend_key = extract_monero_private_key(
                        published_refund_tx,
                        tx_refund,
                        state3.s_a,
                        state3.a.clone(),
                        state3.S_b_bitcoin.clone(),
                    )?;
                    let state = AliceState::BtcRefunded { spend_key, state3 };
                    let db_state = (&state).try_into()?;
                    db.insert_latest_state(swap_id, db_state).await?;
                    swap(
                        state,
                        swarm,
                        bitcoin_wallet.clone(),
                        monero_wallet,
                        swap_id,
                        db,
                    )
                    .await
                }
            }
        }
        AliceState::XmrRefunded => Ok(AliceState::XmrRefunded),
        AliceState::BtcRedeemed => Ok(AliceState::BtcRedeemed),
        AliceState::Punished => Ok(AliceState::Punished),
        AliceState::SafelyAborted => Ok(AliceState::SafelyAborted),
    }
}
