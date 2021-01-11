use anyhow::{anyhow, Result};
use ecdsa_fun::{
    adaptor::{Adaptor, EncryptedSignature},
    nonce::Deterministic,
    Signature,
};
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::fmt;

use crate::{
    bitcoin::{
        self, current_epoch, timelocks::Timelock, wait_for_cancel_timelock_to_expire,
        BroadcastSignedTransaction, BuildTxLockPsbt, GetBlockHeight, GetNetwork, GetRawTransaction,
        Transaction, TransactionBlockHeight, TxCancel, Txid, WatchForRawTransaction,
    },
    monero,
    monero::monero_private_key,
    protocol::{alice, bob, ExpiredTimelocks, SwapAmounts},
};

#[derive(Debug, Clone)]
pub enum BobState {
    Started {
        state0: State0,
        amounts: SwapAmounts,
    },
    Negotiated(State2),
    BtcLocked(State3),
    XmrLocked(State4),
    EncSigSent(State4),
    BtcRedeemed(State5),
    CancelTimelockExpired(State4),
    BtcCancelled(State4),
    BtcRefunded(State4),
    XmrRedeemed,
    BtcPunished,
    SafelyAborted,
}

impl fmt::Display for BobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BobState::Started { .. } => write!(f, "started"),
            BobState::Negotiated(..) => write!(f, "negotiated"),
            BobState::BtcLocked(..) => write!(f, "btc is locked"),
            BobState::XmrLocked(..) => write!(f, "xmr is locked"),
            BobState::EncSigSent(..) => write!(f, "encrypted signature is sent"),
            BobState::BtcRedeemed(..) => write!(f, "btc is redeemed"),
            BobState::CancelTimelockExpired(..) => write!(f, "cancel timelock is expired"),
            BobState::BtcCancelled(..) => write!(f, "btc is cancelled"),
            BobState::BtcRefunded(..) => write!(f, "btc is refunded"),
            BobState::XmrRedeemed => write!(f, "xmr is redeemed"),
            BobState::BtcPunished => write!(f, "btc is punished"),
            BobState::SafelyAborted => write!(f, "safely aborted"),
        }
    }
}

pub fn is_complete(state: &BobState) -> bool {
    matches!(
        state,
        BobState::BtcRefunded(..)
            | BobState::XmrRedeemed
            | BobState::BtcPunished
            | BobState::SafelyAborted
    )
}

pub fn is_btc_locked(state: &BobState) -> bool {
    matches!(state, BobState::BtcLocked(..))
}

pub fn is_xmr_locked(state: &BobState) -> bool {
    matches!(state, BobState::XmrLocked(..))
}

