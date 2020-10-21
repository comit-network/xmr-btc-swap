use crate::{
    alice,
    bitcoin::{
        self, BroadcastSignedTransaction, BuildTxLockPsbt, SignTxLock, TxCancel,
        WatchForRawTransaction,
    },
    monero,
    monero::{CheckTransfer, CreateWalletForOutput, WatchForTransfer},
    serde::{bitcoin_amount, cross_curve_dleq_scalar, monero_private_key},
    transport::{ReceiveMessage, SendMessage},
};
use anyhow::{anyhow, Result};
use ecdsa_fun::{
    adaptor::{Adaptor, EncryptedSignature},
    nonce::Deterministic,
    Signature,
};
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::convert::{TryFrom, TryInto};

pub mod message;
pub use message::{Message, Message0, Message1, Message2, Message3};

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
            Ok(state2.into())
        }
        State::State2(state2) => {
            let message2 = state2.next_message();
            let state3 = state2.lock_btc(bitcoin_wallet).await?;
            tracing::info!("bob has locked btc");
            transport.send_message(message2.into()).await?;
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

#[derive(Debug)]
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
    #[serde(with = "cross_curve_dleq_scalar")]
    s_b: cross_curve_dleq::Scalar,
    v_b: monero::PrivateViewKey,
    #[serde(with = "bitcoin_amount")]
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
    #[serde(with = "cross_curve_dleq_scalar")]
    s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    #[serde(with = "bitcoin_amount")]
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

#[derive(Debug, Deserialize, Serialize)]
pub struct State2 {
    pub A: bitcoin::PublicKey,
    pub b: bitcoin::SecretKey,
    #[serde(with = "cross_curve_dleq_scalar")]
    pub s_b: cross_curve_dleq::Scalar,
    pub S_a_monero: monero::PublicKey,
    pub S_a_bitcoin: bitcoin::PublicKey,
    pub v: monero::PrivateViewKey,
    #[serde(with = "bitcoin_amount")]
    btc: bitcoin::Amount,
    pub xmr: monero::Amount,
    pub refund_timelock: u32,
    punish_timelock: u32,
    pub refund_address: bitcoin::Address,
    pub redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
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
    #[serde(with = "cross_curve_dleq_scalar")]
    s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    #[serde(with = "bitcoin_amount")]
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
    #[serde(with = "cross_curve_dleq_scalar")]
    s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    #[serde(with = "bitcoin_amount")]
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
        let s_a =
            monero::PrivateKey::from_scalar(monero::Scalar::from_bytes_mod_order(s_a.to_bytes()));

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
    #[serde(with = "cross_curve_dleq_scalar")]
    s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    #[serde(with = "bitcoin_amount")]
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
