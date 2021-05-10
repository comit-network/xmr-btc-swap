use crate::monero::{PrivateViewKey, PublicKey, TransferProof};

pub struct XmrLock {
    pub public_spend_key: PublicKey,
    pub public_view_key: PrivateViewKey,
    pub transfer_proof: TransferProof,
}
