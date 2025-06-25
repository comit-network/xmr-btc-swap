use crate::bitcoin::address_serde;
use crate::bitcoin::wallet::{EstimateFeeRate, Subscription};
use crate::bitcoin::{
    self, current_epoch, CancelTimelock, ExpiredTimelocks, PunishTimelock, Transaction, TxCancel,
    TxLock, Txid, Wallet,
};
use crate::monero::wallet::WatchRequest;
use crate::monero::{self, MoneroAddressPool, TxHash};
use crate::monero::{monero_private_key, TransferProof};
use crate::monero_ext::ScalarExt;
use crate::protocol::{Message0, Message1, Message2, Message3, Message4, CROSS_CURVE_PROOF_SYSTEM};
use anyhow::{anyhow, bail, Context, Result};
use ecdsa_fun::adaptor::{Adaptor, HashTranscript};
use ecdsa_fun::nonce::Deterministic;
use ecdsa_fun::Signature;
use monero::BlockHeight;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sigma_fun::ext::dl_secp256k1_ed25519_eq::CrossCurveDLEQProof;
use std::fmt;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum BobState {
    Started {
        #[serde(with = "::bitcoin::amount::serde::as_sat")]
        btc_amount: bitcoin::Amount,
        tx_lock_fee: bitcoin::Amount,
        #[serde(with = "address_serde")]
        change_address: bitcoin::Address,
    },
    SwapSetupCompleted(State2),
    BtcLocked {
        state3: State3,
        monero_wallet_restore_blockheight: BlockHeight,
    },
    XmrLockProofReceived {
        state: State3,
        lock_transfer_proof: TransferProof,
        monero_wallet_restore_blockheight: BlockHeight,
    },
    XmrLocked(State4),
    EncSigSent(State4),
    BtcRedeemed(State5),
    CancelTimelockExpired(State6),
    BtcCancelled(State6),
    BtcRefundPublished(State6),
    BtcEarlyRefundPublished(State6),
    BtcRefunded(State6),
    BtcEarlyRefunded(State6),
    XmrRedeemed {
        tx_lock_id: bitcoin::Txid,
    },
    BtcPunished {
        state: State6,
        tx_lock_id: bitcoin::Txid,
    },
    SafelyAborted,
}

impl fmt::Display for BobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BobState::Started { .. } => write!(f, "quote has been requested"),
            BobState::SwapSetupCompleted(..) => write!(f, "execution setup done"),
            BobState::BtcLocked { .. } => write!(f, "btc is locked"),
            BobState::XmrLockProofReceived { .. } => {
                write!(f, "XMR lock transaction transfer proof received")
            }
            BobState::XmrLocked(..) => write!(f, "xmr is locked"),
            BobState::EncSigSent(..) => write!(f, "encrypted signature is sent"),
            BobState::BtcRedeemed(..) => write!(f, "btc is redeemed"),
            BobState::CancelTimelockExpired(..) => write!(f, "cancel timelock is expired"),
            BobState::BtcCancelled(..) => write!(f, "btc is cancelled"),
            BobState::BtcRefundPublished { .. } => write!(f, "btc refund is published"),
            BobState::BtcEarlyRefundPublished { .. } => write!(f, "btc early refund is published"),
            BobState::BtcRefunded(..) => write!(f, "btc is refunded"),
            BobState::XmrRedeemed { .. } => write!(f, "xmr is redeemed"),
            BobState::BtcPunished { .. } => write!(f, "btc is punished"),
            BobState::BtcEarlyRefunded { .. } => write!(f, "btc is early refunded"),
            BobState::SafelyAborted => write!(f, "safely aborted"),
        }
    }
}

