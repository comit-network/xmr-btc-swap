use anyhow::Result;
use monero::PublicKey;
use rand::rngs::OsRng;

use monero_adaptor::alice::Alice2;
use monero_adaptor::AdaptorSignature;

use crate::bitcoin::Txid;
use crate::monero::wallet::WatchRequest;
use crate::monero::{Scalar, TransferRequest};
use crate::xmr_first_protocol::transactions::xmr_lock::XmrLock;

// watching for xmr_lock
pub struct Bob3 {
    pub xmr_swap_amount: crate::monero::Amount,
    pub btc_swap_amount: crate::bitcoin::Amount,
    pub xmr_lock: XmrLock,
    v_b: crate::monero::PrivateViewKey,
}

impl Bob3 {
    pub fn watch_for_lock_xmr(&self, wallet: &crate::monero::Wallet) {
        let req = WatchRequest {
            public_spend_key: self.xmr_lock.public_spend_key,
            public_view_key: self.v_b.public(),
            transfer_proof: self.xmr_lock.transfer_proof.clone(),
            conf_target: 1,
            expected: self.xmr_swap_amount,
        };
        wallet.watch_for_transfer(req);
    }
}

// published btc_lock, watching for xmr_redeem
pub struct Bob4;
