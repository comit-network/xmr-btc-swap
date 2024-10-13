use super::api::tauri_bindings::TauriEmitter;
use crate::bitcoin::{ExpiredTimelocks, Wallet};
use crate::cli::api::tauri_bindings::TauriHandle;
use crate::protocol::bob::BobState;
use crate::protocol::{Database, State};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// A long running task which watches for changes to timelocks and balance
#[derive(Clone)]
pub struct Watcher {
    wallet: Arc<Wallet>,
    database: Arc<dyn Database + Send + Sync>,
    tauri: Option<TauriHandle>,
    /// This saves for every running swap the last known timelock status
    cached_timelocks: HashMap<Uuid, Option<ExpiredTimelocks>>,
}

impl Watcher {
    /// How often to check for changes (in seconds)
    const CHECK_INTERVAL: u64 = 30;

    /// Create a new Watcher
    pub fn new(
        wallet: Arc<Wallet>,
        database: Arc<dyn Database + Send + Sync>,
        tauri: Option<TauriHandle>,
    ) -> Self {
        Self {
            wallet,
            database,
            cached_timelocks: HashMap::new(),
            tauri,
        }
    }

    /// Start running the watcher event loop.
    /// Should be done in a new task using [`tokio::spawn`].
    pub async fn run(mut self) {
        // Note: since this is a daemon, we have to gracefully handle errors
        // (which in our case means logging the error message and trying again later)
        loop {
            tokio::time::sleep(Duration::from_secs(Watcher::CHECK_INTERVAL)).await;

            match self.do_work().await {
                Ok(()) => (),
                Err(error) => tracing::error!(%error, "Watcher daemon encountered error"),
            }
        }
    }

    /// This function performs the actual work and get's called every [`Watcher::CHECK_INTERVAL`]
    /// seconds.
    ///
    /// Splitting the content of the loop like this allows better error handling.
    async fn do_work(&mut self) -> Result<()> {
        // Fetch the current Bitcoin balance
        let new_balance = self
            .wallet
            .balance()
            .await
            .context("Failed to fetch Bitcoin balance, retrying later")?;
        // Emit a balance update event
        self.tauri.emit_balance_update_event(new_balance);

        // Fetch current transactions and timelocks
        let current_swaps = self
            .get_current_swaps()
            .await
            .context("Failed to ftech current transactions, retrying later")?;

        // Check for changes for every current swap
        for (swap_id, state) in current_swaps {
            // Determine if the timelock has expired for the current swap.
            // We intentionally do not skip swaps with a None timelock status, as this represents a valid state.
            // When a swap reaches its final state, the timelock becomes irrelevant, but it is still important to explicitly send None
            // This indicates that the timelock no longer needs to be displayed in the GUI
            let new_timelock_status = state
                .expired_timelocks(self.wallet.clone())
                .await
                .context("Failed to check timelock status")?;

            // Check if the status changed
            if let Some(old_status) = self.cached_timelocks.get(&swap_id) {
                // And send a tauri event if it did
                if *old_status != new_timelock_status {
                    self.tauri
                        .emit_timelock_change_event(swap_id, new_timelock_status);
                }
            } else {
                // If this is the first time we see this swap, send a tauri event, too
                self.tauri
                    .emit_timelock_change_event(swap_id, new_timelock_status);
            }

            // Insert new status
            self.cached_timelocks.insert(swap_id, new_timelock_status);
        }

        Ok(())
    }

    /// Helper function for fetching the current list of swaps
    async fn get_current_swaps(&self) -> Result<Vec<(Uuid, BobState)>> {
        Ok(self
            .database
            .all()
            .await?
            .into_iter()
            // Filter for BobState
            .filter_map(|(uuid, state)| match state {
                State::Bob(bob_state) => Some((uuid, bob_state)),
                _ => None,
            })
            .collect())
    }
}