impl BobState {
    /// Fetch the expired timelocks for the swap.
    /// Depending on the State, there are no locks to expire.
    pub async fn expired_timelocks(
        &self,
        bitcoin_wallet: Arc<Wallet>,
    ) -> Result<Option<ExpiredTimelocks>> {
        Ok(match self.clone() {
            BobState::Started { .. }
            | BobState::SafelyAborted
            | BobState::SwapSetupCompleted(_) => None,
            BobState::BtcLocked { state3: state, .. }
            | BobState::XmrLockProofReceived { state, .. } => {
                Some(state.expired_timelock(&bitcoin_wallet).await?)
            }
            BobState::XmrLocked(state) | BobState::EncSigSent(state) => {
                Some(state.expired_timelock(&bitcoin_wallet).await?)
            }
            BobState::CancelTimelockExpired(state)
            | BobState::BtcCancelled(state)
            | BobState::BtcRefundPublished(state)
            | BobState::BtcEarlyRefundPublished(state) => {
                Some(state.expired_timelock(&bitcoin_wallet).await?)
            }
            BobState::BtcPunished { .. } => Some(ExpiredTimelocks::Punish),
            BobState::BtcRefunded(_)
            | BobState::BtcEarlyRefunded { .. }
            | BobState::BtcRedeemed(_)
            | BobState::XmrRedeemed { .. } => None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct State0 {
    swap_id: Uuid,
    b: bitcoin::SecretKey,
    s_b: monero::Scalar,
    S_b_monero: monero::PublicKey,
    S_b_bitcoin: bitcoin::PublicKey,
    v_b: monero::PrivateViewKey,
    dleq_proof_s_b: CrossCurveDLEQProof,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    refund_address: bitcoin::Address,
    min_monero_confirmations: u64,
    tx_refund_fee: bitcoin::Amount,
    tx_cancel_fee: bitcoin::Amount,
    tx_lock_fee: bitcoin::Amount,
}

impl State0 {
    #[allow(clippy::too_many_arguments)]
    pub fn new<R: RngCore + CryptoRng>(
        swap_id: Uuid,
        rng: &mut R,
        btc: bitcoin::Amount,
        xmr: monero::Amount,
        cancel_timelock: CancelTimelock,
        punish_timelock: PunishTimelock,
        refund_address: bitcoin::Address,
        min_monero_confirmations: u64,
        tx_refund_fee: bitcoin::Amount,
        tx_cancel_fee: bitcoin::Amount,
        tx_lock_fee: bitcoin::Amount,
    ) -> Self {
        let b = bitcoin::SecretKey::new_random(rng);

        let s_b = monero::Scalar::random(rng);
        let v_b = monero::PrivateViewKey::new_random(rng);

        let (dleq_proof_s_b, (S_b_bitcoin, S_b_monero)) = CROSS_CURVE_PROOF_SYSTEM.prove(&s_b, rng);

        Self {
            swap_id,
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
            tx_refund_fee,
            tx_cancel_fee,
            tx_lock_fee,
        }
    }

    pub fn next_message(&self) -> Message0 {
        Message0 {
            swap_id: self.swap_id,
            B: self.b.public(),
            S_b_monero: self.S_b_monero,
            S_b_bitcoin: self.S_b_bitcoin,
            dleq_proof_s_b: self.dleq_proof_s_b.clone(),
            v_b: self.v_b,
            refund_address: self.refund_address.clone(),
            tx_refund_fee: self.tx_refund_fee,
            tx_cancel_fee: self.tx_cancel_fee,
        }
    }

    pub async fn receive(
        self,
        wallet: &bitcoin::Wallet<
            bdk_wallet::rusqlite::Connection,
            impl EstimateFeeRate + Send + Sync + 'static,
        >,
        msg: Message1,
    ) -> Result<State1> {
        let valid = CROSS_CURVE_PROOF_SYSTEM.verify(
            &msg.dleq_proof_s_a,
            (
                msg.S_a_bitcoin.into(),
                msg.S_a_monero
                    .point
                    .decompress()
                    .ok_or_else(|| anyhow!("S_a is not a monero curve point"))?,
            ),
        );

        if !valid {
            bail!("Alice's dleq proof doesn't verify")
        }

        let tx_lock = bitcoin::TxLock::new(
            wallet,
            self.btc,
            self.tx_lock_fee,
            msg.A,
            self.b.public(),
            self.refund_address.clone(),
        )
        .await?;
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
            tx_redeem_fee: msg.tx_redeem_fee,
            tx_refund_fee: self.tx_refund_fee,
            tx_punish_fee: msg.tx_punish_fee,
            tx_cancel_fee: self.tx_cancel_fee,
        })
    }
}

#[derive(Debug)]
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
    min_monero_confirmations: u64,
    tx_redeem_fee: bitcoin::Amount,
    tx_refund_fee: bitcoin::Amount,
    tx_punish_fee: bitcoin::Amount,
    tx_cancel_fee: bitcoin::Amount,
}

impl State1 {
    pub fn next_message(&self) -> Message2 {
        Message2 {
            psbt: self.tx_lock.clone().into(),
        }
    }

