use anyhow::Result;
use async_trait::async_trait;
use backoff::{backoff::Constant as ConstantBackoff, future::FutureOperation as _};
use monero_harness::rpc::wallet;
use std::{str::FromStr, time::Duration};
use url::Url;

pub use xmr_btc::monero::*;

#[derive(Debug)]
pub struct Wallet {
    pub inner: wallet::Client,
    pub network: Network,
}

impl Wallet {
    pub fn new(url: Url, network: Network) -> Self {
        Self {
            inner: wallet::Client::new(url),
            network,
        }
    }

    /// Get the balance of the primary account.
    pub async fn get_balance(&self) -> Result<Amount> {
        let amount = self.inner.get_balance(0).await?;

        Ok(Amount::from_piconero(amount))
    }
}

#[async_trait]
impl Transfer for Wallet {
    async fn transfer(
        &self,
        public_spend_key: PublicKey,
        public_view_key: PublicViewKey,
        amount: Amount,
    ) -> Result<(TransferProof, Amount)> {
        let destination_address =
            Address::standard(self.network, public_spend_key, public_view_key.into());

        let res = self
            .inner
            .transfer(0, amount.as_piconero(), &destination_address.to_string())
            .await?;

        let tx_hash = TxHash(res.tx_hash);
        tracing::info!("Monero tx broadcasted!, tx hash: {:?}", tx_hash);
        let tx_key = PrivateKey::from_str(&res.tx_key)?;

        let fee = Amount::from_piconero(res.fee);

        let transfer_proof = TransferProof::new(tx_hash, tx_key);
        tracing::debug!("  Transfer proof: {:?}", transfer_proof);

        Ok((transfer_proof, fee))
    }
}

#[async_trait]
impl CreateWalletForOutput for Wallet {
    async fn create_and_load_wallet_for_output(
        &self,
        private_spend_key: PrivateKey,
        private_view_key: PrivateViewKey,
        restore_height: Option<u32>,
    ) -> Result<()> {
        let public_spend_key = PublicKey::from_private_key(&private_spend_key);
        let public_view_key = PublicKey::from_private_key(&private_view_key.into());

        let address = Address::standard(self.network, public_spend_key, public_view_key);

        let _ = self
            .inner
            .generate_from_keys(
                &address.to_string(),
                &private_spend_key.to_string(),
                &PrivateKey::from(private_view_key).to_string(),
                restore_height,
            )
            .await?;

        Ok(())
    }
}

// TODO: For retry, use `backoff::ExponentialBackoff` in production as opposed
// to `ConstantBackoff`.

#[async_trait]
impl WatchForTransfer for Wallet {
    async fn watch_for_transfer(
        &self,
        public_spend_key: PublicKey,
        public_view_key: PublicViewKey,
        transfer_proof: TransferProof,
        expected_amount: Amount,
        expected_confirmations: u32,
    ) -> Result<TransferInfo> {
        enum Error {
            TxNotFound,
            BlockHeight,
            InsufficientConfirmations,
            InsufficientFunds { expected: Amount, actual: Amount },
        }

        let address = Address::standard(self.network, public_spend_key, public_view_key.into());

        let result = (|| async {
            // NOTE: Currently, this is conflating IO errors with the transaction not being
            // in the blockchain yet, or not having enough confirmations on it. All these
            // errors warrant a retry, but the strategy should probably differ per case
            let check_tx_pay_response = self
                .inner
                .check_tx_key(
                    &String::from(transfer_proof.tx_hash()),
                    &transfer_proof.tx_key().to_string(),
                    &address.to_string(),
                )
                .await
                .map_err(|_| backoff::Error::Transient(Error::TxNotFound))?;

            if check_tx_pay_response.received != expected_amount.as_piconero() {
                return Err(backoff::Error::Permanent(Error::InsufficientFunds {
                    expected: expected_amount,
                    actual: Amount::from_piconero(check_tx_pay_response.received),
                }));
            }

            if check_tx_pay_response.confirmations < expected_confirmations {
                return Err(backoff::Error::Transient(Error::InsufficientConfirmations));
            }

            let current_block_height = self
                .inner
                .block_height()
                .await
                .map_err(|_| backoff::Error::Transient(Error::BlockHeight))?;

            let transfer_info = TransferInfo {
                // Substract 1 just in case a block got mined between the two rpc calls.
                // This is a hack as we know this is going to be used to set the wallet's block
                // height.
                first_confirmation_block_height: current_block_height.height
                    - check_tx_pay_response.confirmations
                    - 1,
            };

            Ok((check_tx_pay_response, transfer_info))
        })
        .retry(ConstantBackoff::new(Duration::from_secs(1)))
        .await;

        match result {
            Ok((_, transfer_info)) => Ok(transfer_info),
            Err(Error::InsufficientFunds { expected, actual }) => {
                anyhow::bail!(InsufficientFunds { expected, actual })
            }
            Err(Error::BlockHeight)
            | Err(Error::TxNotFound)
            | Err(Error::InsufficientConfirmations) => {
                unreachable!("Transient backoff error will never be returned")
            }
        }
    }
}
