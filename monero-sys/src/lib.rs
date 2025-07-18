//! A wrapper around the Monero C++ API.
//!
//! This crate provides a safe wrapper around the Monero C++ API.
//! It is used to create and manage Monero wallets, and to interact with the
//! Monero network.
//!
//! The intended use is to create a [`WalletHandle`], which will create a dedicated thread
//! for the wallet being opened.
//!
//! The wallet thread will be running in the background, and the [`WalletHandle`] will
//! internally communicate with the wallet thread.

mod bridge;
pub mod database;

pub use bridge::wallet_listener;
pub use bridge::{TraceListener, WalletEventListener, WalletListenerBox};
pub use database::{Database, RecentWallet};

use std::sync::{Arc, Mutex};
use std::{
    any::Any, cmp::Ordering, collections::HashMap, fmt::Display, future::Future, ops::Deref,
    path::PathBuf, pin::Pin, str::FromStr, time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use backoff::{future::retry_notify, retry_notify as blocking_retry_notify};
use cxx::{let_cxx_string, CxxString, CxxVector, Exception, UniquePtr};
use monero::Amount;
use serde::{Deserialize, Serialize};
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    oneshot,
};

use bridge::ffi::{self, TransactionHistory};
use typeshare::typeshare;
use uuid::Uuid;

/// Approval callback for transactions
/// The callback receives (txid, amount, fee) and returns whether to proceed with the transaction
pub type ApprovalCallback = Arc<
    dyn Fn(String, monero::Amount, monero::Amount) -> Pin<Box<dyn Future<Output = bool> + Send>>
        + Send
        + Sync,
>;

/// A handle which can communicate with the wallet thread via channels.
#[derive(Clone)]
pub struct WalletHandle {
    call_sender: UnboundedSender<Call>,
}

impl WalletHandle {
    fn new(call_sender: UnboundedSender<Call>) -> Self {
        Self { call_sender }
    }
}

impl std::fmt::Display for WalletHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WalletHandle")
    }
}

/// A wrapper around a wallet that can be used to call methods on it.
/// It must live in a single thread due to ffi constraints [1].
///
/// [1] The Monero codebase uses thread local storage and other mechanisms,
/// meaning that it's not safe to access the wallet from any thread other than
/// the one it was created on.
/// This goes for Wallet and WalletManager, meaning that each Wallet must be in its
/// WalletManager's thread (since you need a WalletManager to create a Wallet).
///
pub struct Wallet {
    wallet: FfiWallet,
    manager: WalletManager,
    call_receiver: UnboundedReceiver<Call>,
    pending_transactions: HashMap<Uuid, PendingTransaction>,
}

/// A function call to be executed on the wallet and a channel to send the result back.
struct Call {
    function:
        Box<dyn FnOnce(&mut FfiWallet, &mut HashMap<Uuid, PendingTransaction>) -> AnyBox + Send>,
    sender: oneshot::Sender<AnyBox>,
}

type AnyBox = Box<dyn Any + Send>;

/// A singleton responsible for managing (creating, opening, ...) wallets.
struct WalletManager {
    /// A wrapper around the raw C++ wallet manager pointer.
    inner: RawWalletManager,
}

/// This is our own wrapper around a raw C++ wallet manager pointer.
struct RawWalletManager {
    inner: *mut ffi::WalletManager,
}

/// A single Monero wallet.
pub struct FfiWallet {
    inner: RawWallet,
    listeners: Arc<Mutex<Vec<Box<dyn WalletEventListener>>>>,
}

/// This is our own wrapper around a raw C++ wallet pointer.
/// Do not use for anything except passing it to [`FfiWallet::new`].
struct RawWallet {
    inner: *mut ffi::Wallet,
}

pub const fn no_listener<T>() -> Option<fn(T)> {
    Some(|_| {})
}

/// The progress of synchronization of a wallet with the remote node.
#[derive(Debug, Clone, Copy)]
pub struct SyncProgress {
    /// The current block height of the wallet.
    pub current_block: u64,
    /// The target block height of the wallet.
    pub target_block: u64,
}

/// The status of a transaction.
pub struct TxStatus {
    /// The amount received in the transaction.
    pub received: monero::Amount,
    /// Whether the transaction is in the mempool.
    pub in_pool: bool,
    /// The number of confirmations the transaction has.
    pub confirmations: u64,
}

/// A receipt returned after successfully publishing a transaction.
/// Contains basic information needed for later verification.
pub struct TxReceipt {
    pub txid: String,
    pub tx_key: String,
    /// The blockchain height at the time of publication.
    pub height: u64,
}

/// A remote node to connect to.
#[derive(Debug, Clone, Default)]
pub struct Daemon {
    pub address: String,
    pub ssl: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[typeshare]
pub struct TransactionInfo {
    #[serde(with = "monero_serde")]
    pub fee: monero::Amount,
    #[serde(with = "monero_serde")]
    pub amount: monero::Amount,
    #[typeshare(serialized_as = "number")]
    pub confirmations: u64,
    pub tx_hash: String,
    pub direction: TransactionDirection,
    #[typeshare(serialized_as = "number")]
    pub timestamp: u64,
}

#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransactionDirection {
    In,
    Out,
}

/// A wrapper around a pending transaction.
///
/// Safety: do _not_ implement copy, send, sync, ...
pub struct PendingTransaction(*mut ffi::PendingTransaction);

/// A struct containing transaction history.
pub struct TransactionHistoryHandle(*mut TransactionHistory);

/// A struct containing a single transaction.
pub struct TransactionInfoHandle(*mut ffi::TransactionInfo);

impl WalletHandle {
    /// Open an existing wallet or create a new one, with a random seed.
    pub async fn open_or_create(
        path: String,
        daemon: Daemon,
        network: monero::Network,
        background_sync: bool,
    ) -> anyhow::Result<Self> {
        Self::open_or_create_with_password(path, None, daemon, network, background_sync).await
    }

