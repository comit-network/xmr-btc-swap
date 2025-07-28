//! This module contains the [`Wallets`] struct, which we use to manage and access the
//! Monero blockchain and wallets.
//!
//! Mostly we do two things:
//!  - wait for transactions to be confirmed
//!  - send money from one wallet to another.

use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::common::throttle::{throttle, Throttle};
use anyhow::{Context, Result};
use monero::{Address, Network};
use monero_sys::WalletEventListener;
pub use monero_sys::{Daemon, WalletHandle as Wallet, WalletHandleListener};
use uuid::Uuid;

use crate::cli::api::{
    request::{GetMoneroBalanceResponse, GetMoneroHistoryResponse, GetMoneroSyncProgressResponse},
    tauri_bindings::{MoneroWalletUpdate, TauriEmitter, TauriEvent, TauriHandle},
};

use super::{BlockHeight, TransferProof, TxHash};

/// Entrance point to the Monero blockchain.
/// You can use this struct to open specific wallets and monitor the blockchain.
pub struct Wallets {
    /// The directory we store the wallets in.
    wallet_dir: PathBuf,
    /// The network we're on.
    network: Network,
    /// The monero node we connect to.
    daemon: Daemon,
    /// Keep the main wallet open and synced.
    main_wallet: Arc<Wallet>,
    /// Since Network::Regtest isn't a thing we have to use an extra flag.
    /// When we're in regtest mode, we need to unplug some safty nets to make the wallet work.
    regtest: bool,
    /// A handle we use to send status updates to the UI i.e. when
    /// waiting for a transaction to be confirmed.
    #[expect(dead_code)]
    tauri_handle: Option<TauriHandle>,
    /// Database for tracking wallet usage history.
    wallet_database: Option<Arc<monero_sys::Database>>,
}

/// A request to watch for a transfer.
pub struct WatchRequest {
    pub public_view_key: super::PublicViewKey,
    pub public_spend_key: monero::PublicKey,
    /// The proof of the transfer.
    pub transfer_proof: TransferProof,
    /// The expected amount of the transfer.
    pub expected_amount: monero::Amount,
    /// The number of confirmations required for the transfer to be considered confirmed.
    pub confirmation_target: u64,
}

/// Transfer a specified amount of money to a specified address.
pub struct TransferRequest {
    pub public_spend_key: monero::PublicKey,
    pub public_view_key: super::PublicViewKey,
    pub amount: monero::Amount,
}

struct TauriWalletListener {
    // one throttle wrapper per expensive update
    balance_throttle: Throttle<()>,
    history_throttle: Throttle<()>,
    sync_throttle: Throttle<()>,
    save_throttle: Throttle<()>,
}

impl TauriWalletListener {
    const BALANCE_UPDATE_THROTTLE: Duration = Duration::from_millis(2 * 1000);
    const HISTORY_UPDATE_THROTTLE: Duration = Duration::from_millis(2 * 1000);
    const SYNC_UPDATE_THROTTLE: Duration = Duration::from_millis(2 * 1000);
    const SAVE_UPDATE_THROTTLE: Duration = Duration::from_millis(60 * 1000);

