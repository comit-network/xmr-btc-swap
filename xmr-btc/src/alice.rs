use crate::{
    bitcoin,
    bitcoin::{poll_until_block_height_is_gte, BroadcastSignedTransaction, WatchForRawTransaction},
    bob, monero,
    monero::{CreateWalletForOutput, Transfer},
    transport::{ReceiveMessage, SendMessage},
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use ecdsa_fun::{
    adaptor::{Adaptor, EncryptedSignature},
    nonce::Deterministic,
};
use futures::{
    future::{select, Either},
    pin_mut, FutureExt,
};
use genawaiter::sync::{Gen, GenBoxed};
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    convert::{TryFrom, TryInto},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::Mutex, time::timeout};
use tracing::{error, info};

pub mod message;
pub use message::{Message, Message0, Message1, Message2};

#[derive(Debug)]
pub enum Action {
    // This action also includes proving to Bob that this has happened, given that our current
    // protocol requires a transfer proof to verify that the coins have been locked on Monero
    LockXmr {
        amount: monero::Amount,
        public_spend_key: monero::PublicKey,
        public_view_key: monero::PublicViewKey,
    },
    RedeemBtc(bitcoin::Transaction),
    CreateMoneroWalletForOutput {
        spend_key: monero::PrivateKey,
        view_key: monero::PrivateViewKey,
    },
    CancelBtc(bitcoin::Transaction),
    PunishBtc(bitcoin::Transaction),
}

// TODO: This could be moved to the bitcoin module
#[async_trait]
pub trait ReceiveBitcoinRedeemEncsig {
    async fn receive_bitcoin_redeem_encsig(&mut self) -> bitcoin::EncryptedSignature;
}

