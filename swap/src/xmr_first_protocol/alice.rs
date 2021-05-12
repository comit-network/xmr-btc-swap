use anyhow::Result;
use monero::PublicKey;
use rand::rngs::OsRng;

use monero_adaptor::alice::Alice2;
use monero_adaptor::AdaptorSignature;

use crate::bitcoin::TxLock;
use crate::monero::{Scalar, TransferRequest};
use curve25519_dalek::edwards::EdwardsPoint;

// start
pub struct Alice3 {
    pub xmr_swap_amount: crate::monero::Amount,
    pub btc_swap_amount: crate::bitcoin::Amount,
    // pub adaptor_sig: AdaptorSignature,
    pub a: crate::bitcoin::SecretKey,
    pub B: crate::bitcoin::PublicKey,
    pub s_a: Scalar,
    pub S_b_monero: EdwardsPoint,
    pub v_a: crate::monero::PrivateViewKey,
}

// published xmr_lock, watching for btc_lock
pub struct Alice4 {
    a: crate::bitcoin::SecretKey,
    B: crate::bitcoin::PublicKey,
    btc_swap_amount: crate::bitcoin::Amount,
    // pub adaptor_sig: AdaptorSignature,
}

// published seen btc_lock, published btc_redeem
pub struct Alice5;

impl Alice3 {
    pub fn new(
        S_b_monero: EdwardsPoint,
        B: crate::bitcoin::PublicKey,
        xmr_swap_amount: crate::monero::Amount,
        btc_swap_amount: crate::bitcoin::Amount,
    ) -> Self {
        Self {
            xmr_swap_amount,
            btc_swap_amount,
            // adaptor_sig: alice2.adaptor_sig,
            a: crate::bitcoin::SecretKey::new_random(&mut OsRng),
            B,
            s_a: Scalar::random(&mut OsRng),
            S_b_monero,
            v_a: crate::monero::PrivateViewKey::new_random(&mut OsRng),
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
        let _ = wallet.transfer(req).await?;

        Ok(Alice4 {
            a: self.a.clone(),
            B: self.B,
            btc_swap_amount: Default::default(),
            // adaptor_sig: self.adaptor_sig.clone(),
        })
    }
}

impl Alice4 {
    pub async fn watch_for_btc_lock(&self, wallet: &crate::bitcoin::Wallet) -> Result<Alice5> {
        let btc_lock = TxLock::new(wallet, self.btc_swap_amount, self.a.public(), self.B).await?;
        wallet.subscribe_to(btc_lock);
        Ok(Alice5)
    }
}