    /// Common implementation used by all `open_*` helpers.
    /// Spawns a dedicated wallet thread. Gets the WalletManager. Calls the wallet_op closure with the WalletManager.
    /// The wallet_op closure determines which specific WalletManager method is called to create the wallet.
    async fn open_with<F>(path: String, daemon: Daemon, wallet_op: F) -> anyhow::Result<Self>
    where
        F: FnOnce(&mut WalletManager) -> anyhow::Result<FfiWallet> + Send + 'static,
    {
        let (call_sender, call_receiver) = unbounded_channel();

        let wallet_name = path.rsplit('/').next().unwrap_or(&path).to_owned();
        let thread_name = format!("wallet-{wallet_name}");
        let current_dispatcher = tracing::dispatcher::get_default(|d| d.clone());
        let (tx, rx) = oneshot::channel::<Result<()>>();

        std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                let _guard = tracing::dispatcher::set_default(&current_dispatcher);

                // Get the WalletManager
                // If we fail, send the error through the oneshot channel
                let mut manager = match WalletManager::new(daemon.clone(), &wallet_name) {
                    Ok(m) => m,
                    Err(e) => {
                        let _ = tx.send(Err(e.context("failed to create wallet manager")));
                        return;
                    }
                };

                // Open the wallet via caller-supplied closure
                // If we fail, send the error through the oneshot channel
                let wallet = match wallet_op(&mut manager) {
                    Ok(w) => w,
                    Err(e) => {
                        let _ = tx.send(Err(e.context("failed to open or create wallet")));
                        return;
                    }
                };

                // Run the wallet thread
                let mut wrapped = Wallet::new(wallet, manager, call_receiver);
                let _ = tx.send(Ok(()));
                wrapped.run();
            })
            .context("Couldn't start wallet thread")?;

        // Wait for the thread to report success or failure
        rx.await
            .context("Failed to get result from wallet creation thread through oneshot channel")?
            .context("Failed to open or create wallet")?;

        let handle = WalletHandle::new(call_sender);

        handle
            .check_wallet()
            .await
            .context("Failed to open wallet because health check failed")?;

        Ok(handle)
    }

    /// Opens an existing wallet or creates a new one with the specified password.
    /// Uses password-based encryption for the wallet file.
    /// If no password is provided, the wallet will be unencrypted.
    pub async fn open_or_create_with_password(
        path: String,
        password: impl Into<Option<String>>,
        daemon: Daemon,
        network: monero::Network,
        background_sync: bool,
    ) -> anyhow::Result<Self> {
        let password = password.into();

        Self::open_with(path.clone(), daemon.clone(), move |manager| {
            manager.open_or_create_wallet(
                &path,
                password.as_ref(),
                network,
                background_sync,
                daemon.clone(),
            )
        })
        .await
    }

    /// Opens an existing wallet or recovers it from a mnemonic seed.
    /// If the wallet exists at the path, it opens the existing wallet.
    /// Otherwise, it creates a new wallet by recovering from the provided seed.
    pub async fn open_or_create_from_seed(
        path: String,
        mnemonic: String,
        network: monero::Network,
        restore_height: u64,
        background_sync: bool,
        daemon: Daemon,
    ) -> anyhow::Result<Self> {
        Self::open_with(path.clone(), daemon.clone(), move |manager| {
            if manager.wallet_exists(&path) {
                manager.open_or_create_wallet(&path, None, network, background_sync, daemon.clone())
            } else {
                manager.recover_wallet(
                    &path,
                    None,
                    &mnemonic,
                    network,
                    restore_height,
                    background_sync,
                    daemon.clone(),
                )
            }
        })
        .await
    }

    /// Opens an existing wallet or creates one from spend/view keys.
    /// If the wallet exists at the path, it opens the existing wallet.
    /// Otherwise, it creates a new wallet from the provided cryptographic keys.
    #[allow(clippy::too_many_arguments)]
    pub async fn open_or_create_from_keys(
        path: String,
        password: Option<String>,
        network: monero::Network,
        address: monero::Address,
        view_key: monero::PrivateKey,
        spend_key: monero::PrivateKey,
        restore_height: u64,
        background_sync: bool,
        daemon: Daemon,
    ) -> anyhow::Result<Self> {
        Self::open_with(path.clone(), daemon.clone(), move |manager| {
            manager.open_or_create_wallet_from_keys(
                &path,
                password.as_deref(),
                network,
                &address,
                view_key,
                spend_key,
                restore_height,
                background_sync,
                daemon.clone(),
            )
        })
        .await
    }

    /// Execute a function on the wallet thread and return the result.
    /// Necessary because every interaction with the wallet must run on a single thread.
    /// Panics if the channel is closed unexpectedly.
    pub async fn call<F, R>(&self, function: F) -> R
    where
        F: FnOnce(&mut FfiWallet) -> R + Send + 'static,
        R: Sized + Send + 'static,
    {
        // Delegate to call_with_pending_txs but ignore the pending_txs parameter
        self.call_with_pending_txs(move |wallet, _pending_txs| function(wallet))
            .await
    }

    /// Call a function on the wallet with access to pending transactions storage.
    pub async fn call_with_pending_txs<F, R>(&self, function: F) -> R
    where
        F: FnOnce(&mut FfiWallet, &mut HashMap<Uuid, PendingTransaction>) -> R + Send + 'static,
        R: Sized + Send + 'static,
    {
        // Create a oneshot channel for the result
        let (sender, receiver) = oneshot::channel();

        // Send the function call to the wallet thread (wrapped in a Box)
        self.call_sender
            .send(Call {
                function: Box::new(move |wallet, pending_txs| {
                    Box::new(function(wallet, pending_txs)) as Box<dyn Any + Send>
                }),
                sender,
            })
            .inspect_err(|e| tracing::error!(error=%e, "failed to send call"))
            .expect("channel to be open");

        // Wait for the result and cast back to the expected type
        *receiver
            .await
            .expect("channel to be open")
            .downcast::<R>() // We know that F returns R
            .expect("return type to be consistent")
    }

    /// Get the file system path to the wallet.
    pub async fn path(&self) -> String {
        self.call(move |wallet| wallet.path()).await
    }

    /// Get the main address of the wallet.
    /// The main address is the first address of the first account.
    pub async fn main_address(&self) -> monero::Address {
        self.call(move |wallet| wallet.main_address()).await
    }

    /// Get the current height of the blockchain.
    /// May involve an RPC call to the daemon.
    /// Returns `None` if the wallet is not connected to a daemon.
    ///
    /// Retries at most 5 times with a 500ms delay between attempts.
    pub async fn blockchain_height(&self) -> anyhow::Result<u64> {
        const MAX_RETRIES: u64 = 5;

        for _ in 0..MAX_RETRIES {
            if let Some(height) = self
                .call(move |wallet| wallet.daemon_blockchain_height())
                .await
            {
                return Ok(height);
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        self.check_wallet().await?;

        bail!("Failed to get blockchain height after 5 attempts");
    }

    /// Transfer funds to an address without approval.
    pub async fn transfer(
        &self,
        address: &monero::Address,
        amount: monero::Amount,
    ) -> anyhow::Result<TxReceipt> {
        let address = *address;

        retry_notify(backoff(None, None), || async {
            self.call(move |wallet| wallet.transfer(&address, amount))
                .await
                .map_err(backoff::Error::transient)
        }, |error, duration: Duration| {
            tracing::error!(error=%error, "Failed to transfer funds, retrying in {} secs", duration.as_secs());
        })
        .await
        .map_err(|e| anyhow!("Failed to transfer funds after multiple attempts: {e}"))
    }

    /// Sweep all funds to an address.
    pub async fn sweep(&self, address: &monero::Address) -> anyhow::Result<Vec<TxReceipt>> {
        let address = *address;

        retry_notify(backoff(None, None), || async {
            self.call(move |wallet| wallet.sweep(&address))
                .await
                .map_err(backoff::Error::transient)
        }, |error, duration: Duration| {
            tracing::error!(error=%error, "Failed to sweep funds, retrying in {} secs", duration.as_secs());
        })
        .await
        .map_err(|e| anyhow!("Failed to sweep funds after multiple attempts: {e}"))
    }

    /// Get the seed of the wallet.
    pub async fn seed(&self) -> anyhow::Result<String> {
        self.call(move |wallet| wallet.seed()).await
    }

    /// Get the creation height of the wallet.
    pub async fn creation_height(&self) -> u64 {
        self.call(move |wallet| wallet.creation_height()).await
    }

    /// Sweep all funds to a set of addresses.
    /// If the address is `None`, the address will be set to the primary address of the
    /// wallet
    pub async fn sweep_multi(
        &self,
        addresses: &[monero::Address],
        percentages: &[f64],
    ) -> anyhow::Result<Vec<TxReceipt>> {
        tracing::debug!(addresses=?addresses, percentages=?percentages, "Sweeping multi");

        let percentages = percentages.to_vec();
        let addresses = addresses.to_vec();

        self.call(move |wallet| wallet.sweep_multi(&addresses, &percentages))
            .await
    }

    /// Get the transaction history and convert it to a list of serializable transaction infos.
    /// This is needed because TransactionHistory and TransactionInfo are not Send.
    pub async fn history(&self) -> Vec<TransactionInfo> {
        self.call(move |wallet| wallet.history()).await
    }

    /// Get the unlocked balance of the wallet.
    pub async fn unlocked_balance(&self) -> monero::Amount {
        self.call(move |wallet| wallet.unlocked_balance()).await
    }

    /// Get the total balance of the wallet.
    pub async fn total_balance(&self) -> monero::Amount {
        self.call(move |wallet| wallet.total_balance()).await
    }

    /// Check if the wallet is synchronized.
    async fn synchronized(&self) -> bool {
        self.call(move |wallet| wallet.synchronized()).await
    }

    /// Get the restore height of the wallet.
    pub async fn set_restore_height(&self, height: u64) -> anyhow::Result<()> {
        self.call(move |wallet| wallet.set_restore_height(height))
            .await
            .context("Failed to set restore height: FFI call failed with exception")
            .expect("Shouldn't panic");

        Ok(())
    }

    /// Get the restore height of the wallet.
    pub async fn get_restore_height(&self) -> anyhow::Result<u64> {
        self.call(move |wallet| wallet.get_restore_height())
            .await
            .map_err(|e| {
                anyhow!("Failed to get restore height: FFI call failed with exception: {e}")
            })
    }

    pub async fn get_blockchain_height_by_date(
        &self,
        year: u16,
        month: u8,
        day: u8,
    ) -> Result<u64> {
        self.call(move |wallet| wallet.get_blockchain_height_by_date(year, month, day))
            .await
            .map_err(|e| {
                anyhow!(
                    "Failed to get blockchain height by date: FFI call failed with exception: {e}"
                )
            })
    }

    /// Rescan the blockchain asynchronously.
    pub async fn rescan_blockchain_async(&self) {
        self.call(move |wallet| wallet.rescan_blockchain_async())
            .await
    }

    /// Start the refresh.
    pub async fn start_refresh(&self) {
        self.call(move |wallet| wallet.start_refresh()).await
    }

    /// Pause the background refresh.
    pub async fn pause_refresh(&self) {
        self.call(move |wallet| wallet.pause_refresh()).await
    }

    /// Start the background refresh thread.
    pub async fn start_refresh_thread(&self) {
        self.call(move |wallet| wallet.start_refresh_thread()).await
    }

    /// Stop the background refresh once (doesn't stop background refresh thread).
    pub async fn stop(&self) {
        self.call(move |wallet| wallet.stop()).await
    }

    /// Store the wallet state.
    /// If `path` is `None`, the wallet will be stored in the location it was opened from.
    pub async fn store(&self, path: Option<&str>) {
        let path = path.unwrap_or("").to_string();
        self.call(move |wallet| wallet.store(&path)).await;
    }

    /// Get the sync progress of the wallet.
    pub async fn sync_progress(&self) -> SyncProgress {
        self.call(move |wallet| wallet.sync_progress()).await
    }

    /// Check if the wallet is connected to a daemon.
    pub async fn connected(&self) -> bool {
        self.call(move |wallet| wallet.connected()).await
    }

    /// Check that the wallet is created and ready to use.
    /// Call this after creating a wallet to make sure the wallet thread responds correctly.
    async fn check_wallet(&self) -> anyhow::Result<()> {
        let (sender, receiver) = oneshot::channel();

        self.call_sender
            .send(Call {
                function: Box::new(move |wallet, _pending_txs| {
                    Box::new(wallet.check_error()) as Box<dyn Any + Send>
                }),
                sender,
            })
            .map_err(|_| anyhow::anyhow!("failed to send check_wallet call"))?;

        receiver
            .await
            .context("wallet channel closed unexpectedly")?;

        Ok(())
    }

    /// Allow the wallet to connect to a daemon with a different version.
    /// Also trusts the daemon.
    /// Only used for regtests.
    /// Also forces a full sync, which is only feasible in regtests.
    #[doc(hidden)]
    pub async fn unsafe_prepare_for_regtest(&self) {
        self.call(move |wallet| {
            wallet.force_full_sync();
            wallet.allow_mismatched_daemon_version();
            wallet.set_trusted_daemon(true);
        })
        .await
    }

    /// Wait until the wallet is synchronized.
    ///
    /// Polls the wallet's sync status every 500ms until the wallet is synchronized.
    ///
    /// If a listener is provided, it will be called with the sync progress.
    pub async fn wait_until_synced(
        &self,
        listener: Option<impl Fn(SyncProgress) + Send + 'static>,
    ) -> anyhow::Result<()> {
        // We wait for ms before polling the wallet's sync status again.
        // This is ok because this doesn't involve any blocking calls.
        const POLL_INTERVAL_MILLIS: u64 = 500;

        // Initiate the sync (make sure to drop the lock right after)
        {
            self.call(move |wallet| {
                wallet.start_refresh_thread();
                wallet.force_background_refresh();
            })
            .await;
            tracing::debug!("Wallet refresh initiated");
        }

        // Wait until the wallet is connected to the daemon.
        loop {
            let connected = self.call(move |wallet| wallet.connected()).await;

            if connected {
                break;
            }

            tracing::trace!(
                "Wallet not connected to daemon, sleeping for {}ms",
                POLL_INTERVAL_MILLIS
            );

            tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
        }

        // Keep track of the sync progress to avoid calling
        // the listener twice with the same progress
        let mut current_progress = self.sync_progress().await;

        // Continue polling until the sync is complete
        loop {
            // Get the current sync status
            let (synced, sync_progress) =
                { (self.synchronized().await, self.sync_progress().await) };

            // Notify the listener (if it exists)
            if sync_progress > current_progress {
                if let Some(listener) = &listener {
                    listener(sync_progress);
                }
            }

            // Update the current progress
            current_progress = sync_progress;

            // If the wallet is synced, break out of the loop.
            if synced {
                break;
            }

            tracing::trace!(
                %sync_progress,
                "Wallet sync not complete, sleeping for {}ms",
                POLL_INTERVAL_MILLIS
            );

            // Otherwise, sleep for a bit and try again.
            tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
        }

        tracing::info!("Wallet synced");

        Ok(())
    }

    /// Check the status of a transaction.
    async fn check_tx_status(
        &self,
        txid: String,
        tx_key: monero::PrivateKey,
        destination_address: &monero::Address,
    ) -> anyhow::Result<TxStatus> {
        let destination_address = *destination_address;
        self.call(move |wallet| wallet.check_tx_status(&txid, tx_key, &destination_address))
            .await
    }

    /// Scan a transaction for the wallet.
    /// This makes a transaction visible to the wallet without requiring a full sync.
    pub async fn scan_transaction(&self, txid: String) -> anyhow::Result<()> {
        self.call(move |wallet| wallet.scan_transaction(txid)).await
    }

    /// Wait until a transaction is confirmed.
    pub async fn wait_until_confirmed(
        &self,
        txid: String,
        tx_key: monero::PrivateKey,
        destination_address: &monero::Address,
        expected_amount: monero::Amount,
        confirmations: u64,
        listener: Option<impl Fn((u64, u64)) + Send + 'static>,
    ) -> anyhow::Result<()> {
        tracing::info!(%txid, %destination_address, amount=%expected_amount, %confirmations, "Waiting until transaction is confirmed");

        const DEFAULT_CHECK_INTERVAL_SECS: u64 = 15;

        let mut poll_interval = tokio::time::interval(tokio::time::Duration::from_secs(
            DEFAULT_CHECK_INTERVAL_SECS,
        ));

        loop {
            poll_interval.tick().await;

            let tx_status = match self
                .check_tx_status(txid.clone(), tx_key, destination_address)
                .await
            {
                Ok(tx_status) => tx_status,
                Err(e) => {
                    tracing::error!(
                        "Failed to check tx status: {}, rechecking in {}s",
                        e,
                        DEFAULT_CHECK_INTERVAL_SECS
                    );
                    continue;
                }
            };

            // Make sure the amount is correct
            if tx_status.received != expected_amount {
                tracing::error!(
                    "Transaction received amount mismatch: expected {}, got {}",
                    expected_amount,
                    tx_status.received
                );
                return Err(anyhow::anyhow!(
                    "Transaction received amount mismatch: expected {}, got {}",
                    expected_amount,
                    tx_status.received
                ));
            }

            // If the listener exists, notify it of the result
            if let Some(listener) = &listener {
                listener((tx_status.confirmations, confirmations));
            }

            // Stop when we have the required number of confirmations
            if tx_status.confirmations >= confirmations {
                break;
            }

            tracing::trace!("Transaction not confirmed yet, polling again later");
        }

        // Signal success
        Ok(())
    }

    /// Creates pending transaction, gets approval, then publishes or disposes based on approval.
    /// Return `None` if the transaction is not published, `Some(receipt)` if it is published.
    /// If the amount is `None`, the transaction will be a sweep (whole balance)
    ///
    /// Returns (TxReceipt, amount, fee) if the transaction is published, `None` otherwise.
    pub async fn transfer_with_approval(
        &self,
        address: &monero::Address,
        amount: Option<monero::Amount>,
        approval_callback: ApprovalCallback,
    ) -> anyhow::Result<Option<(TxReceipt, monero::Amount, monero::Amount)>> {
        let address = *address;

        // Construct and sign the transaction. Do not publish the transaction yet
        // Store the pending transaction in the wallet thread inside the [`pending_txs`] map
        let (uuid, txid, amount, fee) = self
            .call_with_pending_txs(move |wallet, pending_txs| {
                let pending_tx = match amount {
                    Some(amount) => wallet.create_pending_transaction(&address, amount)?,
                    None => wallet.create_pending_sweep_transaction(&address)?,
                };

                // Get txid
                let txid = ffi::pendingTransactionTxId(&pending_tx)
                    .context("Failed to get txid from pending transaction")?
                    .to_string();

                // Get the amount
                let amount = ffi::pendingTransactionAmount(&pending_tx)
                    .context("Failed to get amount from pending transaction")?;
                let amount = monero::Amount::from_pico(amount);

                // Get the fee
                let fee = ffi::pendingTransactionFee(&pending_tx)
                    .context("Failed to get fee from pending transaction")?;
                let fee = monero::Amount::from_pico(fee);

                // Generate UUID and store it in the pending transactions map
                let uuid = Uuid::new_v4();
                pending_txs.insert(uuid, pending_tx);

                Ok::<(Uuid, String, monero::Amount, monero::Amount), anyhow::Error>((
                    uuid, txid, amount, fee,
                ))
            })
            .await?;

        // Get approval asynchronously (no wallet thread blocking)
        let approved = approval_callback(txid, amount, fee).await;

        if approved {
            // Publish the transaction
            let receipt = self
                .call_with_pending_txs(move |wallet, pending_txs| {
                    let mut pending_tx = pending_txs.remove(&uuid).ok_or_else(|| {
                        anyhow!("Pending transaction not found for UUID: {}", uuid)
                    })?;

                    let result = wallet.publish_pending_transaction(&mut pending_tx);
                    wallet.dispose_pending_transaction(pending_tx);
                    result
                })
                .await?;

            Ok(Some((receipt, amount, fee)))
        } else {
            // Dispose the pending transaction without publishing
            self.call_with_pending_txs(move |wallet, pending_txs| {
                let pending_tx = pending_txs
                    .remove(&uuid)
                    .ok_or_else(|| anyhow!("Pending transaction not found for UUID: {}", uuid))?;

                wallet.dispose_pending_transaction(pending_tx);
                Ok::<(), anyhow::Error>(())
            })
            .await?;

            Ok(None)
        }
    }

    /// Verify the password for a wallet at the given path.
    /// This function spawns a thread to perform the verification and returns the result via a oneshot channel.
    /// Returns `Ok(true)` if the password is correct, `Ok(false)` if incorrect.
    pub fn verify_wallet_password(path: String, password: String) -> anyhow::Result<bool> {
        use std::sync::mpsc;

        // Get the keys file path from the wallet path (simply append .keys to the path)
        let keys_file_path = format!("{}.keys", path);

        let (sender, receiver) = mpsc::channel();

        std::thread::spawn(move || {
            let wallet_name = path
                .split('/')
                .last()
                .map(ToString::to_string)
                .unwrap_or_else(|| "wallet".to_string());

            let result = (|| -> anyhow::Result<bool> {
                let mut manager = WalletManager::new(
                    // Dummy daemon address
                    Daemon {
                        address: "localhost:18081".to_string(),
                        ssl: false,
                    },
                    &wallet_name,
                )?;

                manager.verify_wallet_password(&keys_file_path, &password)
            })();

            let _ = sender.send(result);
        });

        receiver
            .recv()
            .context("Failed to receive password verification result from thread")?
    }

    /// Sign a message with the wallet's private key.
    ///
    /// # Arguments
    /// * `message` - The message to sign (arbitrary byte data)
    /// * `address` - The address to use for signing (uses main address if None)
    /// * `sign_with_view_key` - Whether to sign with view key instead of spend key (default: false)
    ///
    /// # Returns
    /// A proof type prefix + base58 encoded signature
    pub async fn sign_message(
        &self,
        message: &str,
        address: Option<&str>,
        sign_with_view_key: bool,
    ) -> anyhow::Result<String> {
        let message = message.to_string();
        let address = address.map(|s| s.to_string());

        self.call(move |wallet| {
            wallet.sign_message(&message, address.as_deref(), sign_with_view_key)
        })
        .await
    }
}

impl Wallet {
    fn new(
        wallet: FfiWallet,
        manager: WalletManager,
        call_receiver: UnboundedReceiver<Call>,
    ) -> Self {
        Self {
            wallet,
            manager,
            call_receiver,
            pending_transactions: HashMap::new(),
        }
    }

    fn run(&mut self) {
        while let Some(call) = self.call_receiver.blocking_recv() {
            let result = (call.function)(&mut self.wallet, &mut self.pending_transactions);

            // TODO: Do not panic here
            call.sender
                .send(result)
                .expect("failed to send result back to caller");
        }

        tracing::info!(
            wallet=%self.wallet.path(),
            "Wallet handle dropped, closing wallet and exiting thread",
        );

        let result = self.manager.close_wallet(&mut self.wallet);

        if let Err(e) = result {
            tracing::error!("Failed to close wallet: {}", e);
            // If we fail to close the wallet, we can't do anything about it.
            // This results in it being leaked.
        }
        // TODO: dispose of the manager

        // Uninstall the log callback.
        // We need to do this because easylogging++ may send logs after we end this thread, leading
        // to a tracing panic.

        bridge::log::uninstall_log_callback()
            .context("Failed to uninstall log callback: FFI call failed with exception")
            .expect("Shouldn't panic");
    }
}

impl WalletManager {
    /// For now we don't support custom difficulty
    const DEFAULT_KDF_ROUNDS: u64 = 1;

    /// Get the wallet manager instance.
    /// You can optionally pass a daemon with which the wallet manager and
    /// all wallets opened by the manager will connect.
    pub fn new(daemon: Daemon, span_name: &str) -> anyhow::Result<Self> {
        // Install the log callback to route c++ logs to tracing.
        let_cxx_string!(span_name = span_name);
        bridge::log::install_log_callback(&span_name)
            .context("Failed to install log callback: FFI call failed with exception")?;

        let manager = ffi::getWalletManager()
            .context("Couldn't get wallet manager: FFi call failed with exception")?;

        let mut manager = Self {
            inner: RawWalletManager::new(manager),
        };

        manager.set_daemon_address(&daemon.address);

        Ok(manager)
    }

    /// Create a new wallet, or open if it already exists.
    pub fn open_or_create_wallet(
        &mut self,
        path: &str,
        password: Option<&String>,
        network: monero::Network,
        background_sync: bool,
        daemon: Daemon,
    ) -> anyhow::Result<FfiWallet> {
        tracing::debug!(%path, "Opening or creating wallet");

        // If we haven't loaded the wallet, but it already exists, open it.
        if self.wallet_exists(path) {
            tracing::debug!(wallet=%path, "Wallet already exists, opening it");

            return self
                .open_wallet(
                    path,
                    password,
                    network,
                    background_sync,
                    daemon,
                    Box::new(TraceListener::new(path.to_string())),
                )
                .context(format!("Failed to open wallet `{}`", &path));
        }

        tracing::debug!(%path, "Wallet doesn't exist, creating it");

        // Ensure the parent directory exists so the Monero library can write the wallet files
        if let Some(dir) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(dir).with_context(|| {
                format!("failed to create wallet directory `{}`", dir.display())
            })?;
        }

        // Otherwise, create (and open) a new wallet.
        let kdf_rounds = Self::DEFAULT_KDF_ROUNDS;
        let_cxx_string!(path = path);
        let_cxx_string!(password = password.map_or("", |s| s.as_str()));
        let_cxx_string!(language = "English");
        let network_type = network.into();

        let wallet_pointer = self
            .inner
            .pinned()
            .createWallet(&path, &password, &language, network_type, kdf_rounds)
            .context("Failed to create wallet: FFI call failed with exception")?;

        if wallet_pointer.is_null() {
            anyhow::bail!("Failed to create wallet, got null pointer");
        }

        let raw_wallet = RawWallet::new(wallet_pointer);
        let wallet = FfiWallet::new(raw_wallet, background_sync, daemon)
            .context(format!("Failed to initialize wallet `{}`", &path))?;

        Ok(wallet)
    }

    /// Create a new wallet from keys or open if it already exists.
    #[allow(clippy::too_many_arguments)]
    pub fn open_or_create_wallet_from_keys(
        &mut self,
        path: &str,
        password: Option<&str>,
        network: monero::Network,
        address: &monero::Address,
        view_key: monero::PrivateKey,
        spend_key: monero::PrivateKey,
        restore_height: u64,
        background_sync: bool,
        daemon: Daemon,
    ) -> Result<FfiWallet> {
        tracing::debug!(%path, "Creating wallet from keys");

        if self.wallet_exists(path) {
            tracing::info!(wallet=%path, "Wallet already exists, opening it");

            return self
                .open_wallet(
                    path,
                    password.map(|s| s.to_string()).as_ref(),
                    network,
                    background_sync,
                    daemon.clone(),
                    Box::new(TraceListener::new(path.to_string())),
                )
                .context(format!("Failed to open wallet `{}`", &path));
        }

        let pathbuf = PathBuf::from(path);
        if let Some(directory) = pathbuf.parent() {
            tracing::debug!(
                "Making sure to create wallet directory `{}`",
                directory.display()
            );
            std::fs::create_dir_all(directory).context(format!(
                "failed to create wallet directory `{}`",
                directory.display()
            ))?;
        }

        let path = pathbuf.display().to_string();

        tracing::debug!(restore_height, %address, "Creating wallet from keys");

        let_cxx_string!(path = path);
        let_cxx_string!(password = password.unwrap_or(""));
        let_cxx_string!(language = "English");
        let network_type = network.into();
        let_cxx_string!(address = address.to_string());
        let_cxx_string!(view_key = view_key.to_string());
        let_cxx_string!(spend_key = spend_key.to_string());
        let kdf_rounds = Self::DEFAULT_KDF_ROUNDS;

        let wallet_pointer = self
            .inner
            .pinned()
            .createWalletFromKeys(
                &path,
                &password,
                &language,
                network_type,
                restore_height,
                &address,
                &view_key,
                &spend_key,
                kdf_rounds,
            )
            .context("Failed to create wallet from keys: FFI call failed with exception")?;

        if wallet_pointer.is_null() {
            anyhow::bail!("Failed to create wallet from keys, got null pointer");
        }

        let raw_wallet = RawWallet::new(wallet_pointer);
        tracing::debug!(path=%path, "Created wallet from keys, initializing");
        let wallet = FfiWallet::new(raw_wallet, background_sync, daemon)
            .context(format!("Failed to initialize wallet `{}` from keys", &path))?;

        Ok(wallet)
    }

    /// Recover a wallet from a mnemonic seed (electrum seed).
    #[allow(clippy::too_many_arguments)]
    pub fn recover_wallet(
        &mut self,
        path: &str,
        password: Option<&str>,
        mnemonic: &str,
        network: monero::Network,
        restore_height: u64,
        background_sync: bool,
        daemon: Daemon,
    ) -> anyhow::Result<FfiWallet> {
        tracing::debug!(%path, "Recovering wallet from seed");

        let_cxx_string!(path = path);
        let_cxx_string!(password = password.unwrap_or(""));
        let_cxx_string!(mnemonic = mnemonic);
        let_cxx_string!(seed_offset = "");

        let network_type = network.into();
        let wallet_pointer = self
            .inner
            .pinned()
            .recoveryWallet(
                &path,
                &password,
                &mnemonic,
                network_type,
                restore_height,
                Self::DEFAULT_KDF_ROUNDS,
                &seed_offset,
            )
            .context("Failed to recover wallet from seed: FFI call failed with exception")?;

        let raw_wallet = RawWallet::new(wallet_pointer);
        let wallet = FfiWallet::new(raw_wallet, background_sync, daemon)
            .context(format!("Failed to initialize wallet `{}` from seed", &path))?;

        Ok(wallet)
    }

    /// Close a wallet, storing the wallet state.
    fn close_wallet(&mut self, wallet: &mut FfiWallet) -> anyhow::Result<()> {
        tracing::info!(wallet=%wallet.filename(), "Closing wallet");

        // Safety: we know we have a valid, unique pointer to the wallet and are on the same thread it
        // was created on.
        let success = unsafe { self.inner.pinned().closeWallet(wallet.inner.inner, true) }
            .context("Failed to close wallet: Ffi call failed with exception")?;

        if !success {
            anyhow::bail!("Failed to close wallet");
        }

        Ok(())
    }

    /// Open a wallet. Only used internally. Use [`WalletManager::open_or_create_wallet`] instead.
    fn open_wallet(
        &mut self,
        path: &str,
        password: Option<&String>,
        network_type: monero::Network,
        background_sync: bool,
        daemon: Daemon,
        listener: Box<dyn WalletEventListener>,
    ) -> anyhow::Result<FfiWallet> {
        tracing::debug!(%path, "Opening wallet");

        let_cxx_string!(path = path);
        let_cxx_string!(password = password.map_or("", |s| s.as_str()));
        let network_type = network_type.into();
        let kdf_rounds = Self::DEFAULT_KDF_ROUNDS;

        // Safety: we pass a null pointer which is safe because we don't use it.
        let wallet_pointer = unsafe {
            self.inner.pinned().openWallet(
                &path,
                &password,
                network_type,
                kdf_rounds,
                std::ptr::null_mut(),
            )
        }
        .context("Failed to open wallet: FFI call failed with exception")?;

        if wallet_pointer.is_null() {
            anyhow::bail!("Failed to open wallet: got null pointer")
        }

        let raw_wallet = RawWallet::new(wallet_pointer);

        let wallet = FfiWallet::new(raw_wallet, background_sync, daemon)
            .context("Failed to initialize re-opened wallet")?;

        wallet.add_listener(listener);

        Ok(wallet)
    }

    /// Set the address of the remote node ("daemon").
    fn set_daemon_address(&mut self, address: &str) {
        tracing::debug!(%address, "Updating wallet manager's remote node");

        let_cxx_string!(address = address);

        self.inner
            .pinned()
            .setDaemonAddress(&address)
            .context("Failed to set daemon address: FFI call failed with exception")
            .expect("Shouldn't panic");
    }

    /// Check if a wallet exists at the given path.
    pub fn wallet_exists(&mut self, path: &str) -> bool {
        tracing::debug!(%path, "Checking if wallet exists");

        let_cxx_string!(path = path);
        self.inner
            .pinned()
            .walletExists(&path)
            .context("Failed to check if wallet exists: FFI call failed with exception")
            .expect("Wallet check should never fail")
    }

    /// Verify the password for a wallet at the given path.
    /// Returns `Ok(true)` if the password is correct, `Ok(false)` if incorrect.
    pub fn verify_wallet_password(&mut self, path: &str, password: &str) -> anyhow::Result<bool> {
        let_cxx_string!(path = path);
        let_cxx_string!(password = password);

        // Safety: we know we have a valid, unique pointer to the wallet manager and are on its original thread.
        self.inner
            .deref()
            .verifyWalletPassword(&path, &password, false, Self::DEFAULT_KDF_ROUNDS)
            .context("Failed to verify wallet password: FFI call failed with exception")
    }
}

