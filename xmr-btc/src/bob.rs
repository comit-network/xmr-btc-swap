use crate::{
    alice,
    bitcoin::{
        self, poll_until_block_height_is_gte, BroadcastSignedTransaction, BuildTxLockPsbt,
        SignTxLock, TxCancel, WatchForRawTransaction,
    },
    monero,
    serde::monero_private_key,
    transport::{ReceiveMessage, SendMessage},
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use ecdsa_fun::{
    adaptor::{Adaptor, EncryptedSignature},
    nonce::Deterministic,
    Signature,
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
use tracing::error;

pub mod message;
use crate::monero::{CreateWalletForOutput, WatchForTransfer};
pub use message::{Message, Message0, Message1, Message2, Message3};

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Action {
    LockBtc(bitcoin::TxLock),
    SendBtcRedeemEncsig(bitcoin::EncryptedSignature),
    CreateXmrWalletForOutput {
        spend_key: monero::PrivateKey,
        view_key: monero::PrivateViewKey,
    },
    CancelBtc(bitcoin::Transaction),
    RefundBtc(bitcoin::Transaction),
}

// TODO: This could be moved to the monero module
#[async_trait]
pub trait ReceiveTransferProof {
    async fn receive_transfer_proof(&mut self) -> monero::TransferProof;
}

/// Perform the on-chain protocol to swap monero and bitcoin as Bob.
///
/// This is called post handshake, after all the keys, addresses and most of the
/// signatures have been exchanged.
///
/// The argument `bitcoin_tx_lock_timeout` is used to determine how long we will
/// wait for Bob, the caller of this function, to lock up the bitcoin.
pub fn action_generator<N, M, B>(
    network: Arc<Mutex<N>>,
    monero_client: Arc<M>,
    bitcoin_client: Arc<B>,
    // TODO: Replace this with a new, slimmer struct?
    State2 {
        A,
        b,
        s_b,
        S_a_monero,
        S_a_bitcoin,
        v,
        xmr,
        refund_timelock,
        redeem_address,
        refund_address,
        tx_lock,
        tx_cancel_sig_a,
        tx_refund_encsig,
        ..
    }: State2,
    bitcoin_tx_lock_timeout: u64,
) -> GenBoxed<Action, (), ()>
where
    N: ReceiveTransferProof + Send + 'static,
    M: monero::WatchForTransfer + Send + Sync + 'static,
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
        AfterBtcLock(Reason),
        AfterBtcRedeem(Reason),
    }

    /// Reason why the swap has failed.
    #[derive(Debug)]
    enum Reason {
        /// Bob was too slow to lock the bitcoin.
        InactiveBob,
        /// The refund timelock has been reached.
        BtcExpired,
        /// Alice did not lock up enough monero in the shared output.
        InsufficientXmr(monero::InsufficientFunds),
        /// Could not find Bob's signature on the redeem transaction witness
        /// stack.
        BtcRedeemSignature,
        /// Could not recover secret `s_a` from Bob's redeem transaction
        /// signature.
        SecretRecovery,
    }

    Gen::new_boxed(|co| async move {
        let swap_result: Result<(), SwapFailed> = async {
            co.yield_(Action::LockBtc(tx_lock.clone())).await;

            timeout(
                Duration::from_secs(bitcoin_tx_lock_timeout),
                bitcoin_client.watch_for_raw_transaction(tx_lock.txid()),
            )
            .await
            .map(|tx| tx.txid())
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

            let transfer_proof = {
                let mut guard = network.as_ref().lock().await;
                let transfer_proof = match select(
                    guard.receive_transfer_proof(),
                    poll_until_btc_has_expired.clone(),
                )
                .await
                {
                    Either::Left((proof, _)) => proof,
                    Either::Right(_) => return Err(SwapFailed::AfterBtcLock(Reason::BtcExpired)),
                };

                tracing::debug!("select returned transfer proof from message");

                transfer_proof
            };

            let S_b_monero = monero::PublicKey::from_private_key(&monero::PrivateKey::from_scalar(
                s_b.into_ed25519(),
            ));
            let S = S_a_monero + S_b_monero;

            match select(
                monero_client.watch_for_transfer(S, v.public(), transfer_proof, xmr, 0),
                poll_until_btc_has_expired.clone(),
            )
            .await
            {
                Either::Left((Err(e), _)) => {
                    return Err(SwapFailed::AfterBtcLock(Reason::InsufficientXmr(e)))
                }
                Either::Right(_) => return Err(SwapFailed::AfterBtcLock(Reason::BtcExpired)),
                _ => {}
            }

            let tx_redeem = bitcoin::TxRedeem::new(&tx_lock, &redeem_address);
            let tx_redeem_encsig = b.encsign(S_a_bitcoin.clone(), tx_redeem.digest());

            co.yield_(Action::SendBtcRedeemEncsig(tx_redeem_encsig.clone()))
                .await;

            let tx_redeem_published = match select(
                bitcoin_client.watch_for_raw_transaction(tx_redeem.txid()),
                poll_until_btc_has_expired,
            )
            .await
            {
                Either::Left((tx, _)) => tx,
                Either::Right(_) => return Err(SwapFailed::AfterBtcLock(Reason::BtcExpired)),
            };

            let tx_redeem_sig = tx_redeem
                .extract_signature_by_key(tx_redeem_published, b.public())
                .map_err(|_| SwapFailed::AfterBtcRedeem(Reason::BtcRedeemSignature))?;
            let s_a = bitcoin::recover(S_a_bitcoin, tx_redeem_sig, tx_redeem_encsig)
                .map_err(|_| SwapFailed::AfterBtcRedeem(Reason::SecretRecovery))?;
            let s_a = monero::private_key_from_secp256k1_scalar(s_a.into());

            let s_b = monero::PrivateKey {
                scalar: s_b.into_ed25519(),
            };

            co.yield_(Action::CreateXmrWalletForOutput {
                spend_key: s_a + s_b,
                view_key: v,
            })
            .await;

            Ok(())
        }
        .await;

        if let Err(ref err) = swap_result {
            error!("swap failed: {:?}", err);
        }

        if let Err(SwapFailed::AfterBtcLock(_)) = swap_result {
            let tx_cancel =
                bitcoin::TxCancel::new(&tx_lock, refund_timelock, A.clone(), b.public());
            let tx_cancel_txid = tx_cancel.txid();
            let signed_tx_cancel = {
                let sig_a = tx_cancel_sig_a.clone();
                let sig_b = b.sign(tx_cancel.digest());

                tx_cancel
                    .clone()
                    .add_signatures(&tx_lock, (A.clone(), sig_a), (b.public(), sig_b))
                    .expect("sig_{a,b} to be valid signatures for tx_cancel")
            };

            co.yield_(Action::CancelBtc(signed_tx_cancel)).await;

            let _ = bitcoin_client
                .watch_for_raw_transaction(tx_cancel_txid)
                .await;

            let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &refund_address);
            let tx_refund_txid = tx_refund.txid();
            let signed_tx_refund = {
                let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

                let sig_a =
                    adaptor.decrypt_signature(&s_b.into_secp256k1(), tx_refund_encsig.clone());
                let sig_b = b.sign(tx_refund.digest());

                tx_refund
                    .add_signatures(&tx_cancel, (A.clone(), sig_a), (b.public(), sig_b))
                    .expect("sig_{a,b} to be valid signatures for tx_refund")
            };

            co.yield_(Action::RefundBtc(signed_tx_refund)).await;

            let _ = bitcoin_client
                .watch_for_raw_transaction(tx_refund_txid)
                .await;
        }
    })
}

