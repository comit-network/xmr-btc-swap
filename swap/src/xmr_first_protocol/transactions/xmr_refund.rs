use crate::monero::wallet::WatchRequest;
use crate::monero::{Amount, PrivateViewKey, Scalar};
use crate::xmr_first_protocol::alice::Alice4;
use anyhow::Result;
use monero_adaptor::Signature;

pub struct XmrRefund {
    signature: Signature,
    amount: Amount,
}

struct TransferRequest;

impl XmrRefund {
    pub fn new(signature: Signature, amount: Amount) -> Self {
        XmrRefund {
            signature,
            amount: xmr_swap_amount,
        }
    }
    pub fn transfer_request(&self) -> TransferRequest {
        todo!();
        TransferRequest
    }
    // pub fn watch_request(&self) -> WatchRequest {
    //     let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey {
    // scalar: self.s_a });
    //
    //     let public_spend_key = S_a + self.S_b_monero;
    //     let public_view_key = self.v_a.public();
    //
    //     WatchRequest {
    //         public_spend_key,
    //         public_view_key,
    //         transfer_proof: todo!("xfer without broadcasting to get xfer proof"),
    //         conf_target: 1,
    //         expected: self.amount,
    //     }
    // }
    pub fn extract_r_a(&self) -> Scalar {
        self.signature.extract()
    }
}