    pub async fn new(tauri_handle: TauriHandle, wallet: Arc<Wallet>) -> Self {
        let rt_handle = tokio::runtime::Handle::current();

        let balance_job = {
            let wallet = wallet.clone();
            let tauri = tauri_handle.clone();
            let rt = rt_handle.clone();
            move |()| {
                let wallet = wallet.clone();
                let tauri = tauri.clone();
                let rt = rt.clone();
                rt.spawn(async move {
                    let response = GetMoneroBalanceResponse {
                        total_balance: wallet.total_balance().await.into(),
                        unlocked_balance: wallet.unlocked_balance().await.into(),
                    };
                    tauri.emit_unified_event(TauriEvent::MoneroWalletUpdate(
                        MoneroWalletUpdate::BalanceChange(response),
                    ));
                });
            }
        };

        let history_job = {
            let wallet = wallet.clone();
            let tauri = tauri_handle.clone();
            let rt = rt_handle.clone();
            move |()| {
                let wallet = wallet.clone();
                let tauri = tauri.clone();
                let rt = rt.clone();
                rt.spawn(async move {
                    let transactions = wallet.history().await;
                    let response = GetMoneroHistoryResponse { transactions };

                    tauri.emit_unified_event(TauriEvent::MoneroWalletUpdate(
                        MoneroWalletUpdate::HistoryUpdate(response),
                    ));
                });
            }
        };

        let sync_job = {
            let wallet = wallet.clone();
            let tauri = tauri_handle.clone();
            let rt = rt_handle.clone();
            move |()| {
                let wallet = wallet.clone();
                let tauri = tauri.clone();
                let rt = rt.clone();
                rt.spawn(async move {
                    let sync_progress = wallet.sync_progress().await;

                    let progress_percentage = sync_progress.percentage();

                    let response = GetMoneroSyncProgressResponse {
                        current_block: sync_progress.current_block,
                        target_block: sync_progress.target_block,
                        progress_percentage: progress_percentage,
                    };

                    tauri.emit_unified_event(TauriEvent::MoneroWalletUpdate(
                        MoneroWalletUpdate::SyncProgress(response),
                    ));
                });
            }
        };

        let save_job = {
            let wallet = wallet.clone();
            let rt = rt_handle.clone();
            move |()| {
                let wallet = wallet.clone();
                let rt = rt.clone();
                rt.spawn(async move {
                    wallet.store(None).await;
                });
            }
        };

        Self {
            balance_throttle: throttle(balance_job, Self::BALANCE_UPDATE_THROTTLE),
            history_throttle: throttle(history_job, Self::HISTORY_UPDATE_THROTTLE),
            sync_throttle: throttle(sync_job, Self::SYNC_UPDATE_THROTTLE),
            save_throttle: throttle(save_job, Self::SAVE_UPDATE_THROTTLE),
        }
    }

    fn send_balance_update(&self) {
        self.balance_throttle.call(());
    }

    fn send_history_update(&self) {
        self.history_throttle.call(());
    }

    fn send_sync_progress(&self) {
        self.sync_throttle.call(());
    }

    fn save_wallet(&self) {
        self.save_throttle.call(());
    }
}

impl WalletEventListener for TauriWalletListener {
    fn on_money_spent(&self, _txid: &str, _amount: u64) {
        self.send_balance_update();
        self.send_history_update();
        self.save_wallet();
    }

    fn on_money_received(&self, _txid: &str, _amount: u64) {
        self.send_balance_update();
        self.send_history_update();
        self.save_wallet();
    }

    fn on_unconfirmed_money_received(&self, _txid: &str, _amount: u64) {
        self.send_balance_update();
        self.send_history_update();
        self.save_wallet();
    }

    fn on_new_block(&self, _height: u64) {
        // We send an update here because a new might mean that funds have been unlocked
        // because a UTXO reached 10 confirmations.
        self.send_sync_progress();
    }

    fn on_updated(&self) {
        self.send_balance_update();
    }

    fn on_refreshed(&self) {
        //self.wallet.start_refresh_thread();
        self.send_balance_update();
        self.send_history_update();
        self.save_wallet();
    }

    fn on_reorg(&self, _height: u64, _blocks_detached: u64, _transfers_detached: usize) {
        // We send an update here because a reorg might mean that a UTXO has been double spent
        // or that a UTXO has been confirmed is now unconfirmed.
        self.send_balance_update();
    }

    fn on_pool_tx_removed(&self, _txid: &str) {
        // We send an update here because a pool tx removed might mean that our unconfirmed
        // balance has gone down because a UTXO has been removed from the pool.
        self.send_balance_update();
    }
}

