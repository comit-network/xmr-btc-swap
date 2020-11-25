//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use self::{amounts::*, message0::*, message1::*, message2::*, message3::*};
use crate::{
    bitcoin,
    bitcoin::{EncryptedSignature, TX_LOCK_MINE_TIMEOUT},
    monero,
    network::{
        peer_tracker::{self, PeerTracker},
        request_response::AliceToBob,
        transport::SwapTransport,
        TokioExecutor,
    },
    state,
    storage::Database,
    SwapAmounts, PUNISH_TIMELOCK, REFUND_TIMELOCK,
};
use anyhow::{anyhow, bail, Context, Result};
use async_recursion::async_recursion;
use async_trait::async_trait;
use backoff::{backoff::Constant as ConstantBackoff, future::FutureOperation as _};
use ecdsa_fun::{adaptor::Adaptor, nonce::Deterministic};
use futures::{
    future::{select, Either},
    pin_mut,
};
use genawaiter::GeneratorState;
use libp2p::{
    core::{identity::Keypair, Multiaddr},
    request_response::ResponseChannel,
    NetworkBehaviour, PeerId,
};
use rand::{rngs::OsRng, CryptoRng, RngCore};
use sha2::Sha256;
use std::{sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::timeout};
use tracing::{debug, info, warn};
use uuid::Uuid;
use xmr_btc::{
    alice::{self, action_generator, Action, ReceiveBitcoinRedeemEncsig, State0, State3},
    bitcoin::{
        poll_until_block_height_is_gte, BroadcastSignedTransaction, GetRawTransaction,
        TransactionBlockHeight, TxCancel, TxRefund, WatchForRawTransaction,
        WatchForTransactionFinality,
    },
    bob, cross_curve_dleq,
    monero::{CreateWalletForOutput, Transfer},
};

mod amounts;
mod message0;
mod message1;
mod message2;
mod message3;

trait Rng: RngCore + CryptoRng + Send {}

impl<T> Rng for T where T: RngCore + CryptoRng + Send {}

// The same data structure is used for swap execution and recovery.
// This allows for a seamless transition from a failed swap to recovery.
pub enum AliceState {
    Started {
        amounts: SwapAmounts,
        a: bitcoin::SecretKey,
        s_a: cross_curve_dleq::Scalar,
        v_a: monero::PrivateViewKey,
    },
    Negotiated {
        swap_id: Uuid,
        channel: ResponseChannel<AliceToBob>,
        amounts: SwapAmounts,
        state3: State3,
    },
    BtcLocked {
        swap_id: Uuid,
        channel: ResponseChannel<AliceToBob>,
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
        tx_refund: TxRefund,
        published_refund_tx: ::bitcoin::Transaction,
        state3: State3,
    },
    BtcPunishable {
        tx_refund: TxRefund,
        state3: State3,
    },
    BtcPunished {
        tx_refund: TxRefund,
        punished_tx_id: bitcoin::Txid,
        state3: State3,
    },
    XmrRefunded,
    WaitingToCancel {
        state3: State3,
    },
    Punished,
    SafelyAborted,
}