impl RawWalletManager {
    fn new(inner: *mut ffi::WalletManager) -> Self {
        Self { inner }
    }

    /// Get a pinned reference to the inner (c++) wallet manager.
    /// This is a convenience function necessary because
    /// the ffi interface mostly takes a Pin<&mut T> but
    /// we haven't figured out how to hold that in the struct.
    pub fn pinned(&mut self) -> Pin<&mut ffi::WalletManager> {
        // Safety: we know it's a valid pointer on the original thread, we check for null pointers.
        unsafe {
            Pin::new_unchecked(
                self.inner
                    .as_mut()
                    .expect("wallet manager pointer not to be null"),
            )
        }
    }
}

impl Deref for RawWalletManager {
    type Target = ffi::WalletManager;

    fn deref(&self) -> &Self::Target {
        // Safety: we know it's a valid pointer on the original thread, we check for null pointers.
        unsafe { self.inner.as_ref().expect("wallet manager not to be null") }
    }
}

impl WalletEventListener for Arc<Mutex<Vec<Box<dyn WalletEventListener>>>> {
    fn on_money_spent(&self, txid: &str, amount: u64) {
        for listener in self.lock().unwrap().iter() {
            listener.on_money_spent(txid, amount);
        }
    }

    fn on_money_received(&self, txid: &str, amount: u64) {
        for listener in self.lock().unwrap().iter() {
            listener.on_money_received(txid, amount);
        }
    }