impl Wallets {
    /// Create a new `Wallets` instance.
    /// Wallets will be opened on the specified network, connected to the specified daemon
    /// and stored in the specified directory.
    ///
    /// The main wallet will be kept alive and synced, other wallets are
    /// opened and closed on demand.
    pub async fn new(
        wallet_dir: PathBuf,
        main_wallet_name: String,
        daemon: Daemon,
        network: Network,
        regtest: bool,
        tauri_handle: Option<TauriHandle>,
        wallet_database: Option<Arc<monero_sys::Database>>,
    ) -> Result<Self> {
        let main_wallet = Wallet::open_or_create(
            wallet_dir.join(&main_wallet_name).display().to_string(),
            daemon.clone(),
            network,
            true,
        )
        .await
        .context("Failed to open main wallet")?;

        if regtest {
            main_wallet.unsafe_prepare_for_regtest().await;
        }

        let main_wallet = Arc::new(main_wallet);

        if let Some(tauri_handle) = tauri_handle.clone() {
            let tauri_wallet_listener =
                TauriWalletListener::new(tauri_handle, main_wallet.clone()).await;

            let handle_listener = WalletHandleListener::new(main_wallet.clone());

            main_wallet
                .call(move |wallet| {
                    wallet.add_listener(Box::new(tauri_wallet_listener));
                    wallet.add_listener(Box::new(handle_listener));
                })
                .await;
        }

        let wallets = Self {
            wallet_dir,
            network,
            daemon,
            main_wallet,
            regtest,
            tauri_handle,
            wallet_database,
        };

        // Record wallet access in database
        let wallet_path = wallets.main_wallet.path().await;
        let _ = wallets.record_wallet_access(&wallet_path).await;

        Ok(wallets)
    }

    /// Create a new `Wallets` instance with an existing wallet as the main wallet.
    /// This is used when we want to use a user-selected wallet instead of creating a new one.
    pub async fn new_with_existing_wallet(
        wallet_dir: PathBuf,
        daemon: Daemon,
        network: Network,
        regtest: bool,
        tauri_handle: Option<TauriHandle>,
        existing_wallet: Wallet,
        wallet_database: Option<Arc<monero_sys::Database>>,
    ) -> Result<Self> {
        if regtest {
            existing_wallet.unsafe_prepare_for_regtest().await;
        }

        let main_wallet = Arc::new(existing_wallet);

        if let Some(tauri_handle) = tauri_handle.clone() {
            let tauri_wallet_listener =
                TauriWalletListener::new(tauri_handle, main_wallet.clone()).await;

            let handle_listener = WalletHandleListener::new(main_wallet.clone());

            main_wallet
                .call(move |wallet| {
                    wallet.add_listener(Box::new(tauri_wallet_listener));
                    wallet.add_listener(Box::new(handle_listener));
                })
                .await;
        }

        let wallets = Self {
            wallet_dir,
            network,
            daemon,
            main_wallet,
            regtest,
            tauri_handle,
            wallet_database,
        };

        // Record wallet access in database
        let wallet_path = wallets.main_wallet.path().await;
        let _ = wallets.record_wallet_access(&wallet_path).await;

        Ok(wallets)
    }

    /// Open the lock wallet of a specific swap.
    /// Used to redeem (Bob) or refund (Alice) the Monero.
    pub async fn swap_wallet(
        &self,
        swap_id: Uuid,
        spend_key: monero::PrivateKey,
        view_key: super::PrivateViewKey,
        tx_lock_id: TxHash,
    ) -> Result<Arc<Wallet>> {
        // Derive wallet address from the keys
        let address = {
            let public_spend_key = monero::PublicKey::from_private_key(&spend_key);
            let public_view_key = monero::PublicKey::from_private_key(&view_key.into());

            monero::Address::standard(self.network, public_spend_key, public_view_key)
        };

        // The wallet's filename is just the swap's uuid as a string
        let filename = swap_id.to_string();
        let wallet_path = self.wallet_dir.join(&filename).display().to_string();

        let blockheight = self
            .main_wallet
            .blockchain_height()
            .await
            .context("Couldn't fetch blockchain height")?;

        let wallet = Wallet::open_or_create_from_keys(
            wallet_path.clone(),
            None,
            self.network,
            address,
            view_key.into(),
            spend_key,
            blockheight,
            false, // We don't sync the swap wallet, just import the transaction
            self.daemon.clone(),
        )
        .await
        .context(format!(
            "Failed to open or create wallet `{}` from the specified keys",
            wallet_path
        ))?;

        if self.regtest {
            wallet.unsafe_prepare_for_regtest().await;
        }

        tracing::debug!(
            %swap_id,
            "Opened temporary Monero wallet, loading lock transaction"
        );

        wallet
            .scan_transaction(tx_lock_id.0.clone())
            .await
            .context("Couldn't import Monero lock transaction")?;

        Ok(Arc::new(wallet))
    }