// There are no guarantees that send_message and receive_massage do not block
// the flow of execution. Therefore they must be paired between Alice/Bob, one
// send to one receive in the correct order.
pub async fn next_state<
    R: RngCore + CryptoRng,
    B: WatchForRawTransaction + SignTxLock + BuildTxLockPsbt + BroadcastSignedTransaction,
    M: CreateWalletForOutput + WatchForTransfer,
    T: SendMessage<Message> + ReceiveMessage<alice::Message>,
>(
    bitcoin_wallet: &B,
    monero_wallet: &M,
    transport: &mut T,
    state: State,
    rng: &mut R,
) -> Result<State> {
    match state {
        State::State0(state0) => {
            transport
                .send_message(state0.next_message(rng).into())
                .await?;
            let message0 = transport.receive_message().await?.try_into()?;
            let state1 = state0.receive(bitcoin_wallet, message0).await?;
            Ok(state1.into())
        }
        State::State1(state1) => {
            transport.send_message(state1.next_message().into()).await?;

            let message1 = transport.receive_message().await?.try_into()?;
            let state2 = state1.receive(message1)?;

            let message2 = state2.next_message();
            transport.send_message(message2.into()).await?;
            Ok(state2.into())
        }
        State::State2(state2) => {
            let state3 = state2.lock_btc(bitcoin_wallet).await?;
            tracing::info!("bob has locked btc");

            Ok(state3.into())
        }
        State::State3(state3) => {
            let message2 = transport.receive_message().await?.try_into()?;
            let state4 = state3.watch_for_lock_xmr(monero_wallet, message2).await?;
            tracing::info!("bob has seen that alice has locked xmr");
            Ok(state4.into())
        }
        State::State4(state4) => {
            transport.send_message(state4.next_message().into()).await?;
            tracing::info!("bob is watching for redeem_btc");
            let state5 = state4.watch_for_redeem_btc(bitcoin_wallet).await?;
            tracing::info!("bob has seen that alice has redeemed btc");
            state5.claim_xmr(monero_wallet).await?;
            tracing::info!("bob has claimed xmr");
            Ok(state5.into())
        }
        State::State5(state5) => Ok(state5.into()),
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub enum State {
    State0(State0),
    State1(State1),
    State2(State2),
    State3(State3),
    State4(State4),
    State5(State5),
}

impl_try_from_parent_enum!(State0, State);
impl_try_from_parent_enum!(State1, State);
impl_try_from_parent_enum!(State2, State);
impl_try_from_parent_enum!(State3, State);
impl_try_from_parent_enum!(State4, State);
impl_try_from_parent_enum!(State5, State);

impl_from_child_enum!(State0, State);
impl_from_child_enum!(State1, State);
impl_from_child_enum!(State2, State);
impl_from_child_enum!(State3, State);
impl_from_child_enum!(State4, State);
impl_from_child_enum!(State5, State);

#[derive(Debug, Deserialize, Serialize)]
pub struct State0 {
    b: bitcoin::SecretKey,
    s_b: cross_curve_dleq::Scalar,
    v_b: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    refund_timelock: u32,
    punish_timelock: u32,
    refund_address: bitcoin::Address,
}

impl State0 {
    pub fn new<R: RngCore + CryptoRng>(
        rng: &mut R,
        btc: bitcoin::Amount,
        xmr: monero::Amount,
        refund_timelock: u32,
        punish_timelock: u32,
        refund_address: bitcoin::Address,
    ) -> Self {
        let b = bitcoin::SecretKey::new_random(rng);

        let s_b = cross_curve_dleq::Scalar::random(rng);
        let v_b = monero::PrivateViewKey::new_random(rng);

        Self {
            b,
            s_b,
            v_b,
            btc,
            xmr,
            refund_timelock,
            punish_timelock,
            refund_address,
        }
    }

    pub fn next_message<R: RngCore + CryptoRng>(&self, rng: &mut R) -> Message0 {
        let dleq_proof_s_b = cross_curve_dleq::Proof::new(rng, &self.s_b);

        Message0 {
            B: self.b.public(),
            S_b_monero: monero::PublicKey::from_private_key(&monero::PrivateKey {
                scalar: self.s_b.into_ed25519(),
            }),
            S_b_bitcoin: self.s_b.into_secp256k1().into(),
            dleq_proof_s_b,
            v_b: self.v_b,
            refund_address: self.refund_address.clone(),
        }
    }

    pub async fn receive<W>(self, wallet: &W, msg: alice::Message0) -> anyhow::Result<State1>
    where
        W: BuildTxLockPsbt,
    {
        msg.dleq_proof_s_a.verify(
            msg.S_a_bitcoin.clone().into(),
            msg.S_a_monero
                .point
                .decompress()
                .ok_or_else(|| anyhow!("S_a is not a monero curve point"))?,
        )?;

        let tx_lock =
            bitcoin::TxLock::new(wallet, self.btc, msg.A.clone(), self.b.public()).await?;
        let v = msg.v_a + self.v_b;

        Ok(State1 {
            A: msg.A,
            b: self.b,
            s_b: self.s_b,
            S_a_monero: msg.S_a_monero,
            S_a_bitcoin: msg.S_a_bitcoin,
            v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: msg.redeem_address,
            punish_address: msg.punish_address,
            tx_lock,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct State1 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
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

impl State1 {
    pub fn next_message(&self) -> Message1 {
        Message1 {
            tx_lock: self.tx_lock.clone(),
        }
    }

    pub fn receive(self, msg: alice::Message1) -> Result<State2> {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.refund_timelock,
            self.A.clone(),
            self.b.public(),
        );
        let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &self.refund_address);

        bitcoin::verify_sig(&self.A, &tx_cancel.digest(), &msg.tx_cancel_sig)?;
        bitcoin::verify_encsig(
            self.A.clone(),
            self.s_b.into_secp256k1().into(),
            &tx_refund.digest(),
            &msg.tx_refund_encsig,
        )?;

        Ok(State2 {
            A: self.A,
            b: self.b,
            s_b: self.s_b,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: msg.tx_cancel_sig,
            tx_refund_encsig: msg.tx_refund_encsig,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct State2 {
    pub A: bitcoin::PublicKey,
    pub b: bitcoin::SecretKey,
    pub s_b: cross_curve_dleq::Scalar,
    pub S_a_monero: monero::PublicKey,
    pub S_a_bitcoin: bitcoin::PublicKey,
    pub v: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    pub xmr: monero::Amount,
    pub refund_timelock: u32,
    pub punish_timelock: u32,
    pub refund_address: bitcoin::Address,
    pub redeem_address: bitcoin::Address,
    pub punish_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    pub tx_cancel_sig_a: Signature,
    pub tx_refund_encsig: EncryptedSignature,
}

impl State2 {
    pub fn next_message(&self) -> Message2 {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.refund_timelock,
            self.A.clone(),
            self.b.public(),
        );
        let tx_cancel_sig = self.b.sign(tx_cancel.digest());
        let tx_punish =
            bitcoin::TxPunish::new(&tx_cancel, &self.punish_address, self.punish_timelock);
        let tx_punish_sig = self.b.sign(tx_punish.digest());

        Message2 {
            tx_punish_sig,
            tx_cancel_sig,
        }
    }

    pub async fn lock_btc<W>(self, bitcoin_wallet: &W) -> Result<State3>
    where
        W: bitcoin::SignTxLock + bitcoin::BroadcastSignedTransaction,
    {
        let signed_tx_lock = bitcoin_wallet.sign_tx_lock(self.tx_lock.clone()).await?;

        tracing::info!("{}", self.tx_lock.txid());
        let _ = bitcoin_wallet
            .broadcast_signed_transaction(signed_tx_lock)
            .await?;

        Ok(State3 {
            A: self.A,
            b: self.b,
            s_b: self.s_b,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: self.tx_cancel_sig_a,
            tx_refund_encsig: self.tx_refund_encsig,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct State3 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
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
    tx_cancel_sig_a: Signature,
    tx_refund_encsig: EncryptedSignature,
}

impl State3 {
    pub async fn watch_for_lock_xmr<W>(self, xmr_wallet: &W, msg: alice::Message2) -> Result<State4>
    where
        W: monero::WatchForTransfer,
    {
        let S_b_monero = monero::PublicKey::from_private_key(&monero::PrivateKey::from_scalar(
            self.s_b.into_ed25519(),
        ));
        let S = self.S_a_monero + S_b_monero;

        xmr_wallet
            .watch_for_transfer(
                S,
                self.v.public(),
                msg.tx_lock_proof,
                self.xmr,
                monero::MIN_CONFIRMATIONS,
            )
            .await?;

        Ok(State4 {
            A: self.A,
            b: self.b,
            s_b: self.s_b,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: self.tx_cancel_sig_a,
            tx_refund_encsig: self.tx_refund_encsig,
        })
    }

    pub async fn refund_btc<W: bitcoin::BroadcastSignedTransaction>(
        &self,
        bitcoin_wallet: &W,
    ) -> Result<()> {
        let tx_cancel = bitcoin::TxCancel::new(
            &self.tx_lock,
            self.refund_timelock,
            self.A.clone(),
            self.b.public(),
        );
        let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &self.refund_address);

        {
            let sig_b = self.b.sign(tx_cancel.digest());
            let sig_a = self.tx_cancel_sig_a.clone();

            let signed_tx_cancel = tx_cancel.clone().add_signatures(
                &self.tx_lock,
                (self.A.clone(), sig_a),
                (self.b.public(), sig_b),
            )?;

            let _ = bitcoin_wallet
                .broadcast_signed_transaction(signed_tx_cancel)
                .await?;
        }

        {
            let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

            let sig_b = self.b.sign(tx_refund.digest());
            let sig_a = adaptor
                .decrypt_signature(&self.s_b.into_secp256k1(), self.tx_refund_encsig.clone());

            let signed_tx_refund = tx_refund.add_signatures(
                &tx_cancel.clone(),
                (self.A.clone(), sig_a),
                (self.b.public(), sig_b),
            )?;

            let _ = bitcoin_wallet
                .broadcast_signed_transaction(signed_tx_refund)
                .await?;
        }
        Ok(())
    }
    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct State4 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
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
    tx_cancel_sig_a: Signature,
    tx_refund_encsig: EncryptedSignature,
}

impl State4 {
    pub fn next_message(&self) -> Message3 {
        let tx_redeem = bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address);
        let tx_redeem_encsig = self.b.encsign(self.S_a_bitcoin.clone(), tx_redeem.digest());

        Message3 { tx_redeem_encsig }
    }

    pub async fn watch_for_redeem_btc<W>(self, bitcoin_wallet: &W) -> Result<State5>
    where
        W: WatchForRawTransaction,
    {
        let tx_redeem = bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address);
        let tx_redeem_encsig = self.b.encsign(self.S_a_bitcoin.clone(), tx_redeem.digest());

        let tx_redeem_candidate = bitcoin_wallet
            .watch_for_raw_transaction(tx_redeem.txid())
            .await;

        let tx_redeem_sig =
            tx_redeem.extract_signature_by_key(tx_redeem_candidate, self.b.public())?;
        let s_a = bitcoin::recover(self.S_a_bitcoin.clone(), tx_redeem_sig, tx_redeem_encsig)?;
        let s_a = monero::private_key_from_secp256k1_scalar(s_a.into());

        Ok(State5 {
            A: self.A,
            b: self.b,
            s_a,
            s_b: self.s_b,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_refund_encsig: self.tx_refund_encsig,
            tx_cancel_sig: self.tx_cancel_sig_a,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct State5 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    #[serde(with = "monero_private_key")]
    s_a: monero::PrivateKey,
    s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
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
    tx_refund_encsig: EncryptedSignature,
    tx_cancel_sig: Signature,
}

impl State5 {
    pub async fn claim_xmr<W>(&self, monero_wallet: &W) -> Result<()>
    where
        W: monero::CreateWalletForOutput,
    {
        let s_b = monero::PrivateKey {
            scalar: self.s_b.into_ed25519(),
        };

        let s = self.s_a + s_b;

        // NOTE: This actually generates and opens a new wallet, closing the currently
        // open one.
        monero_wallet
            .create_and_load_wallet_for_output(s, self.v)
            .await?;

        Ok(())
    }
    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }
}