    fn on_unconfirmed_money_received(&self, txid: &str, amount: u64) {
        for listener in self.lock().unwrap().iter() {
            listener.on_unconfirmed_money_received(txid, amount);
        }
    }

    fn on_new_block(&self, height: u64) {
        for listener in self.lock().unwrap().iter() {
            listener.on_new_block(height);
        }
    }

    fn on_updated(&self) {
        for listener in self.lock().unwrap().iter() {
            listener.on_updated();
        }
    }

    fn on_refreshed(&self) {
        for listener in self.lock().unwrap().iter() {
            listener.on_refreshed();
        }
    }

    fn on_reorg(&self, height: u64, blocks_detached: u64, transfers_detached: usize) {
        for listener in self.lock().unwrap().iter() {
            listener.on_reorg(height, blocks_detached, transfers_detached);
        }
    }

    fn on_pool_tx_removed(&self, txid: &str) {
        for listener in self.lock().unwrap().iter() {
            listener.on_pool_tx_removed(txid);
        }
    }
}

impl FfiWallet {
    const MAIN_ACCOUNT_INDEX: u32 = 0;

    /// Create and initialize new wallet from a raw C++ wallet pointer.
    fn new(inner: RawWallet, background_sync: bool, daemon: Daemon) -> anyhow::Result<Self> {
        if inner.inner.is_null() {
            anyhow::bail!("Failed to create wallet: got null pointer");
        }

        let mut wallet = Self {
            inner,
            listeners: Arc::new(Mutex::new(vec![])),
        };

        wallet
            .check_error()
            .context("Something went wrong while creating the wallet (not null pointer, though)")?;

        tracing::debug!(address=%wallet.main_address(), "Initializing wallet");

        blocking_retry_notify(
            backoff(None, None),
            || {
                wallet
                    .init(&daemon.address, daemon.ssl)
                    .context("Failed to initialize wallet")
                    .map_err(backoff::Error::transient)
            },
            |e, duration: Duration| tracing::error!(error=%e, "Failed to initialize wallet, retrying in {} secs", duration.as_secs()),
        )
        .map_err(|e| anyhow!("Failed to initialize wallet: {e}"))?;
        tracing::debug!("Initialized wallet, setting daemon address");

        wallet.set_daemon_address(&daemon.address)?;

        if background_sync {
            tracing::debug!("Background sync enabled, starting refresh thread");

            wallet.start_refresh_thread();
            wallet.force_background_refresh();
        }

        wallet.set_single_listener(Box::new(wallet.listeners.clone()));

        // Check for errors on general principles
        wallet.check_error()?;

        Ok(wallet)
    }