// State machine driver for swap execution
#[async_recursion]
pub async fn simple_swap(
    state: AliceState,
    mut swarm: Swarm,
    db: Database,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
) -> Result<AliceState> {
    match state {
        AliceState::Started {
            amounts,
            a,
            s_a,
            v_a,
        } => {
            // Bob dials us
            let bob_peer_id = match swarm.next().await {
                OutEvent::ConnectionEstablished(bob_peer_id) => bob_peer_id,
                other => bail!("Unexpected event received: {:?}", other),
            };

            // Bob sends us a request
            let (btc, channel) = match swarm.next().await {
                OutEvent::Request(amounts::OutEvent::Btc { btc, channel }) => (btc, channel),
                other => bail!("Unexpected event received: {:?}", other),
            };

            if btc != amounts.btc {
                bail!(
                    "Bob proposed a different amount; got {}, expected: {}",
                    btc,
                    amounts.btc
                );
            }
            swarm.send_amounts(channel, amounts);

            let SwapAmounts { btc, xmr } = amounts;

            let redeem_address = bitcoin_wallet.as_ref().new_address().await?;
            let punish_address = redeem_address.clone();

            let state0 = State0::new(
                a,
                s_a,
                v_a,
                btc,
                xmr,
                REFUND_TIMELOCK,
                PUNISH_TIMELOCK,
                redeem_address,
                punish_address,
            );

            // Bob sends us message0
            let message0 = match swarm.next().await {
                OutEvent::Message0(msg) => msg,
                other => bail!("Unexpected event received: {:?}", other),
            };

            let state1 = state0.receive(message0)?;

            // TODO(Franck) We should use the same channel everytime,
            // Can we remove this response channel?
            let (state2, channel) = match swarm.next().await {
                OutEvent::Message1 { msg, channel } => {
                    let state2 = state1.receive(msg);
                    (state2, channel)
                }
                other => bail!("Unexpected event: {:?}", other),
            };

            let message1 = state2.next_message();
            swarm.send_message1(channel, message1);

            let (state3, channel) = match swarm.next().await {
                OutEvent::Message2 { msg, channel } => {
                    let state3 = state2.receive(msg)?;
                    (state3, channel)
                }
                other => bail!("Unexpected event: {:?}", other),
            };

            let swap_id = Uuid::new_v4();
            // TODO(Franck): Use the same terminology (negotiated) to describe this state.
            db.insert_latest_state(swap_id, state::Alice::Handshaken(state3.clone()).into())
                .await?;

            info!(
                "State transitioned from Started to Negotiated, Bob peer id is {}",
                bob_peer_id
            );

            simple_swap(
                AliceState::Negotiated {
                    swap_id,
                    state3,
                    channel,
                    amounts,
                },
                swarm,
                db,
                bitcoin_wallet,
                monero_wallet,
            )
            .await
        }
        AliceState::Negotiated {
            swap_id,
            state3,
            channel,
            amounts,
        } => {
            // TODO(1): Do a future select with watch bitcoin blockchain time
            // TODO(2): Implement a proper safe expiry module
            timeout(
                Duration::from_secs(TX_LOCK_MINE_TIMEOUT),
                // TODO(Franck): Need to check amount?
                bitcoin_wallet.watch_for_raw_transaction(state3.tx_lock.txid()),
            )
            .await
            .context("Timed out, Bob did not lock Bitcoin in time")?;

            db.insert_latest_state(swap_id, state::Alice::BtcLocked(state3.clone()).into())
                .await?;

            simple_swap(
                AliceState::BtcLocked {
                    swap_id,
                    channel,
                    amounts,
                    state3,
                },
                swarm,
                db,
                bitcoin_wallet,
                monero_wallet,
            )
            .await
        }
        AliceState::BtcLocked {
            swap_id,
            channel,
            amounts,
            state3,
        } => {
            let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey {
                scalar: state3.s_a.into_ed25519(),
            });

            let public_spend_key = S_a + state3.S_b_monero;
            let public_view_key = state3.v.public();

            // TODO(Franck): Probably need to wait at least 1 confirmation to be sure that
            // we don't wrongfully think this is done.
            let (transfer_proof, _) = monero_wallet
                .transfer(public_spend_key, public_view_key, amounts.xmr)
                .await?;

            swarm.send_message2(channel, alice::Message2 {
                tx_lock_proof: transfer_proof,
            });

            // TODO(Franck): we should merge state::Alice and AliceState.
            // There should be only 2 states:
            // 1. the cryptographic state (State0, etc) which only aware of the crypto
            // primitive to execute the protocol 2. the more general/business
            // state that contains the crypto + other business data such as network
            // communication, amounts to verify, swap id, etc.
            db.insert_latest_state(swap_id, state::Alice::XmrLocked(state3.clone()).into())
                .await?;

            simple_swap(
                AliceState::XmrLocked { state3 },
                swarm,
                db,
                bitcoin_wallet,
                monero_wallet,
            )
            .await
        }
        AliceState::XmrLocked { state3 } => {
            let encsig = timeout(
                // TODO(Franck): This is now inefficient as time has been spent since btc was
                // locked
                Duration::from_secs(TX_LOCK_MINE_TIMEOUT),
                async {
                    match swarm.next().await {
                        OutEvent::Message3(msg) => Ok(msg.tx_redeem_encsig),
                        other => Err(anyhow!(
                            "Expected Bob's Bitcoin redeem encsig, got: {:?}",
                            other
                        )),
                    }
                },
            )
            .await
            .context("Timed out, Bob did not send redeem encsign in time");

            match encsig {
                Err(_timeout_error) => {
                    // TODO(Franck): Insert in DB

                    simple_swap(
                        AliceState::WaitingToCancel { state3 },
                        swarm,
                        db,
                        bitcoin_wallet,
                        monero_wallet,
                    )
                    .await
                }
                Ok(Err(_unexpected_msg_error)) => {
                    // TODO(Franck): Insert in DB

                    simple_swap(
                        AliceState::WaitingToCancel { state3 },
                        swarm,
                        db,
                        bitcoin_wallet,
                        monero_wallet,
                    )
                    .await
                }
                Ok(Ok(encrypted_signature)) => {
                    // TODO(Franck): Insert in DB

                    simple_swap(
                        AliceState::EncSignLearned {
                            state3,
                            encrypted_signature,
                        },
                        swarm,
                        db,
                        bitcoin_wallet,
                        monero_wallet,
                    )
                    .await
                }
            }
        }
        AliceState::EncSignLearned {
            state3,
            encrypted_signature,
        } => {
            let (signed_tx_redeem, _tx_redeem_txid) = {
                let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

                let tx_redeem = bitcoin::TxRedeem::new(&state3.tx_lock, &state3.redeem_address);

                bitcoin::verify_encsig(
                    state3.B.clone(),
                    state3.s_a.into_secp256k1().into(),
                    &tx_redeem.digest(),
                    &encrypted_signature,
                )
                .context("Invalid encrypted signature received")?;

                let sig_a = state3.a.sign(tx_redeem.digest());
                let sig_b = adaptor
                    .decrypt_signature(&state3.s_a.into_secp256k1(), encrypted_signature.clone());

                let tx = tx_redeem
                    .add_signatures(
                        &state3.tx_lock,
                        (state3.a.public(), sig_a),
                        (state3.B.clone(), sig_b),
                    )
                    .expect("sig_{a,b} to be valid signatures for tx_redeem");
                let txid = tx.txid();

                (tx, txid)
            };

            // TODO(Franck): Insert in db

            let _ = bitcoin_wallet
                .broadcast_signed_transaction(signed_tx_redeem)
                .await?;

            // TODO(Franck) Wait for confirmations

            simple_swap(
                AliceState::BtcRedeemed,
                swarm,
                db,
                bitcoin_wallet,
                monero_wallet,
            )
            .await
        }
        AliceState::WaitingToCancel { state3 } => {
            let tx_lock_height = bitcoin_wallet
                .transaction_block_height(state3.tx_lock.txid())
                .await;
            poll_until_block_height_is_gte(
                bitcoin_wallet.as_ref(),
                tx_lock_height + state3.refund_timelock,
            )
            .await;

            let tx_cancel = bitcoin::TxCancel::new(
                &state3.tx_lock,
                state3.refund_timelock,
                state3.a.public(),
                state3.B.clone(),
            );

            if let None = bitcoin_wallet.get_raw_transaction(tx_cancel.txid()).await {
                let sig_a = state3.a.sign(tx_cancel.digest());
                let sig_b = state3.tx_cancel_sig_bob.clone();

                let tx_cancel = tx_cancel
                    .clone()
                    .add_signatures(
                        &state3.tx_lock,
                        (state3.a.public(), sig_a),
                        (state3.B.clone(), sig_b),
                    )
                    .expect("sig_{a,b} to be valid signatures for tx_cancel");

                bitcoin_wallet
                    .broadcast_signed_transaction(tx_cancel)
                    .await?;
            }

            simple_swap(
                AliceState::BtcCancelled { state3, tx_cancel },
                swarm,
                db,
                bitcoin_wallet,
                monero_wallet,
            )
            .await
        }
        AliceState::BtcCancelled { state3, tx_cancel } => {
            let tx_cancel_height = bitcoin_wallet
                .transaction_block_height(tx_cancel.txid())
                .await;

            let reached_t2 = poll_until_block_height_is_gte(
                bitcoin_wallet.as_ref(),
                tx_cancel_height + state3.punish_timelock,
            );

            let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &state3.refund_address);
            let seen_refund_tx = bitcoin_wallet.watch_for_raw_transaction(tx_refund.txid());

            pin_mut!(reached_t2);
            pin_mut!(seen_refund_tx);

            match select(reached_t2, seen_refund_tx).await {
                Either::Left(_) => {
                    simple_swap(
                        AliceState::BtcPunishable { tx_refund, state3 },
                        swarm,
                        db,
                        bitcoin_wallet.clone(),
                        monero_wallet,
                    )
                    .await
                }
                Either::Right((published_refund_tx, _)) => {
                    simple_swap(
                        AliceState::BtcRefunded {
                            tx_refund,
                            published_refund_tx,
                            state3,
                        },
                        swarm,
                        db,
                        bitcoin_wallet.clone(),
                        monero_wallet,
                    )
                    .await
                }
            }
        }
        AliceState::BtcRefunded {
            tx_refund,
            published_refund_tx,
            state3,
        } => {
            let s_a = monero::PrivateKey {
                scalar: state3.s_a.into_ed25519(),
            };

            let tx_refund_sig = tx_refund
                .extract_signature_by_key(published_refund_tx, state3.a.public())
                .context("Failed to extract signature from Bitcoin refund tx")?;
            let tx_refund_encsig = state3
                .a
                .encsign(state3.S_b_bitcoin.clone(), tx_refund.digest());

            let s_b = bitcoin::recover(state3.S_b_bitcoin, tx_refund_sig, tx_refund_encsig)
                .context("Failed to recover Monero secret key from Bitcoin signature")?;
            let s_b = monero::private_key_from_secp256k1_scalar(s_b.into());

            let spend_key = s_a + s_b;
            let view_key = state3.v;

            monero_wallet
                .create_and_load_wallet_for_output(spend_key, view_key)
                .await?;

            Ok(AliceState::XmrRefunded)
        }
        AliceState::BtcPunishable { tx_refund, state3 } => {
            let tx_cancel = bitcoin::TxCancel::new(
                &state3.tx_lock,
                state3.refund_timelock,
                state3.a.public(),
                state3.B.clone(),
            );
            let tx_punish =
                bitcoin::TxPunish::new(&tx_cancel, &state3.punish_address, state3.punish_timelock);
            let punished_tx_id = tx_punish.txid();

            let sig_a = state3.a.sign(tx_punish.digest());
            let sig_b = state3.tx_punish_sig_bob.clone();

            let signed_tx_punish = tx_punish
                .add_signatures(
                    &tx_cancel,
                    (state3.a.public(), sig_a),
                    (state3.B.clone(), sig_b),
                )
                .expect("sig_{a,b} to be valid signatures for tx_cancel");

            let _ = bitcoin_wallet
                .broadcast_signed_transaction(signed_tx_punish)
                .await?;

            simple_swap(
                AliceState::BtcPunished {
                    tx_refund,
                    punished_tx_id,
                    state3,
                },
                swarm,
                db,
                bitcoin_wallet.clone(),
                monero_wallet,
            )
            .await
        }
        AliceState::BtcPunished {
            punished_tx_id,
            tx_refund,
            state3,
        } => {
            let punish_tx_finalised = bitcoin_wallet.watch_for_transaction_finality(punished_tx_id);

            let refund_tx_seen = bitcoin_wallet.watch_for_raw_transaction(tx_refund.txid());

            pin_mut!(punish_tx_finalised);
            pin_mut!(refund_tx_seen);

            match select(punish_tx_finalised, refund_tx_seen).await {
                Either::Left(_) => {
                    simple_swap(
                        AliceState::Punished,
                        swarm,
                        db,
                        bitcoin_wallet.clone(),
                        monero_wallet,
                    )
                    .await
                }
                Either::Right((published_refund_tx, _)) => {
                    simple_swap(
                        AliceState::BtcRefunded {
                            tx_refund,
                            published_refund_tx,
                            state3,
                        },
                        swarm,
                        db,
                        bitcoin_wallet.clone(),
                        monero_wallet,
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

pub async fn swap(
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    db: Database,
    listen: Multiaddr,
    transport: SwapTransport,
    behaviour: Behaviour,
) -> Result<()> {
    struct Network(Arc<Mutex<Swarm>>);

    // TODO: For retry, use `backoff::ExponentialBackoff` in production as opposed
    // to `ConstantBackoff`.
    #[async_trait]
    impl ReceiveBitcoinRedeemEncsig for Network {
        async fn receive_bitcoin_redeem_encsig(&mut self) -> bitcoin::EncryptedSignature {
            #[derive(Debug)]
            struct UnexpectedMessage;

            let encsig = (|| async {
                let mut guard = self.0.lock().await;
                let encsig = match guard.next().await {
                    OutEvent::Message3(msg) => msg.tx_redeem_encsig,
                    other => {
                        warn!("Expected Bob's Bitcoin redeem encsig, got: {:?}", other);
                        return Err(backoff::Error::Transient(UnexpectedMessage));
                    }
                };

                Result::<_, backoff::Error<UnexpectedMessage>>::Ok(encsig)
            })
            .retry(ConstantBackoff::new(Duration::from_secs(1)))
            .await
            .expect("transient errors to be retried");

            info!("Received Bitcoin redeem encsig");

            encsig
        }
    }

    let mut swarm = new_swarm(listen, transport, behaviour)?;
    let message0: bob::Message0;
    let mut state0: Option<alice::State0> = None;
    let mut last_amounts: Option<SwapAmounts> = None;

    // TODO: This loop is a neat idea for local development, as it allows us to keep
    // Alice up and let Bob keep trying to connect, request amounts and/or send the
    // first message of the handshake, but it comes at the cost of needing to handle
    // mutable state, which has already been the source of a bug at one point. This
    // is an obvious candidate for refactoring
    loop {
        match swarm.next().await {
            OutEvent::ConnectionEstablished(bob) => {
                info!("Connection established with: {}", bob);
            }
            OutEvent::Request(amounts::OutEvent::Btc { btc, channel }) => {
                let amounts = calculate_amounts(btc);
                last_amounts = Some(amounts);
                swarm.send_amounts(channel, amounts);

                let SwapAmounts { btc, xmr } = amounts;

                let redeem_address = bitcoin_wallet.as_ref().new_address().await?;
                let punish_address = redeem_address.clone();

                // TODO: Pass this in using <R: RngCore + CryptoRng>
                let rng = &mut OsRng;
                let a = bitcoin::SecretKey::new_random(rng);
                let s_a = cross_curve_dleq::Scalar::random(rng);
                let v_a = monero::PrivateViewKey::new_random(rng);
                let state = State0::new(
                    a,
                    s_a,
                    v_a,
                    btc,
                    xmr,
                    REFUND_TIMELOCK,
                    PUNISH_TIMELOCK,
                    redeem_address,
                    punish_address,
                );

                info!("Commencing handshake");
                swarm.set_state0(state.clone());

                state0 = Some(state)
            }
            OutEvent::Message0(msg) => {
                // We don't want Bob to be able to crash us by sending an out of
                // order message. Keep looping if Bob has not requested amounts.
                if last_amounts.is_some() {
                    // TODO: We should verify the amounts and notify Bob if they have changed.
                    message0 = msg;
                    break;
                }
            }
            other => panic!("Unexpected event: {:?}", other),
        };
    }

    let state1 = state0.expect("to be set").receive(message0)?;

    let (state2, channel) = match swarm.next().await {
        OutEvent::Message1 { msg, channel } => {
            let state2 = state1.receive(msg);
            (state2, channel)
        }
        other => panic!("Unexpected event: {:?}", other),
    };

    let msg = state2.next_message();
    swarm.send_message1(channel, msg);

    let state3 = match swarm.next().await {
        OutEvent::Message2(msg) => state2.receive(msg)?,
        other => panic!("Unexpected event: {:?}", other),
    };

    let swap_id = Uuid::new_v4();
    db.insert_latest_state(swap_id, state::Alice::Handshaken(state3.clone()).into())
        .await?;

    info!("Handshake complete, we now have State3 for Alice.");

    let network = Arc::new(Mutex::new(Network(unimplemented!())));

    let mut action_generator = action_generator(
        network,
        bitcoin_wallet.clone(),
        state3.clone(),
        TX_LOCK_MINE_TIMEOUT,
    );

    loop {
        let state = action_generator.async_resume().await;

        tracing::info!("Resumed execution of generator, got: {:?}", state);

        match state {
            GeneratorState::Yielded(Action::LockXmr {
                amount,
                public_spend_key,
                public_view_key,
            }) => {
                db.insert_latest_state(swap_id, state::Alice::BtcLocked(state3.clone()).into())
                    .await?;

                let _ = monero_wallet
                    .transfer(public_spend_key, public_view_key, amount)
                    .await?;

                db.insert_latest_state(swap_id, state::Alice::XmrLocked(state3.clone()).into())
                    .await?;
            }

            GeneratorState::Yielded(Action::RedeemBtc(tx)) => {
                db.insert_latest_state(
                    swap_id,
                    state::Alice::BtcRedeemable {
                        state: state3.clone(),
                        redeem_tx: tx.clone(),
                    }
                    .into(),
                )
                .await?;

                let _ = bitcoin_wallet.broadcast_signed_transaction(tx).await?;
            }
            GeneratorState::Yielded(Action::CancelBtc(tx)) => {
                let _ = bitcoin_wallet.broadcast_signed_transaction(tx).await?;
            }
            GeneratorState::Yielded(Action::PunishBtc(tx)) => {
                db.insert_latest_state(swap_id, state::Alice::BtcPunishable(state3.clone()).into())
                    .await?;

                let _ = bitcoin_wallet.broadcast_signed_transaction(tx).await?;
            }
            GeneratorState::Yielded(Action::CreateMoneroWalletForOutput {
                spend_key,
                view_key,
            }) => {
                db.insert_latest_state(
                    swap_id,
                    state::Alice::BtcRefunded {
                        state: state3.clone(),
                        spend_key,
                        view_key,
                    }
                    .into(),
                )
                .await?;

                monero_wallet
                    .create_and_load_wallet_for_output(spend_key, view_key)
                    .await?;
            }
            GeneratorState::Complete(()) => {
                db.insert_latest_state(swap_id, state::Alice::SwapComplete.into())
                    .await?;

                return Ok(());
            }
        }
    }
}

pub type Swarm = libp2p::Swarm<Behaviour>;

fn new_swarm(listen: Multiaddr, transport: SwapTransport, behaviour: Behaviour) -> Result<Swarm> {
    use anyhow::Context as _;

    let local_peer_id = behaviour.peer_id();

    let mut swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, local_peer_id.clone())
        .executor(Box::new(TokioExecutor {
            handle: tokio::runtime::Handle::current(),
        }))
        .build();

    Swarm::listen_on(&mut swarm, listen.clone())
        .with_context(|| format!("Address is not supported: {:#}", listen))?;

    tracing::info!("Initialized swarm: {}", local_peer_id);

    Ok(swarm)
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum OutEvent {
    ConnectionEstablished(PeerId),
    // TODO (Franck): Change this to get both amounts so parties can verify the amounts are
    // expected early on.
    Request(amounts::OutEvent), // Not-uniform with Bob on purpose, ready for adding Xmr event.
    Message0(bob::Message0),
    Message1 {
        msg: bob::Message1,
        channel: ResponseChannel<AliceToBob>,
    },
    Message2(bob::Message2),
    Message3(bob::Message3),
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
        OutEvent::Request(event)
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
            message1::OutEvent::Msg { msg, channel } => OutEvent::Message1 { msg, channel },
        }
    }
}

impl From<message2::OutEvent> for OutEvent {
    fn from(event: message2::OutEvent) -> Self {
        match event {
            message2::OutEvent::Msg { msg, .. } => OutEvent::Message2(msg),
        }
    }
}

impl From<message3::OutEvent> for OutEvent {
    fn from(event: message3::OutEvent) -> Self {
        match event {
            message3::OutEvent::Msg(msg) => OutEvent::Message3(msg),
        }
    }
}

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", event_process = false)]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pt: PeerTracker,
    amounts: Amounts,
    message0: Message0,
    message1: Message1,
    message2: Message2,
    message3: Message3,
    #[behaviour(ignore)]
    identity: Keypair,
}

impl Behaviour {
    pub fn identity(&self) -> Keypair {
        self.identity.clone()
    }

    pub fn peer_id(&self) -> PeerId {
        PeerId::from(self.identity.public())
    }

    /// Alice always sends her messages as a response to a request from Bob.
    pub fn send_amounts(&mut self, channel: ResponseChannel<AliceToBob>, amounts: SwapAmounts) {
        let msg = AliceToBob::Amounts(amounts);
        self.amounts.send(channel, msg);
        info!("Sent amounts response");
    }

    // TODO(Franck) remove
    /// Message0 gets sent within the network layer using this state0.
    pub fn set_state0(&mut self, state: State0) {
        debug!("Set state 0");
        let _ = self.message0.set_state(state);
    }

    /// Send Message1 to Bob in response to receiving his Message1.
    pub fn send_message1(
        &mut self,
        channel: ResponseChannel<AliceToBob>,
        msg: xmr_btc::alice::Message1,
    ) {
        self.message1.send(channel, msg);
        debug!("Sent Message1");
    }
}

impl Default for Behaviour {
    fn default() -> Self {
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

fn calculate_amounts(btc: ::bitcoin::Amount) -> SwapAmounts {
    // TODO (Franck): This should instead verify that the received amounts matches
    // the command line arguments This value corresponds to 100 XMR per BTC
    const PICONERO_PER_SAT: u64 = 1_000_000;

    let picos = btc.as_sat() * PICONERO_PER_SAT;
    let xmr = monero::Amount::from_piconero(picos);

    SwapAmounts { btc, xmr }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_BTC: u64 = 100_000_000;
    const HUNDRED_XMR: u64 = 100_000_000_000_000;

    #[test]
    fn one_bitcoin_equals_a_hundred_moneroj() {
        let btc = ::bitcoin::Amount::from_sat(ONE_BTC);
        let want = monero::Amount::from_piconero(HUNDRED_XMR);

        let SwapAmounts { xmr: got, .. } = calculate_amounts(btc);
        assert_eq!(got, want);
    }
}