/// Perform the on-chain protocol to swap monero and bitcoin as Alice.
///
/// This is called post handshake, after all the keys, addresses and most of the
/// signatures have been exchanged.
///
/// The argument `bitcoin_tx_lock_timeout` is used to determine how long we will
/// wait for Bob, the counterparty, to lock up the bitcoin.
pub fn action_generator<N, B>(
    network: Arc<Mutex<N>>,
    bitcoin_client: Arc<B>,
    // TODO: Replace this with a new, slimmer struct?
    State3 {
        a,
        B,
        s_a,
        S_b_monero,
        S_b_bitcoin,
        v,
        xmr,
        refund_timelock,
        punish_timelock,
        refund_address,
        redeem_address,
        punish_address,
        tx_lock,
        tx_punish_sig_bob,
        tx_cancel_sig_bob,
        ..
    }: State3,
    bitcoin_tx_lock_timeout: u64,
) -> GenBoxed<Action, (), ()>
where
    N: ReceiveBitcoinRedeemEncsig + Send + 'static,
    B: bitcoin::BlockHeight
        + bitcoin::TransactionBlockHeight
        + bitcoin::WatchForRawTransaction
        + Send
        + Sync
        + 'static,
{
    #[derive(Debug)]
    enum SwapFailed {
        BeforeBtcLock(Reason),
        AfterXmrLock(Reason),
    }

    /// Reason why the swap has failed.
    #[derive(Debug)]
    enum Reason {
        /// Bob was too slow to lock the bitcoin.
        InactiveBob,
        /// Bob's encrypted signature on the Bitcoin redeem transaction is
        /// invalid.
        InvalidEncryptedSignature,
        /// The refund timelock has been reached.
        BtcExpired,
    }

    #[derive(Debug)]
    enum RefundFailed {
        BtcPunishable,
        /// Could not find Alice's signature on the refund transaction witness
        /// stack.
        BtcRefundSignature,
        /// Could not recover secret `s_b` from Alice's refund transaction
        /// signature.
        SecretRecovery,
    }

    Gen::new_boxed(|co| async move {
        let swap_result: Result<(), SwapFailed> = async {
            timeout(
                Duration::from_secs(bitcoin_tx_lock_timeout),
                bitcoin_client.watch_for_raw_transaction(tx_lock.txid()),
            )
            .await
            .map_err(|_| SwapFailed::BeforeBtcLock(Reason::InactiveBob))?;

            let tx_lock_height = bitcoin_client
                .transaction_block_height(tx_lock.txid())
                .await;
            let poll_until_btc_has_expired = poll_until_block_height_is_gte(
                bitcoin_client.as_ref(),
                tx_lock_height + refund_timelock,
            )
            .shared();
            pin_mut!(poll_until_btc_has_expired);

            let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey {
                scalar: s_a.into_ed25519(),
            });

            co.yield_(Action::LockXmr {
                amount: xmr,
                public_spend_key: S_a + S_b_monero,
                public_view_key: v.public(),
            })
            .await;

            // TODO: Watch for LockXmr using watch-only wallet. Doing so will prevent Alice
            // from cancelling/refunding unnecessarily.

            let tx_redeem_encsig = {
                let mut guard = network.as_ref().lock().await;
                let tx_redeem_encsig = match select(
                    guard.receive_bitcoin_redeem_encsig(),
                    poll_until_btc_has_expired.clone(),
                )
                .await
                {
                    Either::Left((encsig, _)) => encsig,
                    Either::Right(_) => return Err(SwapFailed::AfterXmrLock(Reason::BtcExpired)),
                };

                tracing::debug!("select returned redeem encsig from message");

                tx_redeem_encsig
            };

            let (signed_tx_redeem, tx_redeem_txid) = {
                let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

                let tx_redeem = bitcoin::TxRedeem::new(&tx_lock, &redeem_address);

                bitcoin::verify_encsig(
                    B.clone(),
                    s_a.into_secp256k1().into(),
                    &tx_redeem.digest(),
                    &tx_redeem_encsig,
                )
                .map_err(|_| SwapFailed::AfterXmrLock(Reason::InvalidEncryptedSignature))?;

                let sig_a = a.sign(tx_redeem.digest());
                let sig_b =
                    adaptor.decrypt_signature(&s_a.into_secp256k1(), tx_redeem_encsig.clone());

                let tx = tx_redeem
                    .add_signatures(&tx_lock, (a.public(), sig_a), (B.clone(), sig_b))
                    .expect("sig_{a,b} to be valid signatures for tx_redeem");
                let txid = tx.txid();

                (tx, txid)
            };

            co.yield_(Action::RedeemBtc(signed_tx_redeem)).await;

            match select(
                bitcoin_client.watch_for_raw_transaction(tx_redeem_txid),
                poll_until_btc_has_expired,
            )
            .await
            {
                Either::Left(_) => {}
                Either::Right(_) => return Err(SwapFailed::AfterXmrLock(Reason::BtcExpired)),
            };

            Ok(())
        }
        .await;

        if let Err(ref err) = swap_result {
            error!("swap failed: {:?}", err);
        }

        if let Err(SwapFailed::AfterXmrLock(Reason::BtcExpired)) = swap_result {
            let refund_result: Result<(), RefundFailed> = async {
                let tx_cancel =
                    bitcoin::TxCancel::new(&tx_lock, refund_timelock, a.public(), B.clone());
                let signed_tx_cancel = {
                    let sig_a = a.sign(tx_cancel.digest());
                    let sig_b = tx_cancel_sig_bob.clone();

                    tx_cancel
                        .clone()
                        .add_signatures(&tx_lock, (a.public(), sig_a), (B.clone(), sig_b))
                        .expect("sig_{a,b} to be valid signatures for tx_cancel")
                };

                co.yield_(Action::CancelBtc(signed_tx_cancel)).await;

                bitcoin_client
                    .watch_for_raw_transaction(tx_cancel.txid())
                    .await;

                let tx_cancel_height = bitcoin_client
                    .transaction_block_height(tx_cancel.txid())
                    .await;
                let poll_until_bob_can_be_punished = poll_until_block_height_is_gte(
                    bitcoin_client.as_ref(),
                    tx_cancel_height + punish_timelock,
                )
                .shared();
                pin_mut!(poll_until_bob_can_be_punished);

                let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &refund_address);
                let tx_refund_published = match select(
                    bitcoin_client.watch_for_raw_transaction(tx_refund.txid()),
                    poll_until_bob_can_be_punished,
                )
                .await
                {
                    Either::Left((tx, _)) => tx,
                    Either::Right(_) => return Err(RefundFailed::BtcPunishable),
                };

                let s_a = monero::PrivateKey {
                    scalar: s_a.into_ed25519(),
                };

                let tx_refund_sig = tx_refund
                    .extract_signature_by_key(tx_refund_published, a.public())
                    .map_err(|_| RefundFailed::BtcRefundSignature)?;
                let tx_refund_encsig = a.encsign(S_b_bitcoin.clone(), tx_refund.digest());

                let s_b = bitcoin::recover(S_b_bitcoin, tx_refund_sig, tx_refund_encsig)
                    .map_err(|_| RefundFailed::SecretRecovery)?;
                let s_b = monero::private_key_from_secp256k1_scalar(s_b.into());

                co.yield_(Action::CreateMoneroWalletForOutput {
                    spend_key: s_a + s_b,
                    view_key: v,
                })
                .await;

                Ok(())
            }
            .await;

            if let Err(ref err) = refund_result {
                error!("refund failed: {:?}", err);
            }

            // LIMITATION: When approaching the punish scenario, Bob could theoretically
            // wake up in between Alice's publication of tx cancel and beat Alice's punish
            // transaction with his refund transaction. Alice would then need to carry on
            // with the refund on Monero. Doing so may be too verbose with the current,
            // linear approach. A different design may be required
            if let Err(RefundFailed::BtcPunishable) = refund_result {
                let tx_cancel =
                    bitcoin::TxCancel::new(&tx_lock, refund_timelock, a.public(), B.clone());
                let tx_punish =
                    bitcoin::TxPunish::new(&tx_cancel, &punish_address, punish_timelock);
                let tx_punish_txid = tx_punish.txid();
                let signed_tx_punish = {
                    let sig_a = a.sign(tx_punish.digest());
                    let sig_b = tx_punish_sig_bob;

                    tx_punish
                        .add_signatures(&tx_cancel, (a.public(), sig_a), (B, sig_b))
                        .expect("sig_{a,b} to be valid signatures for tx_cancel")
                };

                co.yield_(Action::PunishBtc(signed_tx_punish)).await;

                let _ = bitcoin_client
                    .watch_for_raw_transaction(tx_punish_txid)
                    .await;
            }
        }
    })
}