    /// Get the path to the wallet file.
    pub fn path(&self) -> String {
        ffi::walletPath(&self.inner)
            .context("Failed to get wallet path: FFI call failed with exception")
            .expect("Wallet path should never fail")
            .to_string()
    }

    /// Get the filename of the wallet.
    pub fn filename(&self) -> String {
        ffi::walletFilename(&self.inner)
            .context("Failed to get wallet filename: FFI call failed with exception")
            .expect("Wallet filename should never fail")
            .to_string()
    }

    /// Get the address for the given account and address index.
    /// address(0, 0) is the main address.
    /// We don't use anything besides the main address so this is a private method (for now).
    fn address(&self, account_index: u32, address_index: u32) -> monero::Address {
        let address = ffi::address(&self.inner, account_index, address_index)
            .context("Failed to get wallet address: FFI call failed with exception")
            .expect("Wallet address should never fail");

        monero::Address::from_str(&address.to_string()).expect("wallet's own address to be valid")
    }

    pub fn set_daemon_address(&mut self, address: &str) -> anyhow::Result<()> {
        tracing::debug!(%address, "Setting daemon address");

        let_cxx_string!(address = address);
        let raw_wallet = &mut self.inner;

        let success = ffi::setWalletDaemon(raw_wallet.pinned(), &address)
            .context("Failed to set daemon address: FFI call failed with exception")?;

        if !success {
            self.check_error().context("Failed to set daemon address")?;
            anyhow::bail!("Failed to set daemon address");
        }

        Ok(())
    }

    /// Get the main address of the walllet (account 0, address 0).
    pub fn main_address(&self) -> monero::Address {
        self.address(Self::MAIN_ACCOUNT_INDEX, 0)
    }

    /// Initialize the wallet and download initial values from the remote node.
    /// Does not actuallyt sync the wallet, use any of the refresh methods to do that.
    fn init(&mut self, daemon_address: &str, ssl: bool) -> anyhow::Result<()> {
        tracing::debug!(%daemon_address, %ssl, "Initializing wallet");

        let_cxx_string!(daemon_address = daemon_address);
        let_cxx_string!(daemon_username = "");
        let_cxx_string!(daemon_password = "");
        let_cxx_string!(proxy_address = "");

        let raw_wallet = &mut self.inner;

        let success = raw_wallet
            .pinned()
            .init(
                &daemon_address,
                0,
                &daemon_username,
                &daemon_password,
                ssl,
                false,
                &proxy_address,
            )
            .context("Couldn't `init` wallet: FFI call failed with exception")?;

        if !success {
            self.check_error().context("Failed to initialize wallet")?;
            anyhow::bail!("Failed to initialize wallet, error string empty");
        }

        Ok(())
    }

    /// Set a listener to the wallet.
    pub fn set_single_listener(&mut self, listener: Box<dyn WalletEventListener>) {
        let cpp_listener = bridge::wallet_listener::create_rust_listener_adapter(
            WalletListenerBox::new_boxed(listener),
        ) as *mut ffi::WalletListener;

        unsafe {
            // Safety: we know that create_rust_listener_adapter returns a valid pointer to a c++ object extending Monero::WalletListener.
            self.inner
                .pinned()
                .setListener(cpp_listener)
                .expect("Shouldn't panic");
        }
    }

    /// Add a listener to the wallet.
    pub fn add_listener(&self, listener: Box<dyn WalletEventListener>) {
        self.listeners.lock().unwrap().push(listener);
    }

    /// Get the sync progress of the wallet as a percentage.
    ///
    /// Returns a zeroed sync progress if the daemon is not connected.
    pub fn sync_progress(&self) -> SyncProgress {
        let current_block = self
            .inner
            .blockChainHeight()
            .context("Failed to get current block height: FFI call failed with exception")
            .expect("Shouldn't panic");
        let target_block = self.daemon_blockchain_height().unwrap_or(0);

        if target_block == 0 {
            return SyncProgress::zero();
        }

        let progress = SyncProgress::new(current_block, target_block);

        tracing::trace!(%progress, "Sync progress");

        progress
    }

    fn connected(&self) -> bool {
        match self
            .inner
            .connected()
            .context("Failed to get connection status: FFI call failed with exception")
            .expect("Shouldn't panic")
        {
            ffi::ConnectionStatus::Connected => {
                tracing::trace!("Daemon is connected");
                true
            }
            ffi::ConnectionStatus::WrongVersion => {
                tracing::error!("Version mismatch with daemon, interpreting as disconnected");
                false
            }
            ffi::ConnectionStatus::Disconnected => {
                tracing::trace!("Daemon is disconnected");
                false
            }
            // Fallback since C++ allows any other value.
            status => {
                tracing::error!(
                    "Unknown connection status, interpreting as disconnected: `{}`",
                    status.repr
                );
                false
            }
        }
    }

    /// Set whether the daemon is trusted.
    ///
    /// This is needed for regnet compatibility.
    ///
    /// _Do not use for anything besides testing._
    fn set_trusted_daemon(&mut self, trusted: bool) {
        self.inner
            .pinned()
            .setTrustedDaemon(trusted)
            .context("Failed to set trusted daemon: FFI call failed with exception")
            .expect("Shouldn't panic");
    }

    /// Force a full sync of the wallet.
    /// Use only for regtest environments, utterly slow otherwise.
    fn force_full_sync(&mut self) {
        self.inner
            .pinned()
            .setRefreshFromBlockHeight(0)
            .context("Failed to set refresh from block height: FFI call failed with exception")
            .expect("Shouldn't panic");
    }

    /// Set the restore height of the wallet.
    pub fn set_restore_height(&mut self, height: u64) -> anyhow::Result<()> {
        self.inner
            .pinned()
            .setRefreshFromBlockHeight(height)
            .context("Failed to set restore height: FFI call failed with exception")
            .expect("Shouldn't panic");

        Ok(())
    }

    pub fn get_restore_height(&mut self) -> anyhow::Result<u64> {
        Ok(self
            .inner
            .pinned()
            .getRefreshFromBlockHeight()
            .context("Failed to get restore height: FFI call failed with exception")
            .expect("Shouldn't panic"))
    }

    pub fn get_blockchain_height_by_date(
        &mut self,
        year: u16,
        month: u8,
        day: u8,
    ) -> Result<u64, Exception> {
        self.inner
            .pinned()
            .getBlockchainHeightByDate(year, month, day)
    }

    /// Rescan the blockchain asynchronously.
    fn rescan_blockchain_async(&mut self) {
        self.inner.pinned().rescanBlockchainAsync();
    }

    /// Start the refresh.
    fn start_refresh(&mut self) {
        self.inner
            .pinned()
            .startRefresh()
            .context("Failed to start refresh: FFI call failed with exception")
            .expect("Shouldn't panic");
    }

    /// Pause the background refresh.
    fn pause_refresh(&mut self) {
        self.inner.pinned().pauseRefresh();
    }

    /// Stop the background refresh once (doesn't stop background refresh thread).
    fn stop(&mut self) {
        self.inner.pinned().stop();
    }

    /// Store the wallet state.
    fn store(&mut self, path: &str) {
        let_cxx_string!(path = path);
        self.inner.pinned().store(&path)
            .context("Failed to store wallet: FFI call failed with exception")
            .expect("Shouldn't panic");
    }

    /// Start the background refresh thread (refreshes every 10 seconds).
    fn start_refresh_thread(&mut self) {
        self.inner
            .pinned()
            .startRefresh()
            .context("Failed to start refresh: FFI call failed with exception")
            .expect("Shouldn't panic");
    }

    /// Refresh the wallet asynchronously.
    /// Same as start_refresh except that the background thread only
    /// refreshes once. Maybe?
    fn force_background_refresh(&mut self) {
        self.inner
            .pinned()
            .refreshAsync()
            .context("Failed to refresh wallet asynchronously: FFI call failed with exception")
            .expect("Shouldn't panic");
    }

    /// Refresh the wallet synchronously.
    /// No possibility for progress reporting.
    fn refresh_blocking(&mut self) -> anyhow::Result<()> {
        let success = self
            .inner
            .pinned()
            .refresh()
            .context("Failed to refresh wallet: FFI call failed with exception")?;

        if !success {
            let connected = self.connected();
            tracing::error!(connected, "Failed to sync Monero wallet");
            self.check_error().context("Failed to refresh wallet")?;
            anyhow::bail!("Failed to refresh wallet (no reason given)");
        }

        Ok(())
    }

    /// Get the wallet creation height.
    fn creation_height(&self) -> u64 {
        self.inner
            .getRefreshFromBlockHeight()
            .context("Failed to get refresh from block height: FFI call failed with exception")
            .expect("Shouldn't panic")
    }

    /// Get the current blockchain height.
    fn blockchain_height(&self) -> u64 {
        self.inner
            .blockChainHeight()
            .context("Failed to get blockchain height: FFI call failed with exception")
            .expect("Shouldn't panic")
    }

    /// Get the daemon's blockchain height.
    ///
    /// Returns the height of the blockchain, if connected.
    /// Returns None if not connected.
    fn daemon_blockchain_height(&self) -> Option<u64> {
        // Here we actually use the _target_ height -- incase the remote node is
        // currently catching up we want to work with the height it ends up at.
        match self
            .inner
            .daemonBlockChainTargetHeight()
            .context(
                "Failed to get daemon blockchain target height: FFI call failed with exception",
            )
            .expect("Shouldn't panic")
        {
            0 => None,
            height => Some(height),
        }
    }

