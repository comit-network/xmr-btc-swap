use crate::bitcoin::{
    current_epoch, CancelTimelock, ExpiredTimelocks, PunishTimelock, TxCancel, TxPunish, TxRefund,
};
use crate::env::Config;
use crate::monero::wallet::{TransferRequest, WatchRequest};
use crate::monero::TransferProof;
use crate::monero_ext::ScalarExt;
use crate::protocol::{Message0, Message1, Message2, Message3, Message4, CROSS_CURVE_PROOF_SYSTEM};
use crate::{bitcoin, monero};
use anyhow::{anyhow, bail, Context, Result};
use monero_rpc::wallet::BlockHeight;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sigma_fun::ext::dl_secp256k1_ed25519_eq::CrossCurveDLEQProof;
use std::fmt;
use uuid::Uuid;

#[derive(Debug)]
pub enum AliceState {
    Started {
        state3: Box<State3>,
    },
    BtcLocked {
        state3: Box<State3>,
    },
    XmrLockTransactionSent {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    XmrLocked {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    XmrLockTransferProofSent {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    EncSigLearned {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        encrypted_signature: Box<bitcoin::EncryptedSignature>,
        state3: Box<State3>,
    },
    BtcRedeemed,
    BtcCancelled {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    BtcRefunded {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        spend_key: monero::PrivateKey,
        state3: Box<State3>,
    },
    BtcPunishable {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    XmrRefunded,
    CancelTimelockExpired {
        monero_wallet_restore_blockheight: BlockHeight,
        transfer_proof: TransferProof,
        state3: Box<State3>,
    },
    BtcPunished,
    SafelyAborted,
}

impl fmt::Display for AliceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AliceState::Started { .. } => write!(f, "started"),
            AliceState::BtcLocked { .. } => write!(f, "btc is locked"),
            AliceState::XmrLockTransactionSent { .. } => write!(f, "xmr lock transaction sent"),
            AliceState::XmrLocked { .. } => write!(f, "xmr is locked"),
            AliceState::XmrLockTransferProofSent { .. } => {
                write!(f, "xmr lock transfer proof sent")
            }
            AliceState::EncSigLearned { .. } => write!(f, "encrypted signature is learned"),
            AliceState::BtcRedeemed => write!(f, "btc is redeemed"),
            AliceState::BtcCancelled { .. } => write!(f, "btc is cancelled"),
            AliceState::BtcRefunded { .. } => write!(f, "btc is refunded"),
            AliceState::BtcPunished => write!(f, "btc is punished"),
            AliceState::SafelyAborted => write!(f, "safely aborted"),
            AliceState::BtcPunishable { .. } => write!(f, "btc is punishable"),
            AliceState::XmrRefunded => write!(f, "xmr is refunded"),
            AliceState::CancelTimelockExpired { .. } => write!(f, "cancel timelock is expired"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct State0 {
    a: bitcoin::SecretKey,
    s_a: monero::Scalar,
    v_a: monero::PrivateViewKey,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    dleq_proof_s_a: CrossCurveDLEQProof,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
}

impl State0 {
    pub async fn new<R>(
        btc: bitcoin::Amount,
        xmr: monero::Amount,
        env_config: Config,
        bitcoin_wallet: &bitcoin::Wallet,
        rng: &mut R,
    ) -> Result<Self>
    where
        R: RngCore + CryptoRng,
    {
        let a = bitcoin::SecretKey::new_random(rng);
        let v_a = monero::PrivateViewKey::new_random(rng);
        let redeem_address = bitcoin_wallet.new_address().await?;
        let punish_address = bitcoin_wallet.new_address().await?;

        let s_a = monero::Scalar::random(rng);
        let (dleq_proof_s_a, (S_a_bitcoin, S_a_monero)) = CROSS_CURVE_PROOF_SYSTEM.prove(&s_a, rng);

        Ok(Self {
            a,
            s_a,
            v_a,
            S_a_bitcoin: S_a_bitcoin.into(),
            S_a_monero: monero::PublicKey {
                point: S_a_monero.compress(),
            },
            dleq_proof_s_a,
            redeem_address,
            punish_address,
            btc,
            xmr,
            cancel_timelock: env_config.bitcoin_cancel_timelock,
            punish_timelock: env_config.bitcoin_punish_timelock,
        })
    }

    pub fn receive(self, msg: Message0) -> Result<(Uuid, State1)> {
        let valid = CROSS_CURVE_PROOF_SYSTEM.verify(
            &msg.dleq_proof_s_b,
            (
                msg.S_b_bitcoin.into(),
                msg.S_b_monero
                    .point
                    .decompress()
                    .ok_or_else(|| anyhow!("S_b is not a monero curve point"))?,
            ),
        );

        if !valid {
            bail!("Bob's dleq proof doesn't verify")
        }

        let v = self.v_a + msg.v_b;

        Ok((msg.swap_id, State1 {
            a: self.a,
            B: msg.B,
            s_a: self.s_a,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            S_b_monero: msg.S_b_monero,
            S_b_bitcoin: msg.S_b_bitcoin,
            v,
            v_a: self.v_a,
            dleq_proof_s_a: self.dleq_proof_s_a,
            btc: self.btc,
            xmr: self.xmr,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: msg.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
        }))
    }
}

#[derive(Clone, Debug)]
pub struct State1 {
    a: bitcoin::SecretKey,
    B: bitcoin::PublicKey,
    s_a: monero::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    S_b_monero: monero::PublicKey,
    S_b_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    v_a: monero::PrivateViewKey,
    dleq_proof_s_a: CrossCurveDLEQProof,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
}

impl State1 {
    pub fn next_message(&self) -> Message1 {
        Message1 {
            A: self.a.public(),
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            dleq_proof_s_a: self.dleq_proof_s_a.clone(),
            v_a: self.v_a,
            redeem_address: self.redeem_address.clone(),
            punish_address: self.punish_address.clone(),
        }
    }

    pub fn receive(self, msg: Message2) -> Result<State2> {
        let tx_lock = bitcoin::TxLock::from_psbt(msg.psbt, self.a.public(), self.B, self.btc)
            .context("Failed to re-construct TxLock from received PSBT")?;

        Ok(State2 {
            a: self.a,
            B: self.B,
            s_a: self.s_a,
            S_b_monero: self.S_b_monero,
            S_b_bitcoin: self.S_b_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            cancel_timelock: self.cancel_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock,
        })
    }
}

#[derive(Clone, Debug)]
pub struct State2 {
    a: bitcoin::SecretKey,
    B: bitcoin::PublicKey,
    s_a: monero::Scalar,
    S_b_monero: monero::PublicKey,
    S_b_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
}

impl State2 {
    pub fn next_message(&self) -> Message3 {
        let tx_cancel =
            bitcoin::TxCancel::new(&self.tx_lock, self.cancel_timelock, self.a.public(), self.B);

        let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &self.refund_address);
        // Alice encsigns the refund transaction(bitcoin) digest with Bob's monero
        // pubkey(S_b). The refund transaction spends the output of
        // tx_lock_bitcoin to Bob's refund address.
        // recover(encsign(a, S_b, d), sign(a, d), S_b) = s_b where d is a digest, (a,
        // A) is alice's keypair and (s_b, S_b) is bob's keypair.
        let tx_refund_encsig = self.a.encsign(self.S_b_bitcoin, tx_refund.digest());

        let tx_cancel_sig = self.a.sign(tx_cancel.digest());
        Message3 {
            tx_refund_encsig,
            tx_cancel_sig,
        }
    }

    pub fn receive(self, msg: Message4) -> Result<State3> {
        let tx_cancel =
            bitcoin::TxCancel::new(&self.tx_lock, self.cancel_timelock, self.a.public(), self.B);
        bitcoin::verify_sig(&self.B, &tx_cancel.digest(), &msg.tx_cancel_sig)
            .context("Failed to verify cancel transaction")?;
        let tx_punish =
            bitcoin::TxPunish::new(&tx_cancel, &self.punish_address, self.punish_timelock);
        bitcoin::verify_sig(&self.B, &tx_punish.digest(), &msg.tx_punish_sig)
            .context("Failed to verify punish transaction")?;

        Ok(State3 {
            a: self.a,
            B: self.B,
            s_a: self.s_a,
            S_b_monero: self.S_b_monero,
            S_b_bitcoin: self.S_b_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            cancel_timelock: self.cancel_timelock,
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
    a: bitcoin::SecretKey,
    B: bitcoin::PublicKey,
    s_a: monero::Scalar,
    S_b_monero: monero::PublicKey,
    S_b_bitcoin: bitcoin::PublicKey,
    pub v: monero::PrivateViewKey,
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    pub cancel_timelock: CancelTimelock,
    pub punish_timelock: PunishTimelock,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    pub tx_lock: bitcoin::TxLock,
    tx_punish_sig_bob: bitcoin::Signature,
    tx_cancel_sig_bob: bitcoin::Signature,
}

impl State3 {
    pub async fn expired_timelocks(
        &self,
        bitcoin_wallet: &bitcoin::Wallet,
    ) -> Result<ExpiredTimelocks> {
        let tx_cancel = self.tx_cancel();

        let tx_lock_status = bitcoin_wallet.status_of_script(&self.tx_lock).await?;
        let tx_cancel_status = bitcoin_wallet.status_of_script(&tx_cancel).await?;

        Ok(current_epoch(
            self.cancel_timelock,
            self.punish_timelock,
            tx_lock_status,
            tx_cancel_status,
        ))
    }

    pub fn lock_xmr_transfer_request(&self) -> TransferRequest {
        let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey { scalar: self.s_a });

        let public_spend_key = S_a + self.S_b_monero;
        let public_view_key = self.v.public();

        TransferRequest {
            public_spend_key,
            public_view_key,
            amount: self.xmr,
        }
    }

    pub fn lock_xmr_watch_request(
        &self,
        transfer_proof: TransferProof,
        conf_target: u64,
    ) -> WatchRequest {
        let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey { scalar: self.s_a });

        let public_spend_key = S_a + self.S_b_monero;
        let public_view_key = self.v.public();
        WatchRequest {
            public_spend_key,
            public_view_key,
            transfer_proof,
            conf_target,
            expected: self.xmr,
        }
    }

    pub fn tx_cancel(&self) -> TxCancel {
        TxCancel::new(&self.tx_lock, self.cancel_timelock, self.a.public(), self.B)
    }

    pub fn tx_refund(&self) -> TxRefund {
        bitcoin::TxRefund::new(&self.tx_cancel(), &self.refund_address)
    }

    pub fn extract_monero_private_key(
        &self,
        published_refund_tx: bitcoin::Transaction,
    ) -> Result<monero::PrivateKey> {
        self.tx_refund().extract_monero_private_key(
            published_refund_tx,
            self.s_a,
            self.a.clone(),
            self.S_b_bitcoin,
        )
    }

    pub fn signed_redeem_transaction(
        &self,
        sig: bitcoin::EncryptedSignature,
    ) -> Result<bitcoin::Transaction> {
        bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address)
            .complete(sig, self.a.clone(), self.s_a.to_secpfun_scalar(), self.B)
            .context("Failed to complete Bitcoin redeem transaction")
    }

    pub fn signed_cancel_transaction(&self) -> Result<bitcoin::Transaction> {
        self.tx_cancel()
            .complete_as_alice(self.a.clone(), self.B, self.tx_cancel_sig_bob.clone())
            .context("Failed to complete Bitcoin cancel transaction")
    }

    pub fn signed_punish_transaction(&self) -> Result<bitcoin::Transaction> {
        self.tx_punish()
            .complete(self.tx_punish_sig_bob.clone(), self.a.clone(), self.B)
            .context("Failed to complete Bitcoin punish transaction")
    }

    fn tx_punish(&self) -> TxPunish {
        bitcoin::TxPunish::new(
            &self.tx_cancel(),
            &self.punish_address,
            self.punish_timelock,
        )
    }
}
