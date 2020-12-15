//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::{
    bitcoin,
    bitcoin::EncryptedSignature,
    network::{alice::event_loop::EventLoopHandle, request_response::AliceToBob},
    storage::{model, Database},
    SwapAmounts,
};
use anyhow::{bail, Context, Result};
use async_recursion::async_recursion;
use ecdsa_fun::{adaptor::Adaptor, fun::nonce::Deterministic};
use futures::{
    future::{select, Either},
    pin_mut,
};
use libp2p::request_response::ResponseChannel;
use rand::{rngs::OsRng, CryptoRng, RngCore};
use sha2::Sha256;
use std::{convert::TryFrom, fmt, sync::Arc, time::Duration};
use tokio::time::timeout;
use tracing::{info, trace};
use uuid::Uuid;
use xmr_btc::{
    alice,
    alice::{State0, State3},
    bitcoin::{
        poll_until_block_height_is_gte, BlockHeight, BroadcastSignedTransaction, GetRawTransaction,
        TransactionBlockHeight, TxCancel, TxLock, TxRefund, WaitForTransactionFinality,
        WatchForRawTransaction,
    },
    config::Config,
    cross_curve_dleq, monero,
    monero::{CreateWalletForOutput, Transfer},
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

impl From<&AliceState> for model::Alice {
    fn from(alice_state: &AliceState) -> Self {
        match alice_state {
            AliceState::Negotiated { state3, .. } => model::Alice::Negotiated(state3.clone()),
            AliceState::BtcLocked { state3, .. } => model::Alice::BtcLocked(state3.clone()),
            AliceState::XmrLocked { state3 } => model::Alice::XmrLocked(state3.clone()),
            AliceState::EncSignLearned {
                state3,
                encrypted_signature,
            } => model::Alice::EncSignLearned {
                state: state3.clone(),
                encrypted_signature: encrypted_signature.clone(),
            },
            AliceState::BtcRedeemed => model::Alice::SwapComplete,
            AliceState::BtcCancelled { state3, .. } => model::Alice::BtcCancelled(state3.clone()),
            AliceState::BtcRefunded { .. } => model::Alice::SwapComplete,
            AliceState::BtcPunishable { state3, .. } => model::Alice::BtcPunishable(state3.clone()),
            AliceState::XmrRefunded => model::Alice::SwapComplete,
            AliceState::T1Expired { state3 } => model::Alice::T1Expired(state3.clone()),
            AliceState::Punished => model::Alice::SwapComplete,
            AliceState::SafelyAborted => model::Alice::SwapComplete,
            // TODO: Potentially add support to resume swaps that are not Negotiated
            AliceState::Started { .. } => {
                panic!("Alice attempted to save swap before being negotiated")
            }
        }
    }
}

impl TryFrom<model::Swap> for AliceState {
    type Error = anyhow::Error;

    fn try_from(db_state: model::Swap) -> Result<Self, Self::Error> {
        use AliceState::*;
        if let model::Swap::Alice(state) = db_state {
            let alice_state = match state {
                model::Alice::Negotiated(state3) => Negotiated {
                    channel: None,
                    amounts: SwapAmounts {
                        btc: state3.btc,
                        xmr: state3.xmr,
                    },
                    state3,
                },
                model::Alice::BtcLocked(state3) => BtcLocked {
                    channel: None,
                    amounts: SwapAmounts {
                        btc: state3.btc,
                        xmr: state3.xmr,
                    },
                    state3,
                },
                model::Alice::XmrLocked(state3) => XmrLocked { state3 },
                model::Alice::BtcRedeemable { .. } => bail!("BtcRedeemable state is unexpected"),
                model::Alice::EncSignLearned {
                    state,
                    encrypted_signature,
                } => EncSignLearned {
                    state3: state,
                    encrypted_signature,
                },
                model::Alice::T1Expired(state3) => AliceState::T1Expired { state3 },
                model::Alice::BtcCancelled(state) => {
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
                model::Alice::BtcPunishable(state) => {
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
                model::Alice::BtcRefunded {
                    state, spend_key, ..
                } => BtcRefunded {
                    spend_key,
                    state3: state,
                },
                model::Alice::SwapComplete => {
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

pub async fn swap(
    state: AliceState,
    event_loop_handle: EventLoopHandle,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
    config: Config,
    swap_id: Uuid,
    db: Database,
) -> Result<AliceState> {
    run_until(
        state,
        is_complete,
        event_loop_handle,
        bitcoin_wallet,
        monero_wallet,
        config,
        swap_id,
        db,
    )
    .await
}

pub async fn resume_from_database(
    event_loop_handle: EventLoopHandle,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
    config: Config,
    swap_id: Uuid,
    db: Database,
) -> Result<AliceState> {
    let db_swap = db.get_state(swap_id)?;
    let start_state = AliceState::try_from(db_swap)?;
    let state = swap(
        start_state,
        event_loop_handle,
        bitcoin_wallet,
        monero_wallet,
        config,
        swap_id,
        db,
    )
    .await?;
    Ok(state)
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

pub fn is_xmr_locked(state: &AliceState) -> bool {
    matches!(
        state,
        AliceState::XmrLocked{..}
    )
}

pub fn is_encsig_learned(state: &AliceState) -> bool {
    matches!(
        state,
        AliceState::EncSignLearned{..}
    )
}

// State machine driver for swap execution
#[async_recursion]
#[allow(clippy::too_many_arguments)]
pub async fn run_until(
    state: AliceState,
    is_target_state: fn(&AliceState) -> bool,
    mut event_loop_handle: EventLoopHandle,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
    config: Config,
    swap_id: Uuid,
    db: Database,
    // TODO: Remove EventLoopHandle!
) -> Result<AliceState> {
    info!("Current state:{}", state);
    if is_target_state(&state) {
        Ok(state)
    } else {
        match state {
            AliceState::Started { amounts, state0 } => {
                let (channel, state3) =
                    negotiate(state0, amounts, &mut event_loop_handle, config).await?;

                let state = AliceState::Negotiated {
                    channel: Some(channel),
                    amounts,
                    state3,
                };

                let db_state = (&state).into();
                db.insert_latest_state(swap_id, model::Swap::Alice(db_state))
                    .await?;
                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    bitcoin_wallet,
                    monero_wallet,
                    config,
                    swap_id,
                    db,
                )
                .await
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
                            bitcoin_wallet.clone(),
                            config,
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
                db.insert_latest_state(swap_id, model::Swap::Alice(db_state))
                    .await?;
                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    bitcoin_wallet,
                    monero_wallet,
                    config,
                    swap_id,
                    db,
                )
                .await
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
                            &mut event_loop_handle,
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

                let db_state = (&state).into();
                db.insert_latest_state(swap_id, model::Swap::Alice(db_state))
                    .await?;
                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    bitcoin_wallet,
                    monero_wallet,
                    config,
                    swap_id,
                    db,
                )
                .await
            }
            AliceState::XmrLocked { state3 } => {
                // todo: match statement and wait for t1 can probably be expressed more cleanly
                let state = match state3.current_epoch(bitcoin_wallet.as_ref()).await? {
                    Epoch::T0 => {
                        let wait_for_enc_sig = wait_for_bitcoin_encrypted_signature(
                            &mut event_loop_handle,
                            config.monero_max_finality_time,
                        );
                        let state3_clone = state3.clone();
                        let t1_timeout = state3_clone.wait_for_t1(bitcoin_wallet.as_ref());

                        pin_mut!(wait_for_enc_sig);
                        pin_mut!(t1_timeout);

                        match select(t1_timeout, wait_for_enc_sig).await {
                            Either::Left(_) => AliceState::T1Expired { state3 },
                            Either::Right((enc_sig, _)) => AliceState::EncSignLearned {
                                state3,
                                encrypted_signature: enc_sig?,
                            },
                        }
                    }
                    _ => AliceState::T1Expired { state3 },
                };

                let db_state = (&state).into();
                db.insert_latest_state(swap_id, model::Swap::Alice(db_state))
                    .await?;
                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    bitcoin_wallet.clone(),
                    monero_wallet,
                    config,
                    swap_id,
                    db,
                )
                .await
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
                        state3.wait_for_t1(bitcoin_wallet.as_ref()).await?;

                        let state = AliceState::T1Expired { state3 };
                        let db_state = (&state).into();
                        db.insert_latest_state(swap_id, model::Swap::Alice(db_state))
                            .await?;
                        return run_until(
                            state,
                            is_target_state,
                            event_loop_handle,
                            bitcoin_wallet,
                            monero_wallet,
                            config,
                            swap_id,
                            db,
                        )
                        .await;
                    }
                };

                // TODO(Franck): Error handling is delicate here.
                // If Bob sees this transaction he can redeem Monero
                // e.g. If the Bitcoin node is down then the user needs to take action.
                publish_bitcoin_redeem_transaction(
                    signed_tx_redeem,
                    bitcoin_wallet.clone(),
                    config,
                )
                .await?;

                let state = AliceState::BtcRedeemed;
                let db_state = (&state).into();
                db.insert_latest_state(swap_id, model::Swap::Alice(db_state))
                    .await?;
                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    bitcoin_wallet,
                    monero_wallet,
                    config,
                    swap_id,
                    db,
                )
                .await
            }
            AliceState::T1Expired { state3 } => {
                let tx_cancel = publish_cancel_transaction(
                    state3.tx_lock.clone(),
                    state3.a.clone(),
                    state3.B,
                    state3.refund_timelock,
                    state3.tx_cancel_sig_bob.clone(),
                    bitcoin_wallet.clone(),
                )
                .await?;

                let state = AliceState::BtcCancelled { state3, tx_cancel };
                let db_state = (&state).into();
                db.insert_latest_state(swap_id, model::Swap::Alice(db_state))
                    .await?;
                run_until(
                    state,
                    is_target_state,
                    event_loop_handle,
                    bitcoin_wallet,
                    monero_wallet,
                    config,
                    swap_id,
                    db,
                )
                .await
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
                        let db_state = (&state).into();
                        db.insert_latest_state(swap_id, model::Swap::Alice(db_state))
                            .await?;
                        swap(
                            state,
                            event_loop_handle,
                            bitcoin_wallet.clone(),
                            monero_wallet,
                            config,
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
                            state3.S_b_bitcoin,
                        )?;

                        let state = AliceState::BtcRefunded { spend_key, state3 };
                        let db_state = (&state).into();
                        db.insert_latest_state(swap_id, model::Swap::Alice(db_state))
                            .await?;
                        run_until(
                            state,
                            is_target_state,
                            event_loop_handle,
                            bitcoin_wallet.clone(),
                            monero_wallet,
                            config,
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
                let db_state = (&state).into();
                db.insert_latest_state(swap_id, model::Swap::Alice(db_state))
                    .await?;
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
                    state3.B,
                )?;

                let punish_tx_finalised = publish_bitcoin_punish_transaction(
                    signed_tx_punish,
                    bitcoin_wallet.clone(),
                    config,
                );

                let refund_tx_seen = bitcoin_wallet.watch_for_raw_transaction(tx_refund.txid());

                pin_mut!(punish_tx_finalised);
                pin_mut!(refund_tx_seen);

                match select(punish_tx_finalised, refund_tx_seen).await {
                    Either::Left(_) => {
                        let state = AliceState::Punished;
                        let db_state = (&state).into();
                        db.insert_latest_state(swap_id, model::Swap::Alice(db_state))
                            .await?;
                        run_until(
                            state,
                            is_target_state,
                            event_loop_handle,
                            bitcoin_wallet.clone(),
                            monero_wallet,
                            config,
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
                            state3.S_b_bitcoin,
                        )?;
                        let state = AliceState::BtcRefunded { spend_key, state3 };
                        let db_state = (&state).into();
                        db.insert_latest_state(swap_id, model::Swap::Alice(db_state))
                            .await?;
                        run_until(
                            state,
                            is_target_state,
                            event_loop_handle,
                            bitcoin_wallet.clone(),
                            monero_wallet,
                            config,
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
}
pub async fn negotiate(
    state0: xmr_btc::alice::State0,
    amounts: SwapAmounts,
    event_loop_handle: &mut EventLoopHandle,
    config: Config,
) -> Result<(ResponseChannel<AliceToBob>, State3)> {
    trace!("Starting negotiate");

    // todo: we can move this out, we dont need to timeout here
    let _peer_id = timeout(
        config.bob_time_to_act,
        event_loop_handle.recv_conn_established(),
    )
    .await
    .context("Failed to receive dial connection from Bob")??;

    let event = timeout(config.bob_time_to_act, event_loop_handle.recv_request())
        .await
        .context("Failed to receive amounts from Bob")??;

    if event.btc != amounts.btc {
        bail!(
            "Bob proposed a different amount; got {}, expected: {}",
            event.btc,
            amounts.btc
        );
    }

    event_loop_handle
        .send_amounts(event.channel, amounts)
        .await?;

    let (bob_message0, channel) =
        timeout(config.bob_time_to_act, event_loop_handle.recv_message0()).await??;

    let alice_message0 = state0.next_message(&mut OsRng);
    event_loop_handle
        .send_message0(channel, alice_message0)
        .await?;

    let state1 = state0.receive(bob_message0)?;

    let (bob_message1, channel) =
        timeout(config.bob_time_to_act, event_loop_handle.recv_message1()).await??;

    let state2 = state1.receive(bob_message1);

    event_loop_handle
        .send_message1(channel, state2.next_message())
        .await?;

    let (bob_message2, channel) =
        timeout(config.bob_time_to_act, event_loop_handle.recv_message2()).await??;

    let state3 = state2.receive(bob_message2)?;

    Ok((channel, state3))
}

// TODO(Franck): Use helper functions from xmr-btc instead of re-writing them
// here
pub async fn wait_for_locked_bitcoin<W>(
    lock_bitcoin_txid: bitcoin::Txid,
    bitcoin_wallet: Arc<W>,
    config: Config,
) -> Result<()>
where
    W: WatchForRawTransaction + WaitForTransactionFinality,
{
    // We assume we will see Bob's transaction in the mempool first.
    timeout(
        config.bob_time_to_act,
        bitcoin_wallet.watch_for_raw_transaction(lock_bitcoin_txid),
    )
    .await
    .context("Failed to find lock Bitcoin tx")?;

    // // We saw the transaction in the mempool, waiting for it to be confirmed.
    // bitcoin_wallet
    //     .wait_for_transaction_finality(lock_bitcoin_txid, config)
    //     .await;

    Ok(())
}

pub async fn lock_xmr<W>(
    channel: ResponseChannel<AliceToBob>,
    amounts: SwapAmounts,
    state3: State3,
    event_loop_handle: &mut EventLoopHandle,
    monero_wallet: Arc<W>,
) -> Result<()>
where
    W: Transfer,
{
    let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey {
        scalar: state3.s_a.into_ed25519(),
    });

    let public_spend_key = S_a + state3.S_b_monero;
    let public_view_key = state3.v.public();

    let (transfer_proof, _) = monero_wallet
        .transfer(public_spend_key, public_view_key, amounts.xmr)
        .await?;

    // TODO(Franck): Wait for Monero to be confirmed once

    event_loop_handle
        .send_message2(channel, alice::Message2 {
            tx_lock_proof: transfer_proof,
        })
        .await?;

    Ok(())
}

pub async fn wait_for_bitcoin_encrypted_signature(
    event_loop_handle: &mut EventLoopHandle,
    timeout_duration: Duration,
) -> Result<EncryptedSignature> {
    let msg3 = timeout(timeout_duration, event_loop_handle.recv_message3())
        .await
        .context("Failed to receive Bitcoin encrypted signature from Bob")??;
    Ok(msg3.tx_redeem_encsig)
}

pub fn build_bitcoin_redeem_transaction(
    encrypted_signature: EncryptedSignature,
    tx_lock: &TxLock,
    a: bitcoin::SecretKey,
    s_a: cross_curve_dleq::Scalar,
    B: bitcoin::PublicKey,
    redeem_address: &bitcoin::Address,
) -> Result<bitcoin::Transaction> {
    let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

    let tx_redeem = bitcoin::TxRedeem::new(tx_lock, redeem_address);

    bitcoin::verify_encsig(
        B,
        s_a.into_secp256k1().into(),
        &tx_redeem.digest(),
        &encrypted_signature,
    )
    .context("Invalid encrypted signature received")?;

    let sig_a = a.sign(tx_redeem.digest());
    let sig_b = adaptor.decrypt_signature(&s_a.into_secp256k1(), encrypted_signature);

    let tx = tx_redeem
        .add_signatures(&tx_lock, (a.public(), sig_a), (B, sig_b))
        .context("sig_{a,b} are invalid for tx_redeem")?;

    Ok(tx)
}

pub async fn publish_bitcoin_redeem_transaction<W>(
    redeem_tx: bitcoin::Transaction,
    bitcoin_wallet: Arc<W>,
    config: Config,
) -> Result<()>
where
    W: BroadcastSignedTransaction + WaitForTransactionFinality,
{
    info!("Attempting to publish bitcoin redeem txn");
    let tx_id = bitcoin_wallet
        .broadcast_signed_transaction(redeem_tx)
        .await?;

    bitcoin_wallet
        .wait_for_transaction_finality(tx_id, config)
        .await
}

pub async fn publish_cancel_transaction<W>(
    tx_lock: TxLock,
    a: bitcoin::SecretKey,
    B: bitcoin::PublicKey,
    refund_timelock: u32,
    tx_cancel_sig_bob: bitcoin::Signature,
    bitcoin_wallet: Arc<W>,
) -> Result<bitcoin::TxCancel>
where
    W: GetRawTransaction + TransactionBlockHeight + BlockHeight + BroadcastSignedTransaction,
{
    // First wait for t1 to expire
    let tx_lock_height = bitcoin_wallet
        .transaction_block_height(tx_lock.txid())
        .await;
    poll_until_block_height_is_gte(bitcoin_wallet.as_ref(), tx_lock_height + refund_timelock).await;

    let tx_cancel = bitcoin::TxCancel::new(&tx_lock, refund_timelock, a.public(), B);

    // If Bob hasn't yet broadcasted the tx cancel, we do it
    if bitcoin_wallet
        .get_raw_transaction(tx_cancel.txid())
        .await
        .is_err()
    {
        // TODO(Franck): Maybe the cancel transaction is already mined, in this case,
        // the broadcast will error out.

        let sig_a = a.sign(tx_cancel.digest());
        let sig_b = tx_cancel_sig_bob.clone();

        let tx_cancel = tx_cancel
            .clone()
            .add_signatures(&tx_lock, (a.public(), sig_a), (B, sig_b))
            .expect("sig_{a,b} to be valid signatures for tx_cancel");

        // TODO(Franck): Error handling is delicate, why can't we broadcast?
        bitcoin_wallet
            .broadcast_signed_transaction(tx_cancel)
            .await?;

        // TODO(Franck): Wait until transaction is mined and returned mined
        // block height
    }

    Ok(tx_cancel)
}

pub async fn wait_for_bitcoin_refund<W>(
    tx_cancel: &TxCancel,
    cancel_tx_height: u32,
    punish_timelock: u32,
    refund_address: &bitcoin::Address,
    bitcoin_wallet: Arc<W>,
) -> Result<(bitcoin::TxRefund, Option<bitcoin::Transaction>)>
where
    W: BlockHeight + WatchForRawTransaction,
{
    let punish_timelock_expired =
        poll_until_block_height_is_gte(bitcoin_wallet.as_ref(), cancel_tx_height + punish_timelock);

    let tx_refund = bitcoin::TxRefund::new(tx_cancel, refund_address);

    // TODO(Franck): This only checks the mempool, need to cater for the case where
    // the transaction goes directly in a block
    let seen_refund_tx = bitcoin_wallet.watch_for_raw_transaction(tx_refund.txid());

    pin_mut!(punish_timelock_expired);
    pin_mut!(seen_refund_tx);

    match select(punish_timelock_expired, seen_refund_tx).await {
        Either::Left(_) => Ok((tx_refund, None)),
        Either::Right((published_refund_tx, _)) => Ok((tx_refund, Some(published_refund_tx))),
    }
}

pub fn extract_monero_private_key(
    published_refund_tx: bitcoin::Transaction,
    tx_refund: TxRefund,
    s_a: cross_curve_dleq::Scalar,
    a: bitcoin::SecretKey,
    S_b_bitcoin: bitcoin::PublicKey,
) -> Result<monero::PrivateKey> {
    let s_a = monero::PrivateKey {
        scalar: s_a.into_ed25519(),
    };

    let tx_refund_sig = tx_refund
        .extract_signature_by_key(published_refund_tx, a.public())
        .context("Failed to extract signature from Bitcoin refund tx")?;
    let tx_refund_encsig = a.encsign(S_b_bitcoin, tx_refund.digest());

    let s_b = bitcoin::recover(S_b_bitcoin, tx_refund_sig, tx_refund_encsig)
        .context("Failed to recover Monero secret key from Bitcoin signature")?;
    let s_b = monero::private_key_from_secp256k1_scalar(s_b.into());

    let spend_key = s_a + s_b;

    Ok(spend_key)
}

pub fn build_bitcoin_punish_transaction(
    tx_lock: &TxLock,
    refund_timelock: u32,
    punish_address: &bitcoin::Address,
    punish_timelock: u32,
    tx_punish_sig_bob: bitcoin::Signature,
    a: bitcoin::SecretKey,
    B: bitcoin::PublicKey,
) -> Result<bitcoin::Transaction> {
    let tx_cancel = bitcoin::TxCancel::new(&tx_lock, refund_timelock, a.public(), B);
    let tx_punish = bitcoin::TxPunish::new(&tx_cancel, &punish_address, punish_timelock);

    let sig_a = a.sign(tx_punish.digest());
    let sig_b = tx_punish_sig_bob;

    let signed_tx_punish = tx_punish
        .add_signatures(&tx_cancel, (a.public(), sig_a), (B, sig_b))
        .expect("sig_{a,b} to be valid signatures for tx_cancel");

    Ok(signed_tx_punish)
}

pub async fn publish_bitcoin_punish_transaction<W>(
    punish_tx: bitcoin::Transaction,
    bitcoin_wallet: Arc<W>,
    config: Config,
) -> Result<bitcoin::Txid>
where
    W: BroadcastSignedTransaction + WaitForTransactionFinality,
{
    let txid = bitcoin_wallet
        .broadcast_signed_transaction(punish_tx)
        .await?;

    bitcoin_wallet
        .wait_for_transaction_finality(txid, config)
        .await?;

    Ok(txid)
}