// There are no guarantees that send_message and receive_massage do not block
// the flow of execution. Therefore they must be paired between Alice/Bob, one
// send to one receive in the correct order.
pub async fn next_state<
    R: RngCore + CryptoRng,
    B: WatchForRawTransaction + BroadcastSignedTransaction,
    M: CreateWalletForOutput + Transfer,
    T: SendMessage<Message> + ReceiveMessage<bob::Message>,
>(
    bitcoin_wallet: &B,
    monero_wallet: &M,
    transport: &mut T,
    state: State,
    rng: &mut R,
) -> Result<State> {
    match state {
        State::State0(state0) => {
            let alice_message0 = state0.next_message(rng).into();

            let bob_message0 = transport.receive_message().await?.try_into()?;
            transport.send_message(alice_message0).await?;

            let state1 = state0.receive(bob_message0)?;
            Ok(state1.into())
        }
        State::State1(state1) => {
            let bob_message1 = transport.receive_message().await?.try_into()?;
            let state2 = state1.receive(bob_message1);
            let alice_message1 = state2.next_message();
            transport.send_message(alice_message1.into()).await?;
            Ok(state2.into())
        }
        State::State2(state2) => {
            let bob_message2 = transport.receive_message().await?.try_into()?;
            let state3 = state2.receive(bob_message2)?;
            Ok(state3.into())
        }
        State::State3(state3) => {
            tracing::info!("alice is watching for locked btc");
            let state4 = state3.watch_for_lock_btc(bitcoin_wallet).await?;
            Ok(state4.into())
        }
        State::State4(state4) => {
            let state5 = state4.lock_xmr(monero_wallet).await?;
            tracing::info!("alice has locked xmr");
            Ok(state5.into())
        }
        State::State5(state5) => {
            transport.send_message(state5.next_message().into()).await?;
            // todo: pass in state4b as a parameter somewhere in this call to prevent the
            // user from waiting for a message that wont be sent
            let message3 = transport.receive_message().await?.try_into()?;
            let state6 = state5.receive(message3);
            tracing::info!("alice has received bob message 3");
            tracing::info!("alice is redeeming btc");
            state6.redeem_btc(bitcoin_wallet).await?;
            Ok(state6.into())
        }
        State::State6(state6) => Ok(state6.into()),
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Deserialize, Serialize)]
pub enum State {
    State0(State0),
    State1(State1),
    State2(State2),
    State3(State3),
    State4(State4),
    State5(State5),
    State6(State6),
}

impl_try_from_parent_enum!(State0, State);
impl_try_from_parent_enum!(State1, State);
impl_try_from_parent_enum!(State2, State);
impl_try_from_parent_enum!(State3, State);
impl_try_from_parent_enum!(State4, State);
impl_try_from_parent_enum!(State5, State);
impl_try_from_parent_enum!(State6, State);