pub fn is_encsig_sent(state: &BobState) -> bool {
    matches!(state, BobState::EncSigSent(..))
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct State0 {
    b: bitcoin::SecretKey,
    s_b: cross_curve_dleq::Scalar,
    v_b: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    cancel_timelock: Timelock,
    punish_timelock: Timelock,
    refund_address: bitcoin::Address,
    min_monero_confirmations: u32,
}

impl State0 {
    pub fn new<R: RngCore + CryptoRng>(
        rng: &mut R,
        btc: bitcoin::Amount,
        xmr: monero::Amount,
        cancel_timelock: Timelock,
        punish_timelock: Timelock,
        refund_address: bitcoin::Address,
        min_monero_confirmations: u32,
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
            cancel_timelock,
            punish_timelock,
            refund_address,
            min_monero_confirmations,
        }
    }

    pub fn next_message<R: RngCore + CryptoRng>(&self, rng: &mut R) -> bob::Message0 {
        let dleq_proof_s_b = cross_curve_dleq::Proof::new(rng, &self.s_b);

        bob::Message0 {
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
        W: BuildTxLockPsbt + GetNetwork,
    {
        msg.dleq_proof_s_a.verify(
            msg.S_a_bitcoin.clone().into(),
            msg.S_a_monero
                .point
                .decompress()
                .ok_or_else(|| anyhow!("S_a is not a monero curve point"))?,
        )?;

        let tx_lock = bitcoin::TxLock::new(wallet, self.btc, msg.A, self.b.public()).await?;
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
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: msg.redeem_address,
            punish_address: msg.punish_address,
            tx_lock,
            min_monero_confirmations: self.min_monero_confirmations,
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
    cancel_timelock: Timelock,
    punish_timelock: Timelock,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
    min_monero_confirmations: u32,
}

impl State1 {
    pub fn next_message(&self) -> bob::Message1 {
        bob::Message1 {
            tx_lock: self.tx_lock.clone(),
        }
    }

    pub fn receive(self, msg: alice::Message1) -> Result<State2> {
        let tx_cancel = TxCancel::new(&self.tx_lock, self.cancel_timelock, self.A, self.b.public());
        let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &self.refund_address);

        bitcoin::verify_sig(&self.A, &tx_cancel.digest(), &msg.tx_cancel_sig)?;
        bitcoin::verify_encsig(
            self.A,
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
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: msg.tx_cancel_sig,
            tx_refund_encsig: msg.tx_refund_encsig,
            min_monero_confirmations: self.min_monero_confirmations,
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
    pub cancel_timelock: Timelock,
    pub punish_timelock: Timelock,
    pub refund_address: bitcoin::Address,
    pub redeem_address: bitcoin::Address,
    pub punish_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    pub tx_cancel_sig_a: Signature,
    pub tx_refund_encsig: EncryptedSignature,
    pub min_monero_confirmations: u32,
}

impl State2 {
    pub fn next_message(&self) -> bob::Message2 {
        let tx_cancel = TxCancel::new(&self.tx_lock, self.cancel_timelock, self.A, self.b.public());
        let tx_cancel_sig = self.b.sign(tx_cancel.digest());
        let tx_punish =
            bitcoin::TxPunish::new(&tx_cancel, &self.punish_address, self.punish_timelock);
        let tx_punish_sig = self.b.sign(tx_punish.digest());

        bob::Message2 {
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
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: self.tx_cancel_sig_a,
            tx_refund_encsig: self.tx_refund_encsig,
            min_monero_confirmations: self.min_monero_confirmations,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct State3 {
    pub A: bitcoin::PublicKey,
    pub b: bitcoin::SecretKey,
    pub s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    pub cancel_timelock: Timelock,
    punish_timelock: Timelock,
    pub refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    pub tx_cancel_sig_a: Signature,
    pub tx_refund_encsig: EncryptedSignature,
    pub min_monero_confirmations: u32,
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
                self.min_monero_confirmations,
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
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: self.tx_cancel_sig_a,
            tx_refund_encsig: self.tx_refund_encsig,
        })
    }

    pub async fn wait_for_cancel_timelock_to_expire<W>(&self, bitcoin_wallet: &W) -> Result<()>
    where
        W: WatchForRawTransaction + TransactionBlockHeight + GetBlockHeight,
    {
        wait_for_cancel_timelock_to_expire(
            bitcoin_wallet,
            self.cancel_timelock,
            self.tx_lock.txid(),
        )
        .await
    }

    pub fn state4(&self) -> State4 {
        State4 {
            A: self.A,
            b: self.b.clone(),
            s_b: self.s_b,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address.clone(),
            redeem_address: self.redeem_address.clone(),
            punish_address: self.punish_address.clone(),
            tx_lock: self.tx_lock.clone(),
            tx_cancel_sig_a: self.tx_cancel_sig_a.clone(),
            tx_refund_encsig: self.tx_refund_encsig.clone(),
        }
    }

    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }

    pub async fn current_epoch<W>(&self, bitcoin_wallet: &W) -> Result<ExpiredTimelocks>
    where
        W: WatchForRawTransaction + TransactionBlockHeight + GetBlockHeight,
    {
        current_epoch(
            bitcoin_wallet,
            self.cancel_timelock,
            self.punish_timelock,
            self.tx_lock.txid(),
        )
        .await
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct State4 {
    pub A: bitcoin::PublicKey,
    pub b: bitcoin::SecretKey,
    pub s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    pub S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    pub cancel_timelock: Timelock,
    punish_timelock: Timelock,
    pub refund_address: bitcoin::Address,
    pub redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    pub tx_cancel_sig_a: Signature,
    pub tx_refund_encsig: EncryptedSignature,
}

impl State4 {
    pub fn next_message(&self) -> bob::Message3 {
        let tx_redeem = bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address);
        let tx_redeem_encsig = self.b.encsign(self.S_a_bitcoin, tx_redeem.digest());

        bob::Message3 { tx_redeem_encsig }
    }

    pub fn tx_redeem_encsig(&self) -> EncryptedSignature {
        let tx_redeem = bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address);
        self.b.encsign(self.S_a_bitcoin, tx_redeem.digest())
    }

    pub async fn check_for_tx_cancel<W>(&self, bitcoin_wallet: &W) -> Result<Transaction>
    where
        W: GetRawTransaction,
    {
        let tx_cancel =
            bitcoin::TxCancel::new(&self.tx_lock, self.cancel_timelock, self.A, self.b.public());

        let sig_a = self.tx_cancel_sig_a.clone();
        let sig_b = self.b.sign(tx_cancel.digest());

        let tx_cancel = tx_cancel
            .clone()
            .add_signatures(&self.tx_lock, (self.A, sig_a), (self.b.public(), sig_b))
            .expect(
                "sig_{a,b} to be valid signatures for
                tx_cancel",
            );

        let tx = bitcoin_wallet.get_raw_transaction(tx_cancel.txid()).await?;

        Ok(tx)
    }

    pub async fn submit_tx_cancel<W>(&self, bitcoin_wallet: &W) -> Result<Txid>
    where
        W: BroadcastSignedTransaction,
    {
        let tx_cancel =
            bitcoin::TxCancel::new(&self.tx_lock, self.cancel_timelock, self.A, self.b.public());

        let sig_a = self.tx_cancel_sig_a.clone();
        let sig_b = self.b.sign(tx_cancel.digest());

        let tx_cancel = tx_cancel
            .clone()
            .add_signatures(&self.tx_lock, (self.A, sig_a), (self.b.public(), sig_b))
            .expect(
                "sig_{a,b} to be valid signatures for
                tx_cancel",
            );

        let tx_id = bitcoin_wallet
            .broadcast_signed_transaction(tx_cancel)
            .await?;
        Ok(tx_id)
    }

    pub async fn watch_for_redeem_btc<W>(&self, bitcoin_wallet: &W) -> Result<State5>
    where
        W: WatchForRawTransaction,
    {
        let tx_redeem = bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address);
        let tx_redeem_encsig = self.b.encsign(self.S_a_bitcoin, tx_redeem.digest());

        let tx_redeem_candidate = bitcoin_wallet
            .watch_for_raw_transaction(tx_redeem.txid())
            .await;

        let tx_redeem_sig =
            tx_redeem.extract_signature_by_key(tx_redeem_candidate, self.b.public())?;
        let s_a = bitcoin::recover(self.S_a_bitcoin, tx_redeem_sig, tx_redeem_encsig)?;
        let s_a = monero::private_key_from_secp256k1_scalar(s_a.into());

        Ok(State5 {
            A: self.A,
            b: self.b.clone(),
            s_a,
            s_b: self.s_b,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address.clone(),
            redeem_address: self.redeem_address.clone(),
            punish_address: self.punish_address.clone(),
            tx_lock: self.tx_lock.clone(),
            tx_refund_encsig: self.tx_refund_encsig.clone(),
            tx_cancel_sig: self.tx_cancel_sig_a.clone(),
        })
    }

    pub async fn wait_for_cancel_timelock_to_expire<W>(&self, bitcoin_wallet: &W) -> Result<()>
    where
        W: WatchForRawTransaction + TransactionBlockHeight + GetBlockHeight,
    {
        wait_for_cancel_timelock_to_expire(
            bitcoin_wallet,
            self.cancel_timelock,
            self.tx_lock.txid(),
        )
        .await
    }

    pub async fn expired_timelock<W>(&self, bitcoin_wallet: &W) -> Result<ExpiredTimelocks>
    where
        W: WatchForRawTransaction + TransactionBlockHeight + GetBlockHeight,
    {
        current_epoch(
            bitcoin_wallet,
            self.cancel_timelock,
            self.punish_timelock,
            self.tx_lock.txid(),
        )
        .await
    }

    pub async fn refund_btc<W: bitcoin::BroadcastSignedTransaction>(
        &self,
        bitcoin_wallet: &W,
    ) -> Result<()> {
        let tx_cancel =
            bitcoin::TxCancel::new(&self.tx_lock, self.cancel_timelock, self.A, self.b.public());
        let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &self.refund_address);

        {
            let sig_b = self.b.sign(tx_cancel.digest());
            let sig_a = self.tx_cancel_sig_a.clone();

            let signed_tx_cancel = tx_cancel.clone().add_signatures(
                &self.tx_lock,
                (self.A, sig_a),
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
                (self.A, sig_a),
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct State5 {
    A: bitcoin::PublicKey,
    pub b: bitcoin::SecretKey,
    #[serde(with = "monero_private_key")]
    s_a: monero::PrivateKey,
    pub s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    pub S_a_bitcoin: bitcoin::PublicKey,
    pub v: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    cancel_timelock: Timelock,
    punish_timelock: Timelock,
    refund_address: bitcoin::Address,
    pub redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
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
