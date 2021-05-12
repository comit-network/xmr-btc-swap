use crate::bitcoin::Txid;
use monero_adaptor::AdaptorSignature;

pub mod alice;
pub mod bob;
mod state_machine;
mod transactions;

pub struct SeenBtcLock {
    s_0_b: crate::monero::Scalar,
    pub adaptor_sig: AdaptorSignature,
    tx_lock_id: Txid,
    tx_lock: bitcoin::Transaction,
}
