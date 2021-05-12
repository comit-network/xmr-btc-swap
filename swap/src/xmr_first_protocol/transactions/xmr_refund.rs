use crate::monero::TransferRequest;
use crate::xmr_first_protocol::alice::Alice4;
use anyhow::Result;
use monero_adaptor::AdaptorSignature;

pub struct XmrRefund {
    adaptor: AdaptorSignature,
}

impl XmrRefund {
    pub async fn publish_xmr_refund(&self, wallet: &crate::monero::Wallet) -> Result<()> {
        let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey { scalar: self.s_a });

        let public_spend_key = S_a + self.S_b_monero;
        let public_view_key = self.v_a.public();

        let req = TransferRequest {
            public_spend_key,
            public_view_key,
            amount: self.xmr_swap_amount,
        };

        let _ = wallet.transfer(req).await?;
        Ok(())
    }
}