    /// Get the main wallet (specified when initializing the `Wallets` instance).
    pub async fn main_wallet(&self) -> Arc<Wallet> {
        self.main_wallet.clone()
    }

    /// Get the current blockchain height.
    /// May fail if not connected to a daemon.
    pub async fn blockchain_height(&self) -> Result<BlockHeight> {
        let wallet = self.main_wallet().await;

        Ok(BlockHeight {
            height: wallet.blockchain_height().await.context(
                "Failed to get blockchain height: wallet manager not connected to daemon",
            )?,
        })
    }

    /// Wait until a transfer is detected and confirmed.
    ///
    /// You can pass a listener function that will be called with
    /// the current number of confirmations every time we check the blockchain.
    /// This means that it may be called multiple times with the same number of confirmations.
    pub async fn wait_until_confirmed(
        &self,
        watch_request: WatchRequest,
        listener: Option<impl Fn((u64, u64)) + Send + 'static>,
    ) -> Result<()> {
        let wallet = self.main_wallet().await;

        let address = Address::standard(
            self.network,
            watch_request.public_spend_key,
            watch_request.public_view_key.0,
        );

        wallet
            .wait_until_confirmed(
                watch_request.transfer_proof.tx_hash.0.clone(),
                watch_request.transfer_proof.tx_key,
                &address,
                watch_request.expected_amount,
                watch_request.confirmation_target,
                listener,
            )
            .await?;

        Ok(())
    }

    pub async fn block_height(&self) -> Result<BlockHeight> {
        Ok(BlockHeight {
            height: self
                .main_wallet
                .blockchain_height()
                .await
                .context("Failed to get blockchain height")?,
        })
    }

    /// Get the last 5 recently used wallets
    pub async fn get_recent_wallets(&self) -> Result<Vec<String>> {
        if let Some(db) = &self.wallet_database {
            let recent_wallets = db.get_recent_wallets(5).await?;
            Ok(recent_wallets.into_iter().map(|w| w.wallet_path).collect())
        } else {
            Ok(vec![])
        }
    }

    /// Record that a wallet was accessed
    pub async fn record_wallet_access(&self, wallet_path: &str) -> Result<()> {
        if let Some(db) = &self.wallet_database {
            db.record_wallet_access(wallet_path).await?;
        }
        Ok(())
    }
}

impl TransferRequest {
    pub fn address_and_amount(&self, network: Network) -> (Address, monero::Amount) {
        (
            Address::standard(network, self.public_spend_key, self.public_view_key.0),
            self.amount,
        )
    }
}

/// Pass this to [`Wallet::wait_until_confirmed`] or [`Wallet::wait_until_synced`]
/// to not receive any confirmation callbacks.
pub fn no_listener<T>() -> Option<impl Fn(T) + Send + 'static> {
    Some(|_| {})
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::tracing_ext::capture_logs;
//     use monero_rpc::wallet::CheckTxKey;
//     use std::sync::atomic::{AtomicU32, Ordering};
//     use tokio::sync::Mutex;
//     use tracing::metadata::LevelFilter;

//     async fn wait_for_confirmations<
//         C: monero_rpc::wallet::MoneroWalletRpc<reqwest::Client> + Sync,
//     >(
//         client: Arc<Mutex<Wallet<C>>>,
//         transfer_proof: TransferProof,
//         to_address: Address,
//         expected: Amount,
//         conf_target: u64,
//         check_interval: Interval,
//         wallet_name: String,
//     ) -> Result<(), InsufficientFunds> {
//         wait_for_confirmations_with(
//             client,
//             transfer_proof,
//             to_address,
//             expected,
//             conf_target,
//             check_interval,
//             wallet_name,
//             None,
//         )
//         .await
//     }