    /// Get the total balance across all accounts.
    fn total_balance(&mut self) -> monero::Amount {
        let balance = self
            .inner
            .balanceAll()
            .context("Failed to get total balance: FFI call failed with exception")
            .expect("Shouldn't panic");
        monero::Amount::from_pico(balance)
    }

    /// Get the total unlocked balance across all accounts in atomic units.
    fn unlocked_balance(&mut self) -> monero::Amount {
        let balance = self
            .inner
            .unlockedBalanceAll()
            .context("Failed to get unlocked balance: FFI call failed with exception")
            .expect("Shouldn't panic");
        monero::Amount::from_pico(balance)
    }

    /// Check if the wallet is synced with the daemon.
    fn synchronized(&self) -> bool {
        self.inner
            .synchronized()
            .context("Failed to check if wallet is synchronized: FFI call failed with exception")
            .expect("Shouldn't panic")
    }

    /// Set the allow mismatched daemon version flag.
    ///
    /// This is needed for regnet compatibility.
    ///
    /// _Do not use for anything besides testing._
    fn allow_mismatched_daemon_version(&mut self) {
        self.inner
            .pinned()
            .setAllowMismatchedDaemonVersion(true)
            .context(
                "Failed to set allow mismatched daemon version: FFI call failed with exception",
            )
            .expect("Shouldn't panic");
    }

    /// Check the status of a transaction.
    fn check_tx_status(
        &mut self,
        txid: &str,
        tx_key: monero::PrivateKey,
        address: &monero::Address,
    ) -> anyhow::Result<TxStatus> {
        let_cxx_string!(txid = txid);
        let_cxx_string!(tx_key = tx_key.to_string());
        let_cxx_string!(address = address.to_string());

        let mut received = 0;
        let mut in_pool = false;
        let mut confirmations = 0;

        let raw_wallet = &mut self.inner;

        let success = ffi::checkTxKey(
            raw_wallet.pinned(),
            &txid,
            &tx_key,
            &address,
            &mut received,
            &mut in_pool,
            &mut confirmations,
        )
        .context("Failed to check tx key: FFI call failed with exception")?;

        if !success {
            self.check_error().context("Failed to check tx key")?;
            anyhow::bail!("Failed to check tx key");
        }

        Ok(TxStatus {
            received: monero::Amount::from_pico(received),
            in_pool,
            confirmations,
        })
    }

    /// Scan for a specified transaction.
    /// We use this to import the Monero tx_lock without having to do a
    /// full sync.
    /// This is much faster than a full sync.
    fn scan_transaction(&mut self, tx_id: String) -> anyhow::Result<()> {
        let_cxx_string!(tx_id = tx_id);

        let raw_wallet = &mut self.inner;
        let success = ffi::scanTransaction(raw_wallet.pinned(), &tx_id)
            .context("Failed to scan transaction: FFI call failed with exception")?;

        if !success {
            self.check_error().context("Failed to scan transaction")?;
            anyhow::bail!("Failed to scan transaction (no reason given)");
        }

        Ok(())
    }

    /// Transfer a specified amount of monero to a specified address and return a receipt containing
    /// the transaction id, transaction key and current blockchain height. This can be used later
    /// to prove the transfer or to wait for confirmations.
    fn transfer(
        &mut self,
        address: &monero::Address,
        amount: monero::Amount,
    ) -> anyhow::Result<TxReceipt> {
        let mut pending_tx = self.create_pending_transaction(address, amount)?;
        let result = self.publish_pending_transaction(&mut pending_tx);
        self.dispose_pending_transaction(pending_tx);
        result
    }

    /// Create a pending transaction without publishing it.
    /// Returns the pending transaction that can be inspected before publishing.
    fn create_pending_transaction(
        &mut self,
        address: &monero::Address,
        amount: monero::Amount,
    ) -> anyhow::Result<PendingTransaction> {
        let_cxx_string!(address_str = address.to_string());
        let amount_pico = amount.as_pico();

        let pending_tx = PendingTransaction(
            ffi::createTransaction(self.inner.pinned(), &address_str, amount_pico)
                .context("Failed to create transaction: FFI call failed with exception")?,
        );

        Ok(pending_tx)
    }

    /// Create a pending sweep transaction without publishing it.
    /// Returns the pending transaction that can be inspected before publishing.
    fn create_pending_sweep_transaction(
        &mut self,
        address: &monero::Address,
    ) -> anyhow::Result<PendingTransaction> {
        let_cxx_string!(address_str = address.to_string());

        let pending_tx = PendingTransaction(
            ffi::createSweepTransaction(self.inner.pinned(), &address_str)
                .context("Failed to create sweep transaction: FFI call failed with exception")?,
        );

        Ok(pending_tx)
    }

    /// Publish a pending transaction and return a receipt.
    /// Note: Caller is responsible for disposing the pending transaction afterwards.
    fn publish_pending_transaction(
        &mut self,
        pending_tx: &mut PendingTransaction,
    ) -> anyhow::Result<TxReceipt> {
        // Get the txid from the pending transaction before we publish
        let txid = ffi::pendingTransactionTxId(pending_tx)
            .context("Failed to get txid from pending transaction: FFI call failed with exception")?
            .to_string();

        // Publish the transaction
        pending_tx
            .publish()
            .context("Failed to publish transaction")?;

        // Fetch the tx key from the wallet
        let_cxx_string!(txid_cxx = txid.clone());
        let tx_key = ffi::walletGetTxKey(&self.inner, &txid_cxx)
            .context("Failed to get tx key from wallet: FFI call failed with exception")?
            .to_string();

        // Get current blockchain height
        let height = self.blockchain_height();

        Ok(TxReceipt {
            txid,
            tx_key,
            height,
        })
    }

    /// Dispose of a pending transaction to free memory.
    fn dispose_pending_transaction(&mut self, pending_tx: PendingTransaction) {
        self.dispose_transaction(pending_tx);
    }

    /// Sweep all funds from the wallet to a specified address.
    /// Returns a list of transaction ids of the created transactions.
    fn sweep(&mut self, address: &monero::Address) -> anyhow::Result<Vec<TxReceipt>> {
        tracing::info!("Sweeping funds to {}, refreshing wallet first", address);

        self.refresh_blocking()?;

        let_cxx_string!(address = address.to_string());

        // Create the sweep transaction
        let mut pending_tx = PendingTransaction(
            ffi::createSweepTransaction(self.inner.pinned(), &address)
                .context("Failed to create sweep transaction: FFI call failed with exception")?,
        );

        // Get the txids from the pending transaction before we publish,
        // otherwise it might be null.
        let txids: Vec<String> = ffi::pendingTransactionTxIds(&pending_tx)
            .context("Failed to get txids of pending transaction: FFI call failed with exception")?
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        // Publish the transaction
        let result = pending_tx
            .publish()
            .context("Failed to publish transaction");

        // Dispose of the transaction to avoid leaking memory.
        self.dispose_transaction(pending_tx);

        // Check for errors only after cleaning up the memory.
        result.context("Failed to publish transaction")?;

        // Get the receipts for the transactions.
        let mut receipts = Vec::new();

        for txid in txids {
            let_cxx_string!(txid_cxx = &txid);

            let tx_key = ffi::walletGetTxKey(&self.inner, &txid_cxx)
                .context("Failed to get tx key from wallet: FFI call failed with exception")?
                .to_string();

            let height = self.blockchain_height();

            receipts.push(TxReceipt {
                txid: txid.clone(),
                tx_key,
                height,
            });
        }

        Ok(receipts)
    }

    /// Sweep all funds to a set of addresses with a set of ratios.
    fn sweep_multi(
        &mut self,
        addresses: &[monero::Address],
        ratios: &[f64],
    ) -> anyhow::Result<Vec<TxReceipt>> {
        tracing::warn!("STARTED MULTI SWEEP");

        if addresses.is_empty() {
            bail!("No addresses to sweep to");
        }

        if addresses.len() != ratios.len() {
            bail!("Number of addresses and ratios must match");
        }

        tracing::info!(
            "Sweeping funds to {} addresses, refreshing wallet first",
            addresses.len()
        );

        self.refresh_blocking()?;

        let balance = self.unlocked_balance();

        // Since we're using "subtract fee from outputs", we distribute the full balance
        // The underlying transaction creation will subtract the fee proportionally from each output
        let amounts = FfiWallet::distribute(balance, ratios)?;

        tracing::debug!(%balance, num_outputs = addresses.len(), outputs=?amounts, "Distributing funds to outputs");

        // Build a C++ vector of destination addresses
        let mut cxx_addrs: UniquePtr<CxxVector<CxxString>> = CxxVector::<CxxString>::new();
        for addr in addresses {
            let_cxx_string!(s = addr.to_string());
            ffi::vector_string_push_back(cxx_addrs.pin_mut(), &s);
        }

        // Build a C++ vector of amounts
        let mut cxx_amounts: UniquePtr<CxxVector<u64>> = CxxVector::<u64>::new();
        for &amount in &amounts {
            cxx_amounts.pin_mut().push(amount.as_pico());
        }

        // Create the multi-sweep pending transaction
        let raw_tx = ffi::createTransactionMultiDest(
            self.inner.pinned(),
            cxx_addrs.as_ref().unwrap(),
            cxx_amounts.as_ref().unwrap(),
        );

        if raw_tx.is_null() {
            self.check_error()
                .context("Failed to create multi-sweep transaction")?;
            anyhow::bail!("Failed to create multi-sweep transaction");
        }

        let mut pending_tx = PendingTransaction(raw_tx);

        // Get the txids from the pending transaction before we publish,
        // otherwise it might be null.
        let txids: Vec<String> = ffi::pendingTransactionTxIds(&pending_tx)
            .context("Failed to get txids of pending transaction: FFI call failed with exception")?
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        // Publish the transaction
        let result = pending_tx
            .publish()
            .context("Failed to publish transaction");

        // Dispose of the transaction to avoid leaking memory.
        self.dispose_transaction(pending_tx);

        // Check for errors only after cleaning up the memory.
        result.context("Failed to publish transaction")?;

        // Get the receipts for the transactions.
        let mut receipts = Vec::new();

        for txid in txids {
            let_cxx_string!(txid_cxx = &txid);

            let tx_key = ffi::walletGetTxKey(&self.inner, &txid_cxx)
                .context("Failed to get tx key from wallet: FFI call failed with exception")?
                .to_string();

            let height = self.blockchain_height();

            receipts.push(TxReceipt {
                txid: txid.clone(),
                tx_key,
                height,
            });
        }

        Ok(receipts)
    }