    pub fn receive(self, msg: Message3) -> Result<State2> {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.A,
            self.b.public(),
            self.tx_cancel_fee,
        )?;

        let tx_refund =
            bitcoin::TxRefund::new(&tx_cancel, &self.refund_address, self.tx_refund_fee);

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
            tx_redeem_fee: self.tx_redeem_fee,
            tx_refund_fee: self.tx_refund_fee,
            tx_punish_fee: self.tx_punish_fee,
            tx_cancel_fee: self.tx_cancel_fee,
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
    pub xmr: monero::Amount,
    pub cancel_timelock: CancelTimelock,
    pub punish_timelock: PunishTimelock,
    #[serde(with = "address_serde")]
    pub refund_address: bitcoin::Address,
    #[serde(with = "address_serde")]
    redeem_address: bitcoin::Address,
    #[serde(with = "address_serde")]
    punish_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    tx_refund_encsig: bitcoin::EncryptedSignature,
    min_monero_confirmations: u64,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    tx_redeem_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    tx_punish_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_refund_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_cancel_fee: bitcoin::Amount,
}

impl State2 {
    pub fn next_message(&self) -> Message4 {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.A,
            self.b.public(),
            self.tx_cancel_fee,
        )
        .expect("valid cancel tx");

        let tx_cancel_sig = self.b.sign(tx_cancel.digest());

        let tx_punish = bitcoin::TxPunish::new(
            &tx_cancel,
            &self.punish_address,
            self.punish_timelock,
            self.tx_punish_fee,
        );
        let tx_punish_sig = self.b.sign(tx_punish.digest());

        let tx_early_refund =
            bitcoin::TxEarlyRefund::new(&self.tx_lock, &self.refund_address, self.tx_refund_fee);

        let tx_early_refund_sig = self.b.sign(tx_early_refund.digest());

        Message4 {
            tx_punish_sig,
            tx_cancel_sig,
            tx_early_refund_sig,
        }
    }

    pub async fn lock_btc(self) -> Result<(State3, TxLock)> {
        Ok((
            State3 {
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
                tx_lock: self.tx_lock.clone(),
                tx_cancel_sig_a: self.tx_cancel_sig_a,
                tx_refund_encsig: self.tx_refund_encsig,
                min_monero_confirmations: self.min_monero_confirmations,
                tx_redeem_fee: self.tx_redeem_fee,
                tx_refund_fee: self.tx_refund_fee,
                tx_cancel_fee: self.tx_cancel_fee,
            },
            self.tx_lock,
        ))
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
    pub cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    #[serde(with = "address_serde")]
    refund_address: bitcoin::Address,
    #[serde(with = "address_serde")]
    redeem_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    tx_refund_encsig: bitcoin::EncryptedSignature,
    min_monero_confirmations: u64,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    tx_redeem_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    tx_refund_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    tx_cancel_fee: bitcoin::Amount,
}

impl State3 {
    pub fn lock_xmr_watch_request(&self, transfer_proof: TransferProof) -> WatchRequest {
        let S_b_monero =
            monero::PublicKey::from_private_key(&monero::PrivateKey::from_scalar(self.s_b));
        let S = self.S_a_monero + S_b_monero;

        WatchRequest {
            public_spend_key: S,
            public_view_key: self.v.public(),
            transfer_proof,
            confirmation_target: self.min_monero_confirmations,
            expected_amount: self.xmr.into(),
        }
    }

