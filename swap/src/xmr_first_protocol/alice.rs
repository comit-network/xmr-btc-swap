use anyhow::Result;
use rand::rngs::OsRng;

use crate::bitcoin::EncryptedSignature;
use crate::monero::{Scalar, TransferProof, TransferRequest};
use crate::monero_ext::ScalarExt;
use crate::xmr_first_protocol::transactions::btc_lock::BtcLock;
use crate::xmr_first_protocol::transactions::btc_redeem::BtcRedeem;

// start
pub struct Alice3 {
    pub xmr_swap_amount: crate::monero::Amount,
    pub btc_swap_amount: crate::bitcoin::Amount,
    // pub adaptor_sig: AdaptorSignature,
    // adaptor
    // pub r_a: Scalar,
    pub a: crate::bitcoin::SecretKey,
    pub B: crate::bitcoin::PublicKey,
    pub s_a: Scalar,
    pub S_b_monero: crate::monero::PublicKey,
    pub v_a: crate::monero::PrivateViewKey,
    pub redeem_address: bitcoin::Address,
}

// published xmr_lock, watching for btc_lock
pub struct Alice4 {
    pub a: crate::bitcoin::SecretKey,
    pub B: crate::bitcoin::PublicKey,
    // pub r_a: Scalar,
    pub s_a: Scalar,
    btc_swap_amount: crate::bitcoin::Amount,
    pub transfer_proof: TransferProof,
    pub redeem_address: bitcoin::Address,
    // pub adaptor_sig: AdaptorSignature,
}

// published seen btc_lock, published btc_redeem
pub struct Alice5 {
    pub a: crate::bitcoin::SecretKey,
    pub B: crate::bitcoin::PublicKey,
    // pub r_a: Scalar,
    pub s_a: Scalar,
    pub redeem_address: bitcoin::Address,
    btc_swap_amount: crate::bitcoin::Amount,
}

impl Alice3 {
    pub fn new(
        S_b_monero: crate::monero::PublicKey,
        B: crate::bitcoin::PublicKey,
        xmr_swap_amount: crate::monero::Amount,
        btc_swap_amount: crate::bitcoin::Amount,
        redeem_address: bitcoin::Address,
    ) -> Self {
        Self {
            xmr_swap_amount,
            btc_swap_amount,
            // r_a: Default::default(),
            a: crate::bitcoin::SecretKey::new_random(&mut OsRng),
            B,
            s_a: Scalar::random(&mut OsRng),
            S_b_monero,
            v_a: crate::monero::PrivateViewKey::new_random(&mut OsRng),
            redeem_address,
        }
    }
    pub async fn publish_xmr_lock(&self, wallet: &crate::monero::Wallet) -> Result<Alice4> {
        let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey { scalar: self.s_a });

        let public_spend_key = S_a + self.S_b_monero;
        let public_view_key = self.v_a.public();

        let req = TransferRequest {
            public_spend_key,
            public_view_key,
            amount: self.xmr_swap_amount,
        };

        // we may have to send this to Bob
        let transfer_proof = wallet.transfer(req).await?;

        Ok(Alice4 {
            a: self.a.clone(),
            B: self.B,
            // r_a: Default::default(),
            s_a: self.s_a,
            btc_swap_amount: self.btc_swap_amount,
            transfer_proof,
            // adaptor_sig: self.adaptor_sig.clone(),
            redeem_address: self.redeem_address.clone(),
        })
    }

    // pub async fn publish_xmr_refund(&self, refund_xmr: XmrRefund) -> Result<()> {
    //     let sig = refund_xmr.adaptor.adapt(self.r_a);
    //     todo!("sig");
    //     Ok(())
    // }
}

impl Alice4 {
    pub async fn watch_for_btc_lock(&self, wallet: &crate::bitcoin::Wallet) -> Result<Alice5> {
        let btc_lock = BtcLock::new(wallet, self.btc_swap_amount, self.a.public(), self.B).await?;
        let btc_lock_watcher = wallet.subscribe_to(btc_lock).await;

        btc_lock_watcher.wait_until_confirmed_with(1).await?;

        Ok(Alice5 {
            a: self.a.clone(),
            B: self.B,
            // r_a: Default::default(),
            s_a: self.s_a,
            redeem_address: self.redeem_address.clone(),
            btc_swap_amount: self.btc_swap_amount,
        })
    }
}

impl Alice5 {
    pub async fn publish_btc_redeem(
        &self,
        wallet: &crate::bitcoin::Wallet,
        encsig: EncryptedSignature,
    ) -> Result<()> {
        let tx_lock = BtcLock::new(wallet, self.btc_swap_amount, self.a.public(), self.B).await?;
        let tx_redeem = BtcRedeem::new(&tx_lock, &self.redeem_address);

        let signed_tx_redeem =
            tx_redeem.complete(self.a.clone(), self.s_a.to_secpfun_scalar(), self.B, encsig)?;

        let (txid, sub) = wallet.broadcast(signed_tx_redeem, "lock").await?;

        let _ = sub.wait_until_confirmed_with(1).await?;

        Ok(())
    }
}
