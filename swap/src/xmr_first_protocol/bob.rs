use crate::monero::wallet::{TransferRequest, WatchRequest};
use crate::monero::TransferProof;
use crate::xmr_first_protocol::transactions::btc_lock::BtcLock;
use crate::xmr_first_protocol::transactions::btc_redeem::BtcRedeem;
use anyhow::Result;
use monero_rpc::wallet::BlockHeight;
use uuid::Uuid;

// watching for xmr_lock
pub struct Bob3 {
    pub b: crate::bitcoin::SecretKey,
    pub A: crate::bitcoin::PublicKey,
    pub s_b: crate::monero::Scalar,
    pub xmr_swap_amount: crate::monero::Amount,
    pub btc_swap_amount: crate::bitcoin::Amount,
    pub tx_lock: BtcLock,
    // public spend key
    pub S: crate::monero::PublicKey,
    pub S_a_bitcoin: crate::bitcoin::PublicKey,
    pub v: crate::monero::PrivateViewKey,
    pub alice_redeem_address: bitcoin::Address,
}

impl Bob3 {
    pub async fn watch_for_lock_xmr(
        &self,
        xmr_wallet: &crate::monero::Wallet,
        btc_wallet: &crate::bitcoin::Wallet,
        transfer_proof: TransferProof,
        alice_redeem_address: bitcoin::Address,
    ) -> Result<Bob4> {
        let req = WatchRequest {
            public_spend_key: self.S,
            public_view_key: self.v.public(),
            transfer_proof,
            conf_target: 1,
            expected: self.xmr_swap_amount,
        };
        let _ = xmr_wallet.watch_for_transfer(req).await?;

        let signed_tx_lock = btc_wallet
            .sign_and_finalize(self.tx_lock.clone().into())
            .await?;

        let (_txid, sub) = btc_wallet.broadcast(signed_tx_lock, "lock").await?;

        let _ = sub.wait_until_confirmed_with(1).await?;

        Ok(Bob4 {
            b: self.b.clone(),
            A: self.A,
            s_b: self.s_b,
            S_a_bitcoin: self.S_a_bitcoin,
            tx_lock: self.tx_lock.clone(),
            alice_redeem_address,
            v: self.v,
        })
    }

    pub async fn emergency_refund_if_refund_xmr_seen(
        &self,
        xmr_wallet: &crate::monero::Wallet,
        btc_wallet: &crate::bitcoin::Wallet,
        transfer_proof: TransferProof,
    ) -> Result<Bob4> {
        let req = WatchRequest {
            public_spend_key: todo!(),
            public_view_key: todo!(),
            transfer_proof,
            conf_target: 1,
            expected: self.xmr_swap_amount,
        };
        let _ = xmr_wallet.watch_for_transfer(req).await?;

        let emergency_refund = btc_wallet
            .sign_and_finalize(self.tx_lock.clone().into())
            .await?;

        let (_txid, sub) = btc_wallet.broadcast(emergency_refund, "lock").await?;

        let _ = sub.wait_until_confirmed_with(1).await?;

        Ok(Bob4 {
            b: self.b.clone(),
            A: self.A,
            s_b: self.s_b,
            S_a_bitcoin: self.S_a_bitcoin,
            tx_lock: self.tx_lock.clone(),
            alice_redeem_address: self.alice_redeem_address.clone(),
            v: self.v,
        })
    }
}

// published btc_lock, watching for btc_redeem
pub struct Bob4 {
    pub b: crate::bitcoin::SecretKey,
    pub A: crate::bitcoin::PublicKey,
    pub s_b: crate::monero::Scalar,
    pub S_a_bitcoin: crate::bitcoin::PublicKey,
    pub tx_lock: BtcLock,
    pub alice_redeem_address: crate::bitcoin::Address,
    pub v: crate::monero::PrivateViewKey,
}

impl Bob4 {
    pub async fn redeem_xmr_when_btc_redeem_seen(
        &self,
        bitcoin_wallet: &crate::bitcoin::Wallet,
        monero_wallet: &crate::monero::Wallet,
        swap_id: Uuid,
    ) -> Result<()> {
        let btc_redeem = BtcRedeem::new(&self.tx_lock, &self.alice_redeem_address);
        let btc_redeem_encsig = self.b.encsign(self.S_a_bitcoin, btc_redeem.digest());

        let btc_redeem_watcher = bitcoin_wallet.subscribe_to(btc_redeem.clone()).await;

        btc_redeem_watcher.wait_until_confirmed_with(1).await?;

        let btc_redeem_candidate = bitcoin_wallet
            .get_raw_transaction(btc_redeem.txid())
            .await?;

        let btc_redeem_sig =
            btc_redeem.extract_signature_by_key(btc_redeem_candidate, self.b.public())?;
        let s_a = crate::bitcoin::recover(self.S_a_bitcoin, btc_redeem_sig, btc_redeem_encsig)?;
        let s_a = crate::monero::private_key_from_secp256k1_scalar(s_a.into());

        let (spend_key, view_key) = {
            let s_b = monero::PrivateKey { scalar: self.s_b };
            let s = s_a + s_b;

            (s, self.v)
        };

        monero_wallet
            .create_from_and_load(
                &swap_id.to_string(),
                spend_key,
                view_key,
                monero_rpc::wallet::BlockHeight { height: 0 },
            )
            .await?;

        Ok(())
    }
}