    pub fn xmr_locked(
        self,
        monero_wallet_restore_blockheight: BlockHeight,
        lock_transfer_proof: TransferProof,
    ) -> State4 {
        State4 {
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
            lock_transfer_proof,
            tx_redeem_fee: self.tx_redeem_fee,
            tx_refund_fee: self.tx_refund_fee,
            tx_cancel_fee: self.tx_cancel_fee,
        }
    }

    pub fn cancel(&self, monero_wallet_restore_blockheight: BlockHeight) -> State6 {
        State6 {
            A: self.A,
            b: self.b.clone(),
            s_b: self.s_b,
            v: self.v,
            monero_wallet_restore_blockheight,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address.clone(),
            tx_lock: self.tx_lock.clone(),
            tx_cancel_sig_a: self.tx_cancel_sig_a.clone(),
            tx_refund_encsig: self.tx_refund_encsig.clone(),
            tx_refund_fee: self.tx_refund_fee,
            tx_cancel_fee: self.tx_cancel_fee,
        }
    }

    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }

    pub async fn expired_timelock(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<ExpiredTimelocks> {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.A,
            self.b.public(),
            self.tx_cancel_fee,
        )?;

        let tx_lock_status = bitcoin_wallet.status_of_script(&self.tx_lock).await?;
        let tx_cancel_status = bitcoin_wallet.status_of_script(&tx_cancel).await?;

        Ok(current_epoch(
            self.cancel_timelock,
            self.punish_timelock,
            tx_lock_status,
            tx_cancel_status,
        ))
    }

    pub fn attempt_cooperative_redeem(
        &self,
        s_a: monero::PrivateKey,
        monero_wallet_restore_blockheight: BlockHeight,
        lock_transfer_proof: TransferProof,
    ) -> State5 {
        State5 {
            s_a,
            s_b: self.s_b,
            v: self.v,
            tx_lock: self.tx_lock.clone(),
            monero_wallet_restore_blockheight,
            lock_transfer_proof,
        }
    }

    pub fn construct_tx_early_refund(&self) -> bitcoin::TxEarlyRefund {
        bitcoin::TxEarlyRefund::new(&self.tx_lock, &self.refund_address, self.tx_refund_fee)
    }