impl_from_child_enum!(State0, State);
impl_from_child_enum!(State1, State);
impl_from_child_enum!(State2, State);
impl_from_child_enum!(State3, State);
impl_from_child_enum!(State4, State);
impl_from_child_enum!(State5, State);
impl_from_child_enum!(State6, State);

impl State {
    pub fn new<R: RngCore + CryptoRng>(
        rng: &mut R,
        btc: bitcoin::Amount,
        xmr: monero::Amount,
        refund_timelock: u32,
        punish_timelock: u32,
        redeem_address: bitcoin::Address,
        punish_address: bitcoin::Address,
    ) -> Self {
        Self::State0(State0::new(
            rng,
            btc,
            xmr,
            refund_timelock,
            punish_timelock,
            redeem_address,
            punish_address,
        ))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State0 {
    a: bitcoin::SecretKey,
    s_a: cross_curve_dleq::Scalar,
    v_a: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    refund_timelock: u32,
    punish_timelock: u32,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
}

impl State0 {
    pub fn new<R: RngCore + CryptoRng>(
        rng: &mut R,
        btc: bitcoin::Amount,
        xmr: monero::Amount,
        refund_timelock: u32,
        punish_timelock: u32,
        redeem_address: bitcoin::Address,
        punish_address: bitcoin::Address,
    ) -> Self {
        let a = bitcoin::SecretKey::new_random(rng);

        let s_a = cross_curve_dleq::Scalar::random(rng);
        let v_a = monero::PrivateViewKey::new_random(rng);

        Self {
            a,
            s_a,
            v_a,
            redeem_address,
            punish_address,
            btc,
            xmr,
            refund_timelock,
            punish_timelock,
        }
    }

    pub fn next_message<R: RngCore + CryptoRng>(&self, rng: &mut R) -> Message0 {
        info!("Producing first message");
        let dleq_proof_s_a = cross_curve_dleq::Proof::new(rng, &self.s_a);

        Message0 {
            A: self.a.public(),
            S_a_monero: monero::PublicKey::from_private_key(&monero::PrivateKey {
                scalar: self.s_a.into_ed25519(),
            }),
            S_a_bitcoin: self.s_a.into_secp256k1().into(),
            dleq_proof_s_a,
            v_a: self.v_a,
            redeem_address: self.redeem_address.clone(),
            punish_address: self.punish_address.clone(),
        }
    }

    pub fn receive(self, msg: bob::Message0) -> Result<State1> {
        msg.dleq_proof_s_b.verify(
            msg.S_b_bitcoin.clone().into(),
            msg.S_b_monero
                .point
                .decompress()
                .ok_or_else(|| anyhow!("S_b is not a monero curve point"))?,
        )?;

        let v = self.v_a + msg.v_b;

        Ok(State1 {
            a: self.a,
            B: msg.B,
            s_a: self.s_a,
            S_b_monero: msg.S_b_monero,
            S_b_bitcoin: msg.S_b_bitcoin,
            v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: msg.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State1 {
    a: bitcoin::SecretKey,
    B: bitcoin::PublicKey,
    s_a: cross_curve_dleq::Scalar,
    S_b_monero: monero::PublicKey,
    S_b_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    refund_timelock: u32,
    punish_timelock: u32,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
}

impl State1 {
    pub fn receive(self, msg: bob::Message1) -> State2 {
        State2 {
            a: self.a,
            B: self.B,
            s_a: self.s_a,
            S_b_monero: self.S_b_monero,
            S_b_bitcoin: self.S_b_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: msg.tx_lock,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State2 {
    a: bitcoin::SecretKey,
    B: bitcoin::PublicKey,
    s_a: cross_curve_dleq::Scalar,
    S_b_monero: monero::PublicKey,
    S_b_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    refund_timelock: u32,
    punish_timelock: u32,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
}

impl State2 {
    pub fn next_message(&self) -> Message1 {
        let tx_cancel = bitcoin::TxCancel::new(
            &self.tx_lock,
            self.refund_timelock,
            self.a.public(),
            self.B.clone(),
        );

        let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &self.refund_address);
        // Alice encsigns the refund transaction(bitcoin) digest with Bob's monero
        // pubkey(S_b). The refund transaction spends the output of
        // tx_lock_bitcoin to Bob's refund address.
        // recover(encsign(a, S_b, d), sign(a, d), S_b) = s_b where d is a digest, (a,
        // A) is alice's keypair and (s_b, S_b) is bob's keypair.
        let tx_refund_encsig = self.a.encsign(self.S_b_bitcoin.clone(), tx_refund.digest());

        let tx_cancel_sig = self.a.sign(tx_cancel.digest());
        Message1 {
            tx_refund_encsig,
            tx_cancel_sig,
        }
    }

    pub fn receive(self, msg: bob::Message2) -> Result<State3> {
        let tx_cancel = bitcoin::TxCancel::new(
            &self.tx_lock,
            self.refund_timelock,
            self.a.public(),
            self.B.clone(),
        );
        bitcoin::verify_sig(&self.B, &tx_cancel.digest(), &msg.tx_cancel_sig)?;
        let tx_punish =
            bitcoin::TxPunish::new(&tx_cancel, &self.punish_address, self.punish_timelock);
        bitcoin::verify_sig(&self.B, &tx_punish.digest(), &msg.tx_punish_sig)?;

        Ok(State3 {
            a: self.a,
            B: self.B,
            s_a: self.s_a,
            S_b_monero: self.S_b_monero,
            S_b_bitcoin: self.S_b_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_punish_sig_bob: msg.tx_punish_sig,
            tx_cancel_sig_bob: msg.tx_cancel_sig,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct State3 {
    pub a: bitcoin::SecretKey,
    pub B: bitcoin::PublicKey,
    pub s_a: cross_curve_dleq::Scalar,
    pub S_b_monero: monero::PublicKey,
    pub S_b_bitcoin: bitcoin::PublicKey,
    pub v: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc: bitcoin::Amount,
    pub xmr: monero::Amount,
    pub refund_timelock: u32,
    pub punish_timelock: u32,
    pub refund_address: bitcoin::Address,
    pub redeem_address: bitcoin::Address,
    pub punish_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    pub tx_punish_sig_bob: bitcoin::Signature,
    pub tx_cancel_sig_bob: bitcoin::Signature,
}

impl State3 {
    pub async fn watch_for_lock_btc<W>(self, bitcoin_wallet: &W) -> Result<State4>
    where
        W: bitcoin::WatchForRawTransaction,
    {
        tracing::info!("watching for lock btc with txid: {}", self.tx_lock.txid());
        let tx = bitcoin_wallet
            .watch_for_raw_transaction(self.tx_lock.txid())
            .await;

        tracing::info!("tx lock seen with txid: {}", tx.txid());

        Ok(State4 {
            a: self.a,
            B: self.B,
            s_a: self.s_a,
            S_b_monero: self.S_b_monero,
            S_b_bitcoin: self.S_b_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_punish_sig_bob: self.tx_punish_sig_bob,
            tx_cancel_sig_bob: self.tx_cancel_sig_bob,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State4 {
    a: bitcoin::SecretKey,
    B: bitcoin::PublicKey,
    s_a: cross_curve_dleq::Scalar,
    S_b_monero: monero::PublicKey,
    S_b_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    refund_timelock: u32,
    punish_timelock: u32,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
    tx_punish_sig_bob: bitcoin::Signature,
    tx_cancel_sig_bob: bitcoin::Signature,
}

impl State4 {
    pub async fn lock_xmr<W>(self, monero_wallet: &W) -> Result<State5>
    where
        W: monero::Transfer,
    {
        let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey {
            scalar: self.s_a.into_ed25519(),
        });
        let S_b = self.S_b_monero;

        let (tx_lock_proof, fee) = monero_wallet
            .transfer(S_a + S_b, self.v.public(), self.xmr)
            .await?;

        Ok(State5 {
            a: self.a,
            B: self.B,
            s_a: self.s_a,
            S_b_monero: self.S_b_monero,
            S_b_bitcoin: self.S_b_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_lock_proof,
            tx_punish_sig_bob: self.tx_punish_sig_bob,
            tx_cancel_sig_bob: self.tx_cancel_sig_bob,
            lock_xmr_fee: fee,
        })
    }

    pub async fn punish<W: bitcoin::BroadcastSignedTransaction>(
        &self,
        bitcoin_wallet: &W,
    ) -> Result<()> {
        let tx_cancel = bitcoin::TxCancel::new(
            &self.tx_lock,
            self.refund_timelock,
            self.a.public(),
            self.B.clone(),
        );
        let tx_punish =
            bitcoin::TxPunish::new(&tx_cancel, &self.punish_address, self.punish_timelock);

        {
            let sig_a = self.a.sign(tx_cancel.digest());
            let sig_b = self.tx_cancel_sig_bob.clone();

            let signed_tx_cancel = tx_cancel.clone().add_signatures(
                &self.tx_lock,
                (self.a.public(), sig_a),
                (self.B.clone(), sig_b),
            )?;

            let _ = bitcoin_wallet
                .broadcast_signed_transaction(signed_tx_cancel)
                .await?;
        }

        {
            let sig_a = self.a.sign(tx_punish.digest());
            let sig_b = self.tx_punish_sig_bob.clone();

            let signed_tx_punish = tx_punish.add_signatures(
                &tx_cancel,
                (self.a.public(), sig_a),
                (self.B.clone(), sig_b),
            )?;

            let _ = bitcoin_wallet
                .broadcast_signed_transaction(signed_tx_punish)
                .await?;
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State5 {
    a: bitcoin::SecretKey,
    B: bitcoin::PublicKey,
    s_a: cross_curve_dleq::Scalar,
    S_b_monero: monero::PublicKey,
    S_b_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    refund_timelock: u32,
    punish_timelock: u32,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
    tx_lock_proof: monero::TransferProof,

    tx_punish_sig_bob: bitcoin::Signature,

    tx_cancel_sig_bob: bitcoin::Signature,
    lock_xmr_fee: monero::Amount,
}

impl State5 {
    pub fn next_message(&self) -> Message2 {
        Message2 {
            tx_lock_proof: self.tx_lock_proof.clone(),
        }
    }

    pub fn receive(self, msg: bob::Message3) -> State6 {
        State6 {
            a: self.a,
            B: self.B,
            s_a: self.s_a,
            S_b_monero: self.S_b_monero,
            S_b_bitcoin: self.S_b_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_punish_sig_bob: self.tx_punish_sig_bob,
            tx_redeem_encsig: msg.tx_redeem_encsig,
            lock_xmr_fee: self.lock_xmr_fee,
        }
    }

    // watch for refund on btc, recover s_b and refund xmr
    pub async fn refund_xmr<B, M>(self, bitcoin_wallet: &B, monero_wallet: &M) -> Result<()>
    where
        B: WatchForRawTransaction,
        M: CreateWalletForOutput,
    {
        let tx_cancel = bitcoin::TxCancel::new(
            &self.tx_lock,
            self.refund_timelock,
            self.a.public(),
            self.B.clone(),
        );

        let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &self.refund_address);

        let tx_refund_encsig = self.a.encsign(self.S_b_bitcoin.clone(), tx_refund.digest());

        let tx_refund_candidate = bitcoin_wallet
            .watch_for_raw_transaction(tx_refund.txid())
            .await;

        let tx_refund_sig =
            tx_refund.extract_signature_by_key(tx_refund_candidate, self.a.public())?;

        let s_b = bitcoin::recover(self.S_b_bitcoin, tx_refund_sig, tx_refund_encsig)?;
        let s_b = monero::private_key_from_secp256k1_scalar(s_b.into());

        let s = s_b.scalar + self.s_a.into_ed25519();

        // NOTE: This actually generates and opens a new wallet, closing the currently
        // open one.
        monero_wallet
            .create_and_load_wallet_for_output(monero::PrivateKey::from_scalar(s), self.v)
            .await?;

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State6 {
    a: bitcoin::SecretKey,
    B: bitcoin::PublicKey,
    s_a: cross_curve_dleq::Scalar,
    S_b_monero: monero::PublicKey,
    S_b_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    refund_timelock: u32,
    punish_timelock: u32,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,

    tx_punish_sig_bob: bitcoin::Signature,
    tx_redeem_encsig: EncryptedSignature,
    lock_xmr_fee: monero::Amount,
}

impl State6 {
    pub async fn redeem_btc<W: bitcoin::BroadcastSignedTransaction>(
        &self,
        bitcoin_wallet: &W,
    ) -> Result<()> {
        let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

        let tx_redeem = bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address);

        let sig_a = self.a.sign(tx_redeem.digest());
        let sig_b =
            adaptor.decrypt_signature(&self.s_a.into_secp256k1(), self.tx_redeem_encsig.clone());

        let sig_tx_redeem = tx_redeem.add_signatures(
            &self.tx_lock,
            (self.a.public(), sig_a),
            (self.B.clone(), sig_b),
        )?;
        bitcoin_wallet
            .broadcast_signed_transaction(sig_tx_redeem)
            .await?;

        Ok(())
    }

    pub fn lock_xmr_fee(&self) -> monero::Amount {
        self.lock_xmr_fee
    }
}
