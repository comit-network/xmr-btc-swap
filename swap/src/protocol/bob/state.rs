use crate::bitcoin::{
    self, current_epoch, wait_for_cancel_timelock_to_expire, CancelTimelock, ExpiredTimelocks,
    PunishTimelock, Transaction, TxCancel, Txid,
};
use crate::execution_params::ExecutionParams;
use crate::monero;
use crate::monero::{monero_private_key, InsufficientFunds, TransferProof};
use crate::monero_ext::ScalarExt;
use crate::protocol::alice::{Message1, Message3};
use crate::protocol::bob::{EncryptedSignature, Message0, Message2, Message4};
use crate::protocol::CROSS_CURVE_PROOF_SYSTEM;
use anyhow::{anyhow, bail, Context, Result};
use ecdsa_fun::adaptor::{Adaptor, HashTranscript};
use ecdsa_fun::nonce::Deterministic;
use ecdsa_fun::Signature;
use monero_rpc::wallet::BlockHeight;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sigma_fun::ext::dl_secp256k1_ed25519_eq::CrossCurveDLEQProof;
use std::fmt;

#[derive(Debug, Clone)]
pub enum BobState {
    Started {
        btc_amount: bitcoin::Amount,
    },
    ExecutionSetupDone(State2),
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
            BobState::Started { .. } => write!(f, "quote has been requested"),
            BobState::ExecutionSetupDone(..) => write!(f, "execution setup done"),
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
    s_b: monero::Scalar,
    S_b_monero: monero::PublicKey,
    S_b_bitcoin: bitcoin::PublicKey,
    v_b: monero::PrivateViewKey,
    dleq_proof_s_b: CrossCurveDLEQProof,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    refund_address: bitcoin::Address,
    min_monero_confirmations: u32,
}

impl State0 {
    pub fn new<R: RngCore + CryptoRng>(
        rng: &mut R,
        btc: bitcoin::Amount,
        xmr: monero::Amount,
        cancel_timelock: CancelTimelock,
        punish_timelock: PunishTimelock,
        refund_address: bitcoin::Address,
        min_monero_confirmations: u32,
    ) -> Self {
        let b = bitcoin::SecretKey::new_random(rng);

        let s_b = monero::Scalar::random(rng);
        let v_b = monero::PrivateViewKey::new_random(rng);

        let (dleq_proof_s_b, (S_b_bitcoin, S_b_monero)) = CROSS_CURVE_PROOF_SYSTEM.prove(&s_b, rng);

        Self {
            b,
            s_b,
            v_b,
            S_b_bitcoin: bitcoin::PublicKey::from(S_b_bitcoin),
            S_b_monero: monero::PublicKey {
                point: S_b_monero.compress(),
            },
            btc,
            xmr,
            dleq_proof_s_b,
            cancel_timelock,
            punish_timelock,
            refund_address,
            min_monero_confirmations,
        }
    }

    pub fn next_message(&self) -> Message0 {
        Message0 {
            B: self.b.public(),
            S_b_monero: self.S_b_monero,
            S_b_bitcoin: self.S_b_bitcoin,
            dleq_proof_s_b: self.dleq_proof_s_b.clone(),
            v_b: self.v_b,
            refund_address: self.refund_address.clone(),
        }
    }