    pub async fn check_for_tx_early_refund(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<Option<Arc<Transaction>>> {
        let tx_early_refund = self.construct_tx_early_refund();
        let tx = bitcoin_wallet
            .get_raw_transaction(tx_early_refund.txid())
            .await
            .context("Failed to check for existence of tx_early_refund")?;

        Ok(tx)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct State4 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: monero::Scalar,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    pub cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    #[serde(with = "address_serde")]
    refund_address: bitcoin::Address,
    #[serde(with = "address_serde")]
    redeem_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    tx_refund_encsig: bitcoin::EncryptedSignature,
    monero_wallet_restore_blockheight: BlockHeight,
    lock_transfer_proof: TransferProof,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    tx_redeem_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    tx_refund_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    tx_cancel_fee: bitcoin::Amount,
}

impl State4 {
    pub async fn check_for_tx_redeem(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<Option<State5>> {
        let tx_redeem =
            bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address, self.tx_redeem_fee);
        let tx_redeem_encsig = self.b.encsign(self.S_a_bitcoin, tx_redeem.digest());

        let tx_redeem_candidate = bitcoin_wallet.get_raw_transaction(tx_redeem.txid()).await?;

        if let Some(tx_redeem_candidate) = tx_redeem_candidate {
            let tx_redeem_sig =
                tx_redeem.extract_signature_by_key(tx_redeem_candidate, self.b.public())?;
            let s_a = bitcoin::recover(self.S_a_bitcoin, tx_redeem_sig, tx_redeem_encsig)?;
            let s_a = monero::private_key_from_secp256k1_scalar(s_a.into());

            Ok(Some(State5 {
                s_a,
                s_b: self.s_b,
                v: self.v,
                tx_lock: self.tx_lock.clone(),
                monero_wallet_restore_blockheight: self.monero_wallet_restore_blockheight,
                lock_transfer_proof: self.lock_transfer_proof.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn tx_redeem_encsig(&self) -> bitcoin::EncryptedSignature {
        let tx_redeem =
            bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address, self.tx_redeem_fee);
        self.b.encsign(self.S_a_bitcoin, tx_redeem.digest())
    }

    pub async fn watch_for_redeem_btc(&self, bitcoin_wallet: &bitcoin::Wallet) -> Result<State5> {
        let tx_redeem =
            bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address, self.tx_redeem_fee);

        bitcoin_wallet
            .subscribe_to(tx_redeem.clone())
            .await
            .wait_until_seen()
            .await?;

        let state5 = self.check_for_tx_redeem(bitcoin_wallet).await?;

        state5.ok_or_else(|| {
            anyhow!("Bitcoin redeem transaction was not found in the chain even though we previously saw it in the mempool. Our Electrum server might have cleared its mempool?")
        })
    }

    pub async fn expired_timelock(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<ExpiredTimelocks> {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.A,
            self.b.public(),
            self.tx_cancel_fee,
        )?;

        let tx_lock_status = bitcoin_wallet.status_of_script(&self.tx_lock).await?;
        let tx_cancel_status = bitcoin_wallet.status_of_script(&tx_cancel).await?;

        Ok(current_epoch(
            self.cancel_timelock,
            self.punish_timelock,
            tx_lock_status,
            tx_cancel_status,
        ))
    }

    pub fn cancel(self) -> State6 {
        State6 {
            A: self.A,
            b: self.b,
            s_b: self.s_b,
            v: self.v,
            monero_wallet_restore_blockheight: self.monero_wallet_restore_blockheight,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: self.tx_cancel_sig_a,
            tx_refund_encsig: self.tx_refund_encsig,
            tx_refund_fee: self.tx_refund_fee,
            tx_cancel_fee: self.tx_cancel_fee,
        }
    }

    pub fn construct_tx_early_refund(&self) -> bitcoin::TxEarlyRefund {
        bitcoin::TxEarlyRefund::new(&self.tx_lock, &self.refund_address, self.tx_refund_fee)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct State5 {
    #[serde(with = "monero_private_key")]
    s_a: monero::PrivateKey,
    s_b: monero::Scalar,
    v: monero::PrivateViewKey,
    tx_lock: bitcoin::TxLock,
    pub monero_wallet_restore_blockheight: BlockHeight,
    pub lock_transfer_proof: TransferProof,
}

impl State5 {
    pub fn xmr_keys(&self) -> (monero::PrivateKey, monero::PrivateViewKey) {
        let s_b = monero::PrivateKey { scalar: self.s_b };
        let s = self.s_a + s_b;

        (s, self.v)
    }

    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }

    pub async fn redeem_xmr(
        &self,
        monero_wallet: &monero::Wallets,
        swap_id: Uuid,
        monero_receive_pool: MoneroAddressPool,
    ) -> Result<Vec<TxHash>> {
        let (spend_key, view_key) = self.xmr_keys();

        tracing::info!(%swap_id, "Redeeming Monero from extracted keys");

        tracing::debug!(%swap_id, "Opening temporary Monero wallet");

        let wallet = monero_wallet
            .swap_wallet(
                swap_id,
                spend_key,
                view_key,
                self.lock_transfer_proof.tx_hash(),
            )
            .await
            .context("Failed to open Monero wallet")?;

        // Update blockheight to ensure that the wallet knows the funds are unlocked
        tracing::debug!(%swap_id, "Updating temporary Monero wallet's blockheight");
        let _ = wallet
            .blockchain_height()
            .await
            .context("Couldn't get Monero blockheight")?;

        tracing::debug!(%swap_id, receive_address=?monero_receive_pool, "Sweeping Monero to receive address");

        let tx_hashes = wallet
            .sweep_multi(
                &monero_receive_pool.addresses(),
                &monero_receive_pool.percentages(),
            )
            .await
            .context("Failed to redeem Monero")?
            .into_iter()
            .map(|tx_receipt| TxHash(tx_receipt.txid))
            .collect();

        tracing::info!(%swap_id, txids=?tx_hashes, "Monero sweep completed");

        Ok(tx_hashes)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct State6 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: monero::Scalar,
    v: monero::PrivateViewKey,
    pub monero_wallet_restore_blockheight: BlockHeight,
    pub cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    #[serde(with = "address_serde")]
    refund_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    tx_refund_encsig: bitcoin::EncryptedSignature,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_refund_fee: bitcoin::Amount,
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_cancel_fee: bitcoin::Amount,
}

impl State6 {
    pub async fn expired_timelock(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<ExpiredTimelocks> {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.A,
            self.b.public(),
            self.tx_cancel_fee,
        )?;

        let tx_lock_status = bitcoin_wallet.status_of_script(&self.tx_lock).await?;
        let tx_cancel_status = bitcoin_wallet.status_of_script(&tx_cancel).await?;

        Ok(current_epoch(
            self.cancel_timelock,
            self.punish_timelock,
            tx_lock_status,
            tx_cancel_status,
        ))
    }

    pub fn construct_tx_cancel(&self) -> Result<bitcoin::TxCancel> {
        bitcoin::TxCancel::new(
            &self.tx_lock,
            self.cancel_timelock,
            self.A,
            self.b.public(),
            self.tx_cancel_fee,
        )
    }

    pub async fn check_for_tx_cancel(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<Option<Arc<Transaction>>> {
        let tx_cancel = self.construct_tx_cancel()?;

        let tx = bitcoin_wallet
            .get_raw_transaction(tx_cancel.txid())
            .await
            .context("Failed to check for existence of tx_cancel")?;

        Ok(tx)
    }

    pub async fn submit_tx_cancel(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<(Txid, Subscription)> {
        let transaction = self
            .construct_tx_cancel()?
            .complete_as_bob(self.A, self.b.clone(), self.tx_cancel_sig_a.clone())
            .context("Failed to complete Bitcoin cancel transaction")?;

        let (tx_id, subscription) = bitcoin_wallet.broadcast(transaction, "cancel").await?;

        Ok((tx_id, subscription))
    }

    pub async fn publish_refund_btc(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<bitcoin::Txid> {
        let signed_tx_refund = self.signed_refund_transaction()?;
        let signed_tx_refund_txid = signed_tx_refund.compute_txid();
        bitcoin_wallet.broadcast(signed_tx_refund, "refund").await?;

        Ok(signed_tx_refund_txid)
    }

    pub fn construct_tx_refund(&self) -> Result<bitcoin::TxRefund> {
        let tx_cancel = self.construct_tx_cancel()?;

        let tx_refund =
            bitcoin::TxRefund::new(&tx_cancel, &self.refund_address, self.tx_refund_fee);

        Ok(tx_refund)
    }

    pub fn signed_refund_transaction(&self) -> Result<Transaction> {
        let tx_refund = self.construct_tx_refund()?;

        let adaptor = Adaptor::<HashTranscript<Sha256>, Deterministic<Sha256>>::default();

        let sig_b = self.b.sign(tx_refund.digest());
        let sig_a =
            adaptor.decrypt_signature(&self.s_b.to_secpfun_scalar(), self.tx_refund_encsig.clone());

        let signed_tx_refund =
            tx_refund.add_signatures((self.A, sig_a), (self.b.public(), sig_b))?;

        Ok(signed_tx_refund)
    }

    pub fn construct_tx_early_refund(&self) -> bitcoin::TxEarlyRefund {
        bitcoin::TxEarlyRefund::new(&self.tx_lock, &self.refund_address, self.tx_refund_fee)
    }

    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }
    pub fn attempt_cooperative_redeem(
        &self,
        s_a: monero::PrivateKey,
        lock_transfer_proof: TransferProof,
    ) -> State5 {
        State5 {
            s_a,
            s_b: self.s_b,
            v: self.v,
            tx_lock: self.tx_lock.clone(),
            monero_wallet_restore_blockheight: self.monero_wallet_restore_blockheight,
            lock_transfer_proof,
        }
    }

    pub async fn check_for_tx_early_refund(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<Option<Arc<Transaction>>> {
        let tx_early_refund = self.construct_tx_early_refund();

        let tx = bitcoin_wallet
            .get_raw_transaction(tx_early_refund.txid())
            .await
            .context("Failed to check for existence of tx_early_refund")?;

        Ok(tx)
    }
}
