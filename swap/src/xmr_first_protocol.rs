use crate::bitcoin::Txid;
use crate::monero::wallet::WatchRequest;
use crate::monero::{PrivateViewKey, Scalar, TransferRequest};
use crate::xmr_first_protocol::transactions::xmr_lock::XmrLock;
use anyhow::Result;
use monero::PublicKey;
use monero_adaptor::alice::Alice2;
use monero_adaptor::AdaptorSignature;
use rand::rngs::OsRng;

// start
pub struct Alice3 {
    pub adaptor_sig: AdaptorSignature,
    s_a: Scalar,
    S_b_monero: monero::PublicKey,
}

// published xmr_lock, watching for btc_lock
pub struct Alice4 {
    pub adaptor_sig: AdaptorSignature,
}

// published seen btc_lock, published btc_redeem
pub struct Alice5 {}

// watching for xmr_lock
pub struct Bob3 {
    xmr_swap_amount: crate::monero::Amount,
    btc_swap_amount: crate::bitcoin::Amount,
    xmr_lock: XmrLock,
}

impl Bob3 {
    pub fn watch_for_lock_xmr(&self, wallet: &crate::monero::Wallet) {
        let req = WatchRequest {
            public_spend_key: self.xmr_lock.public_spend_key,
            public_view_key: self.xmr_lock.public_view_key,
            transfer_proof: self.xmr_lock.transfer_proof.clone(),
            conf_target: 1,
            expected: self.xmr_swap_amount,
        };
        wallet.watch_for_transfer(req);
    }
}

// published btc_lock, watching for xmr_redeem
pub struct Bob4;

mod alice;
mod bob;
mod transactions;

impl Alice3 {
    pub fn new(alice2: Alice2, S_b_monero: PublicKey) -> Self {
        Self {
            adaptor_sig: alice2.adaptor_sig,
            s_a: Scalar::random(&mut OsRng),
            S_b_monero,
        }
    }
    pub fn publish_xmr_lock(&self, wallet: &crate::monero::Wallet) -> Result<Alice4> {
        let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey { scalar: self.s_a });

        let public_spend_key = S_a + self.S_b_monero;
        let public_view_key = self.v.public();

        let req = TransferRequest {
            public_spend_key,
            public_view_key,
            amount: self.xmr,
        };

        // we may have to send this to Bob
        let _ = wallet.transfer(req)?;
    }
}

impl Alice4 {
    pub fn watch_for_btc_lock(&self, wallet: &crate::bitcoin::Wallet) -> Result<Alice4> {
        wallet.subscribe_to(self.btc_lock());
    }
}

pub struct SeenBtcLock {
    s_0_b: monero::Scalar,
    pub adaptor_sig: AdaptorSignature,
    tx_lock_id: Txid,
    tx_lock: bitcoin::Transaction,
}

#[cfg(test)]
mod test {
    use crate::monero::Scalar;
    use crate::xmr_first_protocol::Alice3;
    use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
    use curve25519_dalek::edwards::EdwardsPoint;
    use monero_adaptor::alice::Alice0;
    use monero_adaptor::bob::Bob0;
    use rand::rngs::OsRng;

    #[test]
    fn happy_path() {
        let msg_to_sign = b"hello world, monero is amazing!!";

        let s_prime_a = Scalar::random(&mut OsRng);
        let s_b = Scalar::random(&mut OsRng);

        let pk = (s_prime_a + s_b) * ED25519_BASEPOINT_POINT;

        let (r_a, R_a, R_prime_a) = {
            let r_a = Scalar::random(&mut OsRng);
            let R_a = r_a * ED25519_BASEPOINT_POINT;

            let pk_hashed_to_point = hash_point_to_point(pk);

            let R_prime_a = r_a * pk_hashed_to_point;

            (r_a, R_a, R_prime_a)
        };

        let mut ring = [EdwardsPoint::default(); RING_SIZE];
        ring[0] = pk;

        ring[1..].fill_with(|| {
            let x = Scalar::random(&mut OsRng);

            x * ED25519_BASEPOINT_POINT
        });

        let alice = Alice0::new(ring, *msg_to_sign, R_a, R_prime_a, s_prime_a).unwrap();
        let bob = Bob0::new(ring, *msg_to_sign, R_a, R_prime_a, s_b).unwrap();

        let msg = alice.next_message();
        let bob = bob.receive(msg);

        let msg = bob.next_message();
        let alice = alice.receive(msg).unwrap();

        let msg = alice.next_message();
        let bob = bob.receive(msg).unwrap();

        let msg = bob.next_message();
        let alice = alice.receive(msg);

        let sig = alice.adaptor_sig.adapt(r_a);

        assert!(sig.verify(ring, msg_to_sign).unwrap());

        let alice = Alice::new(alice);
    }
}