    /// Distribute the funds in the wallet to a set of addresses with a set of percentages,
    /// such that the complete balance is spent (takes fee into account).
    ///
    /// # Arguments
    ///
    /// * `balance` - The total balance to distribute
    /// * `percentages` - A slice of percentages that must sum to 100.0
    ///
    /// # Returns
    ///
    /// A vector of Monero amounts proportional to the input percentages.
    /// The last amount gets any remainder to ensure exact distribution.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Percentages don't sum to 1.0
    /// - Balance is zero
    /// - There are more outputs than piconeros in balance
    fn distribute(balance: monero::Amount, percentages: &[f64]) -> Result<Vec<monero::Amount>> {
        if percentages.is_empty() {
            bail!("No ratios to distribute to");
        }

        const TOLERANCE: f64 = 1e-6;
        let sum: f64 = percentages.iter().sum();
        if (sum - 1.0).abs() > TOLERANCE {
            bail!("Percentages must sum to 1 (actual sum: {})", sum);
        }

        // Handle the case where distributable amount is zero
        if balance.as_pico() == 0 {
            bail!("Zero balance to distribute");
        }

        // Check if the distributable amount is enough to cover at least one piconero per output
        if balance.as_pico() < percentages.len() as u64 {
            bail!("More outputs than piconeros in balance");
        }

        let mut amounts = Vec::new();
        let mut total = Amount::ZERO;

        // Distribute amounts according to ratios, except for the last one
        for &percentage in &percentages[..percentages.len() - 1] {
            let amount_pico = ((balance.as_pico() as f64) * percentage).floor() as u64;
            let amount = Amount::from_pico(amount_pico);
            amounts.push(amount);
            total += amount;
        }

        // Give the remainder to the last recipient to ensure exact distribution
        let remainder = balance.checked_sub(total).context(format!(
            "Underflow when calculating rest (unexpected) - balance {}, distributed: {}",
            balance, total,
        ))?;
        amounts.push(remainder);

        Ok(amounts)
    }

    /// Get the transaction history.
    /// Returns an empty vector if the transaction history is missing.
    fn history(&mut self) -> Vec<TransactionInfo> {
        let history_ptr = self
            .inner
            .pinned()
            .history()
            .context("Failed to get transaction history: FFI call failed with exception");

        let Ok(history_ptr) = history_ptr else {
            tracing::error!(error=%history_ptr.unwrap_err(), "Failed to get transaction history, proceeding with empty history");
            return vec![];
        };

        let history = unsafe {
            // Safety: the pointer is valid as long as the wallet is alive (which is is when we called this a millisecond ago)
            Pin::new_unchecked(
                history_ptr
                    // Safety: this pointer isn't exclusive, however this is how we're supposed to do this according to the api
                    .as_mut()
                    .expect("history pointer to not be null after we just checked"),
            )
        };
        // Ignore result, we'll proceed anyway
        let _ = history
            .refresh()
            .context("Failed to refresh transaction history: FFI call failed with exception")
            .inspect_err(|e| tracing::error!(error=%e,"Failed to refresh transaction history"));

        let history_handle = TransactionHistoryHandle(history_ptr);
        let count = history_handle.count();

        let mut transactions = Vec::new();
        for i in 0..count {
            if let Some(tx_info_handle) = history_handle.transaction(i) {
                if let Some(serialized_tx) = tx_info_handle.serialize() {
                    transactions.push(serialized_tx);
                }
            }
        }
        transactions
    }

    /// Dispose (deallocate) a pending transaction object.
    /// Always call this before dropping a pending transaction object,
    /// otherwise we leak memory.
    fn dispose_transaction(&mut self, tx: PendingTransaction) {
        // Safety: we pass a valid pointer and we verified it's not used again since PendingTransaction is moved into this function
        unsafe {
            self.inner
                .pinned()
                .disposeTransaction(tx.0)
                .context("Failed to dispose transaction: FFI call failed with exception")
                .expect("Shouldn't panic");
        }
    }

    /// Return `Ok` when the wallet is ok, otherwise return the error.
    /// This is a convenience method we use for retrieving errors after
    /// a method call failed.
    ///
    /// We have to pass the raw wallet here to make sure we don't have to
    /// release the mutex in between an operation and the check.
    fn check_error(&self) -> anyhow::Result<()> {
        let mut status = 0;
        let mut error_string = String::new();
        let_cxx_string!(error_string_ref = &mut error_string);

        self.inner
            .statusWithErrorString(&mut status, error_string_ref)
            .context("Failed to get wallet status: FFI call failed with exception")?;

        // If the status is ok, we return None
        if status == 0 {
            return Ok(());
        }

        let error_string = if error_string.is_empty() {
            "unknown error, error not set".to_string()
        } else {
            error_string
        };

        let error_type = if status == 2 { "critical" } else { "error" };

        // Otherwise we return the error
        bail!(format!(
            "Experienced wallet error ({}): `{}`",
            error_type,
            error_string.to_string()
        ))
    }

    /// Get the seed of the wallet.
    fn seed(&mut self) -> anyhow::Result<String> {
        let_cxx_string!(language = "English");
        self.inner
            .pinned()
            .setSeedLanguage(&language)
            .context("Failed to set seed language")?;

        let_cxx_string!(seed_offset = "");
        let seed = ffi::walletSeed(&self.inner, &seed_offset)
            .context("Failed to get wallet seed: FFI call failed with exception")
            .expect("Shouldn't panic")
            .to_string();

        if seed.is_empty() {
            bail!("Failed to get wallet seed");
        }

        Ok(seed)
    }

    /// Sign a message with the wallet's private key.
    ///
    /// # Arguments
    /// * `message` - The message to sign (arbitrary byte data)
    /// * `address` - The address to use for signing (uses main address if None)
    /// * `sign_with_view_key` - Whether to sign with view key instead of spend key (default: false)
    ///
    /// # Returns
    /// A proof type prefix + base58 encoded signature
    pub fn sign_message(
        &mut self,
        message: &str,
        address: Option<&str>,
        sign_with_view_key: bool,
    ) -> anyhow::Result<String> {
        let_cxx_string!(message_cxx = message);
        let_cxx_string!(address_cxx = address.unwrap_or(""));

        let signature = ffi::signMessage(
            self.inner.pinned(),
            &message_cxx,
            &address_cxx,
            sign_with_view_key,
        )
        .context("Failed to sign message: FFI call failed with exception")?
        .to_string();

        if signature.is_empty() {
            self.check_error().context("Failed to sign message")?;
            anyhow::bail!("Failed to sign message (no signature returned)");
        }

        Ok(signature)
    }
}

impl PendingTransaction {
    /// Return `Ok` when the pending transaction is ok, otherwise return the error.
    /// This is a convenience method we use for retrieving errors after
    /// a method call failed.
    fn check_error(&self) -> anyhow::Result<()> {
        let status = self
            .status()
            .context("Failed to get pending transaction status: FFI call failed with exception")?;
        let error_string = ffi::pendingTransactionErrorString(self)
            .context(
                "Failed to get pending transaction error string: FFI call failed with exception",
            )?
            .to_string();

        if status == 0 {
            return Ok(());
        }

        let error_type = if status == 2 { "critical" } else { "error" };

        bail!(format!(
            "Experienced pending transaction error ({}): {}",
            error_type, error_string
        ))
    }

    /// Publish this transaction to the blockchain or return an error.
    ///
    /// **Important**: you still have to dispose the transaction.
    fn publish(&mut self) -> anyhow::Result<()> {
        self.check_error().context("Failed to create transaction")?;

        // Then we commit it to the blockchain.
        let_cxx_string!(filename = ""); // Empty filename means we commit to the blockchain
        let success = self.pinned().commit(&filename, false).context(
            "Failed to commit transaction to blockchain: FFI call failed with exception",
        )?;

        if success {
            Ok(())
        } else {
            // Get the error from the pending transaction.
            Err(self
                .check_error()
                .context("Failed to commit transaction to blockchain")
                .err()
                .unwrap_or(anyhow::anyhow!(
                    "Failed to commit transaction to blockchain"
                )))
        }
    }
}

impl SyncProgress {
    /// Create a new sync progress object.
    fn new(current_block: u64, target_block: u64) -> Self {
        Self {
            current_block,
            target_block,
        }
    }

    /// Create a new sync progress object with zero progess.
    fn zero() -> Self {
        Self {
            current_block: 0,
            target_block: 1,
        }
    }

    /// Get the sync progress as a fraction.
    pub fn fraction(&self) -> f32 {
        if self.target_block == 0 {
            return 0.0;
        }

        // Handle the case where current_block is greater than target_block
        if self.current_block >= self.target_block {
            return 1.0;
        }

        self.current_block as f32 / self.target_block as f32
    }

    /// Get the sync progress as a percentage.
    pub fn percentage(&self) -> f32 {
        100.0 * self.fraction()
    }
}

impl Display for SyncProgress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}%", self.percentage())
    }
}

impl PartialOrd for SyncProgress {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.fraction().partial_cmp(&other.fraction())
    }
}

impl PartialEq for SyncProgress {
    fn eq(&self, other: &Self) -> bool {
        self.fraction() == other.fraction()
    }
}

impl RawWallet {
    fn new(inner: *mut ffi::Wallet) -> Self {
        Self { inner }
    }

    /// Convenience method for getting a pinned reference to the inner (c++) wallet.
    fn pinned(&mut self) -> Pin<&mut ffi::Wallet> {
        // Safety: we know this is a valid pointer in the original thread
        unsafe { Pin::new_unchecked(self.inner.as_mut().expect("wallet pointer not to be null")) }
    }
}

// We implement Deref for RawWallet such that we can use the
// const c++ methods directly on the RawWallet struct.
impl Deref for RawWallet {
    type Target = ffi::Wallet;

    fn deref(&self) -> &ffi::Wallet {
        // Safety: we know this is a valid pointer in the original thread
        unsafe { self.inner.as_ref().expect("wallet pointer not to be null") }
    }
}

impl PendingTransaction {
    fn pinned(&mut self) -> Pin<&mut ffi::PendingTransaction> {
        // Safety: we know this is a valid pointer in the original thread
        unsafe {
            Pin::new_unchecked(
                self.0
                    .as_mut()
                    .expect("pending transaction pointer not to be null"),
            )
        }
    }
}

impl Deref for PendingTransaction {
    type Target = ffi::PendingTransaction;

    fn deref(&self) -> &ffi::PendingTransaction {
        // Safety: we know this is a valid pointer in the original thread
        unsafe {
            self.0
                .as_ref()
                .expect("pending transaction pointer not to be null")
        }
    }
}

impl TransactionHistoryHandle {
    /// Get the number of transactions in the history.
    pub fn count(&self) -> i32 {
        self.deref().count()
    }

