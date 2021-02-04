use crate::{
    bitcoin::{
        self, current_epoch,
        timelocks::{ExpiredTimelocks, Timelock},
        wait_for_cancel_timelock_to_expire, BroadcastSignedTransaction, BuildTxLockPsbt,
        GetBlockHeight, GetNetwork, GetRawTransaction, Transaction, TransactionBlockHeight,
        TxCancel, Txid, WatchForRawTransaction,
    },
    execution_params::ExecutionParams,
    monero,
    monero::{monero_private_key, TransferProof},
    protocol::{
        alice, bob,
        bob::{EncryptedSignature, Message4},
        SwapAmounts,
    },
};
use anyhow::{anyhow, Result};
use ecdsa_fun::{adaptor::Adaptor, nonce::Deterministic, Signature};
use monero_harness::rpc::wallet::BlockHeight;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::fmt;

#[derive(Debug, Clone)]
pub enum BobState {
    Started {
        state0: State0,
        amounts: SwapAmounts,
    },
    Negotiated(State2),
    BtcLocked(State3),
    XmrLockProofReceived {
        state: State3,
        lock_transfer_proof: TransferProof,
        monero_wallet_restore_blockheight: BlockHeight,
    },
    XmrLocked(State4),
    EncSigSent(State4),
    BtcRedeemed(State5),
    CancelTimelockExpired(State4),
    BtcCancelled(State4),
    BtcRefunded(State4),
    XmrRedeemed {
        tx_lock_id: bitcoin::Txid,
    },
    BtcPunished {
        tx_lock_id: bitcoin::Txid,
    },
    SafelyAborted,
}

impl fmt::Display for BobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BobState::Started { .. } => write!(f, "started"),
            BobState::Negotiated(..) => write!(f, "negotiated"),
            BobState::BtcLocked(..) => write!(f, "btc is locked"),
            BobState::XmrLockProofReceived { .. } => {
                write!(f, "XMR lock transaction transfer proof received")
            }
            BobState::XmrLocked(..) => write!(f, "xmr is locked"),
            BobState::EncSigSent(..) => write!(f, "encrypted signature is sent"),
            BobState::BtcRedeemed(..) => write!(f, "btc is redeemed"),
            BobState::CancelTimelockExpired(..) => write!(f, "cancel timelock is expired"),
            BobState::BtcCancelled(..) => write!(f, "btc is cancelled"),
            BobState::BtcRefunded(..) => write!(f, "btc is refunded"),
            BobState::XmrRedeemed { .. } => write!(f, "xmr is redeemed"),
            BobState::BtcPunished { .. } => write!(f, "btc is punished"),
            BobState::SafelyAborted => write!(f, "safely aborted"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct State0 {
    b: bitcoin::SecretKey,
    s_b: cross_curve_dleq::Scalar,
    v_b: monero::PrivateViewKey,
    dleq_proof_s_b: cross_curve_dleq::Proof,
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
        let dleq_proof_s_b = cross_curve_dleq::Proof::new(rng, &s_b);

        Self {
            b,
            s_b,
            v_b,
            btc,
            xmr,
            dleq_proof_s_b,
            cancel_timelock,
            punish_timelock,
            refund_address,
            min_monero_confirmations,
        }
    }

    pub fn next_message(&self) -> bob::Message0 {
        bob::Message0 {
            B: self.b.public(),
            S_b_monero: monero::PublicKey::from_private_key(&monero::PrivateKey {
                scalar: self.s_b.into_ed25519(),
            }),
            S_b_bitcoin: self.s_b.into_secp256k1().into(),
            dleq_proof_s_b: self.dleq_proof_s_b.clone(),
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
    pub tx_refund_encsig: bitcoin::EncryptedSignature,
    pub min_monero_confirmations: u32,
}

impl State2 {
    pub fn next_message(&self) -> Message4 {
        let tx_cancel = TxCancel::new(&self.tx_lock, self.cancel_timelock, self.A, self.b.public());
        let tx_cancel_sig = self.b.sign(tx_cancel.digest());
        let tx_punish =
            bitcoin::TxPunish::new(&tx_cancel, &self.punish_address, self.punish_timelock);
        let tx_punish_sig = self.b.sign(tx_punish.digest());

        Message4 {
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
    pub tx_refund_encsig: bitcoin::EncryptedSignature,
    pub min_monero_confirmations: u32,
}

impl State3 {
    pub async fn watch_for_lock_xmr<W>(
        self,
        xmr_wallet: &W,
        transfer_proof: TransferProof,
        monero_wallet_restore_blockheight: u32,
    ) -> Result<State4>
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
                transfer_proof,
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
            monero_wallet_restore_blockheight,
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
            monero_wallet_restore_blockheight: 0u32,
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
    pub tx_refund_encsig: bitcoin::EncryptedSignature,
    pub monero_wallet_restore_blockheight: u32,
}

impl State4 {
    pub fn next_message(&self) -> EncryptedSignature {
        let tx_redeem = bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address);
        let tx_redeem_encsig = self.b.encsign(self.S_a_bitcoin, tx_redeem.digest());

        EncryptedSignature { tx_redeem_encsig }
    }

    pub fn tx_redeem_encsig(&self) -> bitcoin::EncryptedSignature {
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
            monero_wallet_restore_blockheight: self.monero_wallet_restore_blockheight,
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

    pub async fn refund_btc<W>(
        &self,
        bitcoin_wallet: &W,
        execution_params: ExecutionParams,
    ) -> Result<()>
    where
        W: bitcoin::BroadcastSignedTransaction + bitcoin::WaitForTransactionFinality,
    {
        let tx_cancel =
            bitcoin::TxCancel::new(&self.tx_lock, self.cancel_timelock, self.A, self.b.public());
        let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &self.refund_address);

        let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

        let sig_b = self.b.sign(tx_refund.digest());
        let sig_a =
            adaptor.decrypt_signature(&self.s_b.into_secp256k1(), self.tx_refund_encsig.clone());

        let signed_tx_refund = tx_refund.add_signatures(
            &tx_cancel.clone(),
            (self.A, sig_a),
            (self.b.public(), sig_b),
        )?;

        let txid = bitcoin_wallet
            .broadcast_signed_transaction(signed_tx_refund)
            .await?;

        bitcoin_wallet
            .wait_for_transaction_finality(txid, execution_params)
            .await?;

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
    tx_refund_encsig: bitcoin::EncryptedSignature,
    tx_cancel_sig: Signature,
    pub monero_wallet_restore_blockheight: u32,
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
            .create_and_load_wallet_for_output(
                s,
                self.v,
                Some(self.monero_wallet_restore_blockheight),
            )
            .await?;

        Ok(())
    }
    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }
}
