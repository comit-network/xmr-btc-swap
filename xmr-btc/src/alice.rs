use crate::{bitcoin, bitcoin::WatchForRawTransaction, bob, monero, monero::CreateWalletForOutput};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use ecdsa_fun::{
    adaptor::{Adaptor, EncryptedSignature},
    nonce::Deterministic,
};
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::convert::TryFrom;
use tracing::info;

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
        let a = bitcoin::SecretKey::new_random(rng);
        let s_a = cross_curve_dleq::Scalar::random(rng);
        let v_a = monero::PrivateViewKey::new_random(rng);

        Self::State0(State0::new(
            a,
            s_a,
            v_a,
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        a: bitcoin::SecretKey,
        s_a: cross_curve_dleq::Scalar,
        v_a: monero::PrivateViewKey,
        btc: bitcoin::Amount,
        xmr: monero::Amount,
        refund_timelock: u32,
        punish_timelock: u32,
        redeem_address: bitcoin::Address,
        punish_address: bitcoin::Address,
    ) -> Self {
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