    /// Get a transaction from the history by index.
    pub fn transaction(&self, index: i32) -> Option<TransactionInfoHandle> {
        let tx_info_ptr = self.deref().transaction(index);

        if tx_info_ptr.is_null() {
            None
        } else {
            // We wrap the raw pointer in our safe wrapper struct.
            Some(TransactionInfoHandle(tx_info_ptr))
        }
    }
}

impl Deref for TransactionHistoryHandle {
    type Target = ffi::TransactionHistory;

    fn deref(&self) -> &Self::Target {
        // Safety: we know this is a valid pointer in the original thread
        unsafe {
            self.0
                .as_ref()
                .expect("transaction history pointer not to be null")
        }
    }
}

impl TransactionInfoHandle {
    /// Get the amount of the transaction.
    pub fn amount(&self) -> u64 {
        self.deref().amount()
    }

    /// Get the fee of the transaction.
    pub fn fee(&self) -> u64 {
        self.deref().fee()
    }

    /// Get the confirmations of the transaction.
    pub fn confirmations(&self) -> u64 {
        self.deref().confirmations()
    }

    /// Get the hash of the transaction.
    pub fn hash(&self) -> String {
        ffi::transactionInfoHash(self.deref()).to_string()
    }

    /// Get the direction of the transaction.
    pub fn direction(&self) -> Result<TransactionDirection> {
        match self.deref().direction() {
            0 => Ok(TransactionDirection::In),
            1 => Ok(TransactionDirection::Out),
            otherwise => bail!(
                "Invalid transaction direction received from ffi call: `{}`",
                otherwise
            ),
        }
    }

    /// Get the timestamp of the transaction.
    pub fn timestamp(&self) -> u64 {
        ffi::transactionInfoTimestamp(self.deref())
    }

    pub fn serialize(&self) -> Option<TransactionInfo> {
        let fee = self.fee();
        let amount = self.amount();
        let confirmations = self.confirmations();
        let tx_hash = self.hash();
        let timestamp = self.timestamp();
        let direction = self
            .direction()
            .inspect_err(
                |e| tracing::error!(error=%e, %tx_hash, "Failed to get direction of transaction"),
            )
            .ok()?;

        Some(TransactionInfo {
            fee: monero::Amount::from_pico(fee),
            amount: monero::Amount::from_pico(amount),
            confirmations,
            tx_hash,
            direction,
            timestamp,
        })
    }
}

impl Deref for TransactionInfoHandle {
    type Target = ffi::TransactionInfo;

    fn deref(&self) -> &Self::Target {
        // Safety: we know this is a valid pointer in the original thread
        unsafe {
            self.0
                .as_ref()
                .expect("transaction info pointer not to be null")
        }
    }
}

pub struct WalletHandleListener {
    handle: Arc<WalletHandle>,
    rt_handle: tokio::runtime::Handle,
}

impl WalletHandleListener {
    pub fn new(handle: Arc<WalletHandle>) -> Self {
        let rt_handle = tokio::runtime::Handle::current();
        Self { handle, rt_handle }
    }
}

impl WalletEventListener for WalletHandleListener {
    fn on_money_spent(&self, _txid: &str, _amount: u64) {}

    fn on_money_received(&self, _txid: &str, _amount: u64) {}

    fn on_unconfirmed_money_received(&self, _txid: &str, _amount: u64) {}

    fn on_new_block(&self, _height: u64) {}

    fn on_updated(&self) {}

    fn on_refreshed(&self) {
        // When the wallet finishes refreshing, we start the refresh thread again.
        // The purpose of this is to ensure that if the user does a rescan (restore height changed)
        // We start the refresh thread again after the rescan is complete.
        let handle = self.handle.clone();
        self.rt_handle.spawn(async move {
            handle.start_refresh_thread().await;
        });
    }

    fn on_reorg(&self, _height: u64, _blocks_detached: u64, _transfers_detached: usize) {}

    fn on_pool_tx_removed(&self, _txid: &str) {}
}

pub mod monero_serde {
    use monero::Amount;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(x: &Amount, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_u64(x.as_pico())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Amount, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let picos = u64::deserialize(deserializer)?;
        let amount = Amount::from_pico(picos);

        Ok(amount)
    }
}

/// Create a backoff strategy for retrying a function.
/// Default max elapsed time is 5 minutes, default max interval is 30 seconds.
fn backoff(
    max_elapsed_time_secs: impl Into<Option<u64>>,
    max_interval_secs: impl Into<Option<u64>>,
) -> backoff::ExponentialBackoff {
    let max_elapsed_time_secs: Option<u64> = max_elapsed_time_secs.into();
    let max_elapsed_time = Duration::from_secs(max_elapsed_time_secs.unwrap_or(5 * 60));

    let max_interval_secs: Option<u64> = max_interval_secs.into();
    let max_interval = Duration::from_secs(max_interval_secs.unwrap_or(30));

    backoff::ExponentialBackoffBuilder::new()
        .with_max_elapsed_time(Some(max_elapsed_time))
        .with_max_interval(max_interval)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::TestResult;
    use quickcheck_macros::quickcheck;

    #[quickcheck]
    fn prop_distribute_sum_equals_balance(balance_pico: u64, percentages: Vec<f64>) -> TestResult {
        // Filter out invalid inputs
        if percentages.is_empty() || balance_pico == 0 {
            return TestResult::discard();
        }

        // Ensure percentages are valid (non-negative and sum to approximately 1.0)
        if percentages.iter().any(|&p| !(0.0..=1.0).contains(&p)) {
            return TestResult::discard();
        }

        let percentage_sum: f64 = percentages.iter().sum();
        if (percentage_sum - 1.0).abs() > 1e-6 {
            return TestResult::discard();
        }

        let balance = monero::Amount::from_pico(balance_pico);

        let amounts = FfiWallet::distribute(balance, &percentages);

        // Property: sum of distributed amounts should equal balance
        let total_distributed: u64 = amounts.unwrap().iter().map(|a| a.as_pico()).sum();
        let expected = balance.as_pico();

        TestResult::from_bool(total_distributed == expected)
    }

    #[quickcheck]
    fn prop_distribute_count_matches_percentages(
        balance_pico: u64,
        percentages: Vec<f64>,
    ) -> TestResult {
        if percentages.is_empty() || balance_pico == 0 {
            return TestResult::discard();
        }

        if percentages.iter().any(|&p| !(0.0..=1.0).contains(&p)) {
            return TestResult::discard();
        }

        let percentage_sum: f64 = percentages.iter().sum();
        if (percentage_sum - 1.0).abs() > 1e-6 {
            return TestResult::discard();
        }

        let balance = monero::Amount::from_pico(balance_pico);

        let amounts = FfiWallet::distribute(balance, &percentages).unwrap();

        // Property: number of amounts should equal number of percentages
        TestResult::from_bool(amounts.len() == percentages.len())
    }

    #[quickcheck]
    fn prop_distribute_respects_percentages(
        balance_pico: u64,
        percentages: Vec<f64>,
    ) -> TestResult {
        if percentages.len() < 2 || balance_pico == 0 {
            return TestResult::discard();
        }

        if percentages.iter().any(|&p| !(0.0..=1.0).contains(&p)) {
            return TestResult::discard();
        }

        let percentage_sum: f64 = percentages.iter().sum();
        if (percentage_sum - 1.0).abs() > 1e-6 {
            return TestResult::discard();
        }

        let balance = monero::Amount::from_pico(balance_pico);

        let amounts = FfiWallet::distribute(balance, &percentages).unwrap();

        // Property: percentages should be approximately respected (except for rounding)
        // We check all but the last amount since the last one gets the remainder
        let mut percentages_respected = true;
        for i in 0..percentages.len() - 1 {
            let expected_amount = ((balance.as_pico() as f64) * percentages[i]).floor() as u64;
            if amounts[i].as_pico() != expected_amount {
                percentages_respected = false;
                break;
            }
        }

        TestResult::from_bool(percentages_respected)
    }

    #[test]
    fn test_distribute_empty_percentages() {
        let balance = monero::Amount::from_pico(1000);
        let percentages: Vec<f64> = vec![];

        let amounts = FfiWallet::distribute(balance, &percentages);
        assert!(amounts.is_err());
    }

    #[test]
    fn test_distribute_zero_balance() {
        let balance = monero::Amount::from_pico(0);
        let percentages = vec![0.5, 0.5];

        let amounts = FfiWallet::distribute(balance, &percentages);
        assert!(amounts.is_err());
    }

    #[test]
    fn test_distribute_insufficient_balance_for_outputs() {
        let balance = monero::Amount::from_pico(2);
        let percentages = vec![0.3, 0.3, 0.4]; // 3 outputs but only 2 piconeros

        let amounts = FfiWallet::distribute(balance, &percentages);
        assert!(amounts.is_err());
    }

    #[test]
    fn test_distribute_simple_case() {
        let balance = monero::Amount::from_pico(1000);
        let percentages = vec![0.5, 0.3, 0.2];

        let amounts = FfiWallet::distribute(balance, &percentages).unwrap();

        assert_eq!(amounts.len(), 3);

        // Total should equal balance
        let total: u64 = amounts.iter().map(|a| a.as_pico()).sum();
        assert_eq!(total, 1000);

        // First two amounts should respect percentages exactly
        assert_eq!(amounts[0].as_pico(), 500); // 50% of 1000
        assert_eq!(amounts[1].as_pico(), 300); // 30% of 1000
                                               // Last amount gets remainder: 1000 - 500 - 300 = 200
        assert_eq!(amounts[2].as_pico(), 200);
    }

    #[test]
    fn test_distribute_small_donation() {
        let balance = monero::Amount::from_pico(1000);
        let percentages = vec![0.999, 0.001];

        let amounts = FfiWallet::distribute(balance, &percentages).unwrap();

        assert_eq!(amounts.len(), 2);

        // Total should equal balance
        let total: u64 = amounts.iter().map(|a| a.as_pico()).sum();
        assert_eq!(total, 1000);

        // First amount should respect percentage exactly
        assert_eq!(amounts[0].as_pico(), 999); // 99.9% of 1000 (floored)
                                               // Last amount gets remainder: 1000 - 999 = 1
        assert_eq!(amounts[1].as_pico(), 1);
    }

    #[test]
    fn test_distribute_percentages_not_sum_to_1() {
        let balance = monero::Amount::from_pico(1000);
        let percentages = vec![0.5, 0.3]; // Only sums to 0.8

        let amounts = FfiWallet::distribute(balance, &percentages);
        assert!(amounts.is_err());
    }
}