//     #[tokio::test]
//     async fn given_exact_confirmations_does_not_fetch_tx_again() {
//         let wallet = Arc::new(Mutex::new(Wallet::from_dummy(
//             DummyClient::new(vec![Ok(CheckTxKey {
//                 confirmations: 10,
//                 received: 100,
//             })]),
//             Network::Testnet,
//         )));

//         let result = wait_for_confirmations(
//             wallet.clone(),
//             TransferProof::new(TxHash("<FOO>".to_owned()), PrivateKey {
//                 scalar: crate::monero::Scalar::random(&mut rand::thread_rng())
//             }),
//             "53H3QthYLckeCXh9u38vohb2gZ4QgEG3FMWHNxccR6MqV1LdDVYwF1FKsRJPj4tTupWLf9JtGPBcn2MVN6c9oR7p5Uf7JdJ".parse().unwrap(),
//             Amount::from_piconero(100),
//             10,
//             tokio::time::interval(Duration::from_millis(10)),
//             "foo-wallet".to_owned(),
//         )
//         .await;

//         assert!(result.is_ok());
//         assert_eq!(
//             wallet
//                 .lock()
//                 .await
//                 .inner
//                 .check_tx_key_invocations
//                 .load(Ordering::SeqCst),
//             1
//         );
//     }

//     #[tokio::test]
//     async fn visual_log_check() {
//         let writer = capture_logs(LevelFilter::INFO);

//         let client = Arc::new(Mutex::new(Wallet::from_dummy(
//             DummyClient::new(vec![
//                 Ok(CheckTxKey {
//                     confirmations: 1,
//                     received: 100,
//                 }),
//                 Ok(CheckTxKey {
//                     confirmations: 1,
//                     received: 100,
//                 }),
//                 Ok(CheckTxKey {
//                     confirmations: 1,
//                     received: 100,
//                 }),
//                 Ok(CheckTxKey {
//                     confirmations: 3,
//                     received: 100,
//                 }),
//                 Ok(CheckTxKey {
//                     confirmations: 5,
//                     received: 100,
//                 }),
//             ]),
//             Network::Testnet,
//         )));

//         wait_for_confirmations(
//             client.clone(),
//             TransferProof::new(TxHash("<FOO>".to_owned()), PrivateKey {
//                 scalar: crate::monero::Scalar::random(&mut rand::thread_rng())
//             }),
//             "53H3QthYLckeCXh9u38vohb2gZ4QgEG3FMWHNxccR6MqV1LdDVYwF1FKsRJPj4tTupWLf9JtGPBcn2MVN6c9oR7p5Uf7JdJ".parse().unwrap(),
//             Amount::from_piconero(100),
//             5,
//             tokio::time::interval(Duration::from_millis(10)),
//             "foo-wallet".to_owned()
//         )
//         .await
//         .unwrap();

//         assert_eq!(
//             writer.captured(),
//             r" INFO swap::monero::wallet: Received new confirmation for Monero lock tx txid=<FOO> seen_confirmations=1 needed_confirmations=5
//  INFO swap::monero::wallet: Received new confirmation for Monero lock tx txid=<FOO> seen_confirmations=3 needed_confirmations=5
//  INFO swap::monero::wallet: Received new confirmation for Monero lock tx txid=<FOO> seen_confirmations=5 needed_confirmations=5
// "
//         );
//     }

//     #[tokio::test]
//     async fn reopens_wallet_in_case_not_available() {
//         let writer = capture_logs(LevelFilter::DEBUG);

//         let client = Arc::new(Mutex::new(Wallet::from_dummy(
//             DummyClient::new(vec![
//                 Ok(CheckTxKey {
//                     confirmations: 1,
//                     received: 100,
//                 }),
//                 Ok(CheckTxKey {
//                     confirmations: 1,
//                     received: 100,
//                 }),
//                 Err((-13, "No wallet file".to_owned())),
//                 Ok(CheckTxKey {
//                     confirmations: 3,
//                     received: 100,
//                 }),
//                 Ok(CheckTxKey {
//                     confirmations: 5,
//                     received: 100,
//                 }),
//             ]),
//             Network::Testnet,
//         )));

