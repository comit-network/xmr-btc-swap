use crate::monero::wallet::{TransferRequest, WatchRequest};
use crate::monero::{Amount, PrivateKey, PrivateViewKey, Scalar, TransferProof};
use curve25519_dalek::edwards::EdwardsPoint;
use monero::PublicKey;

pub struct XmrRedeem {
    // recover s_a from btc_redeem
    s_a: monero::PrivateKey,
    s_b: monero::PrivateKey,
    v_a: PrivateViewKey,
    v_b: PrivateViewKey,
    // D: EdwardsPoint,
    amount: Amount,
}

impl XmrRedeem {
    pub fn new(
        s_a: monero::PrivateKey,
        s_b: monero::PrivateKey,
        v_a: PrivateViewKey,
        v_b: PrivateViewKey,
        // D: EdwardsPoint,
        amount: Amount,
    ) -> Self {
        Self {
            s_a,
            s_b,
            v_a,
            v_b,
            // D,
            amount,
        }
    }
    pub fn transfer_request(&self) -> TransferRequest {
        let v = self.v_a + self.v_b;
        // let h = self.D * v_view;
        // let private_spend_key = self.s_a + self.s_b + h;
        let vk = self.s_a + self.s_b;

        TransferRequest {
            public_spend_key: PublicKey::from_private_key(&vk),
            public_view_key: v.public(),
            amount: self.amount,
        }
    }
    pub fn watch_request(&self, transfer_proof: TransferProof) -> WatchRequest {
        let private_spend_key = self.s_a + self.s_b;
        let private_view_key = self.v_a + self.v_b;

        WatchRequest {
            public_spend_key: PublicKey::from_private_key(&private_spend_key),
            public_view_key: private_view_key.public(),
            transfer_proof,
            conf_target: 1,
            expected: self.amount,
        }
    }
}