    pub async fn receive(self, wallet: &bitcoin::Wallet, msg: Message1) -> Result<State1> {
        let valid = CROSS_CURVE_PROOF_SYSTEM.verify(
            &msg.dleq_proof_s_a,
            (
                msg.S_a_bitcoin.clone().into(),
                msg.S_a_monero
                    .point
                    .decompress()
                    .ok_or_else(|| anyhow!("S_a is not a monero curve point"))?,
            ),
        );

        if !valid {
            bail!("Alice's dleq proof doesn't verify")
        }

        let tx_lock = bitcoin::TxLock::new(wallet, self.btc, msg.A, self.b.public()).await?;
        let v = msg.v_a + self.v_b;

        Ok(State1 {
            A: msg.A,
            b: self.b,
            s_b: self.s_b,
            S_a_monero: msg.S_a_monero,
            S_a_bitcoin: msg.S_a_bitcoin,
            v,
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
    s_b: monero::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    xmr: monero::Amount,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
    min_monero_confirmations: u32,
}

impl State1 {
    pub fn next_message(&self) -> Message2 {
        Message2 {
            tx_lock: self.tx_lock.clone(),
        }
    }

    pub fn receive(self, msg: Message3) -> Result<State2> {
        let tx_cancel = TxCancel::new(&self.tx_lock, self.cancel_timelock, self.A, self.b.public());
        let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &self.refund_address);

        bitcoin::verify_sig(&self.A, &tx_cancel.digest(), &msg.tx_cancel_sig)?;
        bitcoin::verify_encsig(
            self.A,
            bitcoin::PublicKey::from(self.s_b.to_secpfun_scalar()),
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
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: monero::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    xmr: monero::Amount,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    tx_refund_encsig: bitcoin::EncryptedSignature,
    min_monero_confirmations: u32,
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

    pub async fn lock_btc(self, bitcoin_wallet: &bitcoin::Wallet) -> Result<State3> {
        let signed_tx = bitcoin_wallet
            .sign_and_finalize(self.tx_lock.clone().into())
            .await
            .context("Failed to sign Bitcoin lock transaction")?;

        let _ = bitcoin_wallet.broadcast(signed_tx, "lock").await?;

        Ok(State3 {
            A: self.A,
            b: self.b,
            s_b: self.s_b,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            xmr: self.xmr,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: self.tx_cancel_sig_a,
            tx_refund_encsig: self.tx_refund_encsig,
            min_monero_confirmations: self.min_monero_confirmations,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct State3 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: monero::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    xmr: monero::Amount,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    tx_refund_encsig: bitcoin::EncryptedSignature,
    min_monero_confirmations: u32,
}

impl State3 {
    pub async fn watch_for_lock_xmr(
        self,
        xmr_wallet: &monero::Wallet,
        transfer_proof: TransferProof,
        monero_wallet_restore_blockheight: BlockHeight,
    ) -> Result<Result<State4, InsufficientFunds>> {
        let S_b_monero =
            monero::PublicKey::from_private_key(&monero::PrivateKey::from_scalar(self.s_b));
        let S = self.S_a_monero + S_b_monero;

        if let Err(e) = xmr_wallet
            .watch_for_transfer(
                S,
                self.v.public(),
                transfer_proof,
                self.xmr,
                self.min_monero_confirmations,
            )
            .await
        {
            return Ok(Err(e));
        }

        Ok(Ok(State4 {
            A: self.A,
            b: self.b,
            s_b: self.s_b,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: self.tx_cancel_sig_a,
            tx_refund_encsig: self.tx_refund_encsig,
            monero_wallet_restore_blockheight,
        }))
    }

    pub async fn wait_for_cancel_timelock_to_expire(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<()> {
        wait_for_cancel_timelock_to_expire(
            bitcoin_wallet,
            self.cancel_timelock,
            self.tx_lock.txid(),
        )
        .await
    }

    pub fn cancel(&self) -> State4 {
        State4 {
            A: self.A,
            b: self.b.clone(),
            s_b: self.s_b,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address.clone(),
            redeem_address: self.redeem_address.clone(),
            tx_lock: self.tx_lock.clone(),
            tx_cancel_sig_a: self.tx_cancel_sig_a.clone(),
            tx_refund_encsig: self.tx_refund_encsig.clone(),
            // For cancel scenarios the monero wallet rescan blockchain height is irrelevant for
            // Bob, because Bob's cancel can only lead to refunding on Bitcoin
            monero_wallet_restore_blockheight: BlockHeight { height: 0 },
        }
    }

    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }

    pub async fn current_epoch(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<ExpiredTimelocks> {
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
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: monero::Scalar,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    tx_refund_encsig: bitcoin::EncryptedSignature,
    monero_wallet_restore_blockheight: BlockHeight,
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

    pub async fn check_for_tx_cancel(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<Transaction> {
        let tx_cancel =
            bitcoin::TxCancel::new(&self.tx_lock, self.cancel_timelock, self.A, self.b.public());

        let sig_a = self.tx_cancel_sig_a.clone();
        let sig_b = self.b.sign(tx_cancel.digest());

        let tx_cancel = tx_cancel
            .clone()
            .add_signatures((self.A, sig_a), (self.b.public(), sig_b))
            .expect(
                "sig_{a,b} to be valid signatures for
                tx_cancel",
            );

        let tx = bitcoin_wallet.get_raw_transaction(tx_cancel.txid()).await?;

        Ok(tx)
    }

    pub async fn submit_tx_cancel(&self, bitcoin_wallet: &bitcoin::Wallet) -> Result<Txid> {
        let tx_cancel =
            bitcoin::TxCancel::new(&self.tx_lock, self.cancel_timelock, self.A, self.b.public());

        let sig_a = self.tx_cancel_sig_a.clone();
        let sig_b = self.b.sign(tx_cancel.digest());

        let tx_cancel = tx_cancel
            .clone()
            .add_signatures((self.A, sig_a), (self.b.public(), sig_b))
            .expect(
                "sig_{a,b} to be valid signatures for
                tx_cancel",
            );

        let tx_id = bitcoin_wallet.broadcast(tx_cancel, "cancel").await?;

        Ok(tx_id)
    }

    pub async fn watch_for_redeem_btc(&self, bitcoin_wallet: &bitcoin::Wallet) -> Result<State5> {
        let tx_redeem = bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address);
        let tx_redeem_encsig = self.b.encsign(self.S_a_bitcoin, tx_redeem.digest());

        let tx_redeem_candidate = bitcoin_wallet
            .watch_for_raw_transaction(tx_redeem.txid())
            .await?;

        let tx_redeem_sig =
            tx_redeem.extract_signature_by_key(tx_redeem_candidate, self.b.public())?;
        let s_a = bitcoin::recover(self.S_a_bitcoin, tx_redeem_sig, tx_redeem_encsig)?;
        let s_a = monero::private_key_from_secp256k1_scalar(s_a.into());

        Ok(State5 {
            s_a,
            s_b: self.s_b,
            v: self.v,
            tx_lock: self.tx_lock.clone(),
            monero_wallet_restore_blockheight: self.monero_wallet_restore_blockheight,
        })
    }

    pub async fn wait_for_cancel_timelock_to_expire(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<()> {
        wait_for_cancel_timelock_to_expire(
            bitcoin_wallet,
            self.cancel_timelock,
            self.tx_lock.txid(),
        )
        .await
    }

    pub async fn expired_timelock(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<ExpiredTimelocks> {
        current_epoch(
            bitcoin_wallet,
            self.cancel_timelock,
            self.punish_timelock,
            self.tx_lock.txid(),
        )
        .await
    }

    pub async fn refund_btc(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
        execution_params: ExecutionParams,
    ) -> Result<()> {
        let tx_cancel =
            bitcoin::TxCancel::new(&self.tx_lock, self.cancel_timelock, self.A, self.b.public());
        let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &self.refund_address);

        let adaptor = Adaptor::<HashTranscript<Sha256>, Deterministic<Sha256>>::default();

        let sig_b = self.b.sign(tx_refund.digest());
        let sig_a =
            adaptor.decrypt_signature(&self.s_b.to_secpfun_scalar(), self.tx_refund_encsig.clone());

        let signed_tx_refund =
            tx_refund.add_signatures((self.A, sig_a), (self.b.public(), sig_b))?;

        let txid = bitcoin_wallet.broadcast(signed_tx_refund, "refund").await?;

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
    #[serde(with = "monero_private_key")]
    s_a: monero::PrivateKey,
    s_b: monero::Scalar,
    v: monero::PrivateViewKey,
    tx_lock: bitcoin::TxLock,
    monero_wallet_restore_blockheight: BlockHeight,
}

impl State5 {
    pub async fn claim_xmr(&self, monero_wallet: &monero::Wallet) -> Result<()> {
        let s_b = monero::PrivateKey { scalar: self.s_b };

        let s = self.s_a + s_b;

        // NOTE: This actually generates and opens a new wallet, closing the currently
        // open one.
        monero_wallet
            .create_from_and_load(s, self.v, self.monero_wallet_restore_blockheight)
            .await?;

        Ok(())
    }
    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }
}
