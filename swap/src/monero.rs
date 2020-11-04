use anyhow::Result;
use async_trait::async_trait;
use backoff::{backoff::Constant as ConstantBackoff, future::FutureOperation as _};
use futures::TryFutureExt;
use monero::{Address, Network, PrivateKey};
use monero_harness::rpc::wallet;
use std::time::Duration;
use url::Url;
pub use xmr_btc::monero::{
    Amount, CreateWalletForOutput, InsufficientFunds, PrivateViewKey, PublicKey, PublicViewKey,
    Transfer, *,
};

pub struct Facade {
    pub user_wallet: wallet::Client,
    pub watch_only_wallet: wallet::Client,
}

impl Facade {
    pub fn new(user_url: Url, watch_only_url: Url) -> Self {
        Self {
            user_wallet: wallet::Client::new(user_url),
            watch_only_wallet: wallet::Client::new(watch_only_url),
        }
    }

    /// Get the balance of the primary account.
    pub async fn get_balance(&self) -> Result<Amount> {
        let amount = self.user_wallet.get_balance(0).await?;

        Ok(Amount::from_piconero(amount))
    }
}

#[async_trait]
impl Transfer for Facade {
    async fn transfer(
        &self,
        public_spend_key: PublicKey,
        public_view_key: PublicViewKey,
        amount: Amount,
    ) -> Result<Amount> {
        let destination_address =
            Address::standard(Network::Mainnet, public_spend_key, public_view_key.into());

        let res = self
            .user_wallet
            .transfer(0, amount.as_piconero(), &destination_address.to_string())
            .await?;

        Ok(Amount::from_piconero(res.fee))
    }
}

#[async_trait]
impl CreateWalletForOutput for Facade {
    async fn create_and_load_wallet_for_output(
        &self,
        private_spend_key: PrivateKey,
        private_view_key: PrivateViewKey,
    ) -> Result<()> {
        let public_spend_key = PublicKey::from_private_key(&private_spend_key);
        let public_view_key = PublicKey::from_private_key(&private_view_key.into());

        let address = Address::standard(Network::Mainnet, public_spend_key, public_view_key);

        let _ = self
            .user_wallet
            .generate_from_keys(
                &address.to_string(),
                Some(&private_spend_key.to_string()),
                &PrivateKey::from(private_view_key).to_string(),
            )
            .await?;

        Ok(())
    }
}

// TODO: For retry, use `backoff::ExponentialBackoff` in production as opposed
// to `ConstantBackoff`.

#[async_trait]
impl WatchForTransfer for Facade {
    async fn watch_for_transfer(
        &self,
        address: Address,
        expected_amount: Amount,
        private_view_key: PrivateViewKey,
    ) {
        let address = address.to_string();
        let private_view_key = PrivateKey::from(private_view_key).to_string();
        let load_address = || {
            self.watch_only_wallet
                .generate_from_keys(&address, None, &private_view_key)
                .map_err(backoff::Error::Transient)
        };

        // QUESTION: Should we really retry every error?
        load_address
            .retry(ConstantBackoff::new(Duration::from_secs(1)))
            .await
            .expect("transient error is never returned");

        // QUESTION: Should we retry this error at all?
        let refresh = || {
            self.watch_only_wallet
                .refresh()
                .map_err(backoff::Error::Transient)
        };

        refresh
            .retry(ConstantBackoff::new(Duration::from_secs(1)))
            .await
            .expect("transient error is never returned");

        let check_balance = || async {
            let balance = self
                .watch_only_wallet
                .get_balance(0)
                .await
                .map_err(|_| backoff::Error::Transient("io"))?;
            let balance = Amount::from_piconero(balance);

            if balance != expected_amount {
                return Err(backoff::Error::Transient("insufficient funds"));
            }

            Ok(())
        };

        check_balance
            .retry(ConstantBackoff::new(Duration::from_secs(1)))
            .await
            .expect("transient error is never returned");
    }
}
