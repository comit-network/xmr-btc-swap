use anyhow::Result;
use async_trait::async_trait;
use backoff::{future::FutureOperation as _, ExponentialBackoff};
use monero::{Address, Network, PrivateKey};
use monero_harness::Monero;
use std::str::FromStr;
use xmr_btc::monero::{
    Amount, CreateWalletForOutput, InsufficientFunds, PrivateViewKey, PublicKey, PublicViewKey,
    Transfer, TransferProof, TxHash, WatchForTransfer,
};

#[derive(Debug)]
pub struct AliceWallet<'c>(pub &'c Monero<'c>);

#[async_trait]
impl Transfer for AliceWallet<'_> {
    async fn transfer(
        &self,
        public_spend_key: PublicKey,
        public_view_key: PublicViewKey,
        amount: Amount,
    ) -> Result<(TransferProof, Amount)> {
        let destination_address =
            Address::standard(Network::Mainnet, public_spend_key, public_view_key.into());

        let res = self
            .0
            .transfer_from_alice(amount.as_piconero(), &destination_address.to_string())
            .await?;

        let tx_hash = TxHash(res.tx_hash);
        let tx_key = PrivateKey::from_str(&res.tx_key)?;

        let fee = Amount::from_piconero(res.fee);

        Ok((TransferProof::new(tx_hash, tx_key), fee))
    }
}

#[async_trait]
impl CreateWalletForOutput for AliceWallet<'_> {
    async fn create_and_load_wallet_for_output(
        &self,
        private_spend_key: PrivateKey,
        private_view_key: PrivateViewKey,
    ) -> Result<()> {
        let public_spend_key = PublicKey::from_private_key(&private_spend_key);
        let public_view_key = PublicKey::from_private_key(&private_view_key.into());

        let address = Address::standard(Network::Mainnet, public_spend_key, public_view_key);

        let _ = self
            .0
            .alice_wallet_rpc_client()
            .generate_from_keys(
                &address.to_string(),
                &private_spend_key.to_string(),
                &PrivateKey::from(private_view_key).to_string(),
            )
            .await?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct BobWallet<'c>(pub &'c Monero<'c>);

#[async_trait]
impl WatchForTransfer for BobWallet<'_> {
    async fn watch_for_transfer(
        &self,
        public_spend_key: PublicKey,
        public_view_key: PublicViewKey,
        transfer_proof: TransferProof,
        expected_amount: Amount,
        expected_confirmations: u32,
    ) -> Result<(), InsufficientFunds> {
        enum Error {
            TxNotFound,
            InsufficientConfirmations,
            InsufficientFunds { expected: Amount, actual: Amount },
        }

        let wallet = self.0.bob_wallet_rpc_client();
        let address = Address::standard(Network::Mainnet, public_spend_key, public_view_key.into());

        let res = (|| async {
            // NOTE: Currently, this is conflating IO errors with the transaction not being
            // in the blockchain yet, or not having enough confirmations on it. All these
            // errors warrant a retry, but the strategy should probably differ per case
            let proof = wallet
                .check_tx_key(
                    &String::from(transfer_proof.tx_hash()),
                    &transfer_proof.tx_key().to_string(),
                    &address.to_string(),
                )
                .await
                .map_err(|_| backoff::Error::Transient(Error::TxNotFound))?;

            if proof.received != expected_amount.as_piconero() {
                return Err(backoff::Error::Permanent(Error::InsufficientFunds {
                    expected: expected_amount,
                    actual: Amount::from_piconero(proof.received),
                }));
            }

            if proof.confirmations < expected_confirmations {
                return Err(backoff::Error::Transient(Error::InsufficientConfirmations));
            }

            Ok(proof)
        })
        .retry(ExponentialBackoff::default())
        .await;

        if let Err(Error::InsufficientFunds { expected, actual }) = res {
            return Err(InsufficientFunds { expected, actual });
        };

        Ok(())
    }
}

#[async_trait]
impl CreateWalletForOutput for BobWallet<'_> {
    async fn create_and_load_wallet_for_output(
        &self,
        private_spend_key: PrivateKey,
        private_view_key: PrivateViewKey,
    ) -> Result<()> {
        let public_spend_key = PublicKey::from_private_key(&private_spend_key);
        let public_view_key = PublicKey::from_private_key(&private_view_key.into());

        let address = Address::standard(Network::Mainnet, public_spend_key, public_view_key);

        let _ = self
            .0
            .bob_wallet_rpc_client()
            .generate_from_keys(
                &address.to_string(),
                &private_spend_key.to_string(),
                &PrivateKey::from(private_view_key).to_string(),
            )
            .await?;

        Ok(())
    }
}