//         tokio::time::timeout(Duration::from_secs(30), wait_for_confirmations(
//             client.clone(),
//             TransferProof::new(TxHash("<FOO>".to_owned()), PrivateKey {
//                 scalar: crate::monero::Scalar::random(&mut rand::thread_rng())
//             }),
//             "53H3QthYLckeCXh9u38vohb2gZ4QgEG3FMWHNxccR6MqV1LdDVYwF1FKsRJPj4tTupWLf9JtGPBcn2MVN6c9oR7p5Uf7JdJ".parse().unwrap(),
//             Amount::from_piconero(100),
//             5,
//             tokio::time::interval(Duration::from_millis(10)),
//             "foo-wallet".to_owned(),
//         ))
//         .await
//         .expect("timeout: shouldn't take more than 10 seconds")
//         .unwrap();

//         assert_eq!(
//             writer.captured(),
//             r" INFO swap::monero::wallet: Received new confirmation for Monero lock tx txid=<FOO> seen_confirmations=1 needed_confirmations=5
// DEBUG swap::monero::wallet: No wallet loaded. Opening wallet `foo-wallet` to continue monitoring of Monero transaction <FOO>
//  INFO swap::monero::wallet: Received new confirmation for Monero lock tx txid=<FOO> seen_confirmations=3 needed_confirmations=5
//  INFO swap::monero::wallet: Received new confirmation for Monero lock tx txid=<FOO> seen_confirmations=5 needed_confirmations=5
// "
//         );
//         assert_eq!(
//             client
//                 .lock()
//                 .await
//                 .inner
//                 .open_wallet_invocations
//                 .load(Ordering::SeqCst),
//             1
//         );
//     }

//     type ErrorCode = i64;
//     type ErrorMessage = String;

//     struct DummyClient {
//         check_tx_key_responses: Vec<Result<wallet::CheckTxKey, (ErrorCode, ErrorMessage)>>,

//         check_tx_key_invocations: AtomicU32,
//         open_wallet_invocations: AtomicU32,
//     }

//     impl DummyClient {
//         fn new(
//             check_tx_key_responses: Vec<Result<wallet::CheckTxKey, (ErrorCode, ErrorMessage)>>,
//         ) -> Self {
//             Self {
//                 check_tx_key_responses,
//                 check_tx_key_invocations: Default::default(),
//                 open_wallet_invocations: Default::default(),
//             }
//         }
//     }

//     #[async_trait::async_trait]
//     impl monero_rpc::wallet::MoneroWalletRpc<reqwest::Client> for DummyClient {
//         async fn open_wallet(
//             &self,
//             _: String,
//         ) -> Result<wallet::WalletOpened, monero_rpc::jsonrpc::Error<reqwest::Error>> {
//             self.open_wallet_invocations.fetch_add(1, Ordering::SeqCst);

//             Ok(monero_rpc::wallet::Empty {})
//         }

//         async fn check_tx_key(
//             &self,
//             _: String,
//             _: String,
//             _: String,
//         ) -> Result<wallet::CheckTxKey, monero_rpc::jsonrpc::Error<reqwest::Error>> {
//             let index = self.check_tx_key_invocations.fetch_add(1, Ordering::SeqCst);

//             self.check_tx_key_responses[index as usize]
//                 .clone()
//                 .map_err(|(code, message)| {
//                     monero_rpc::jsonrpc::Error::JsonRpc(monero_rpc::jsonrpc::JsonRpcError {
//                         code,
//                         message,
//                         data: None,
//                     })
//                 })
//         }

//         async fn send_request<P>(
//             &self,
//             _: String,
//         ) -> Result<monero_rpc::jsonrpc::Response<P>, reqwest::Error>
//         where
//             P: serde::de::DeserializeOwned,
//         {
//             todo!()
//         }
//     }
// }
