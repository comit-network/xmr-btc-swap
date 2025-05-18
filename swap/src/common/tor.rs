use std::path::Path;
use std::sync::Arc;

use crate::cli::api::tauri_bindings::{
    TauriBackgroundProgress, TauriEmitter, TauriHandle, TorBootstrapStatus,
};
use arti_client::{config::TorClientConfigBuilder, status::BootstrapStatus, Error, TorClient};
use futures::StreamExt;
use tor_rtcompat::tokio::TokioRustlsRuntime;

pub async fn init_tor_client(
    data_dir: &Path,
    tauri_handle: Option<TauriHandle>,
) -> Result<Arc<TorClient<TokioRustlsRuntime>>, Error> {
    // We store the Tor state in the data directory
    let data_dir = data_dir.join("tor");
    let state_dir = data_dir.join("state");
    let cache_dir = data_dir.join("cache");

    // The client configuration describes how to connect to the Tor network,
    // and what directories to use for storing persistent state.
    let config = TorClientConfigBuilder::from_directories(state_dir, cache_dir)
        .build()
        .expect("We initialized the Tor client all required attributes");

    // Start the Arti client, and let it bootstrap a connection to the Tor network.
    // (This takes a while to gather the necessary directory information.
    // It uses cached information when possible.)
    let runtime = TokioRustlsRuntime::current().expect("We are always running with tokio");

    tracing::debug!("Bootstrapping Tor client");

    let tor_client = TorClient::with_runtime(runtime)
        .config(config)
        .create_unbootstrapped_async()
        .await?;

    let mut bootstrap_events = tor_client.bootstrap_events();

    // Create a background progress handle for the Tor bootstrap process
    // The handle manages the TauriHandle internally, so we don't need to worry about it anymore
    let progress_handle =
        tauri_handle.new_background_process(TauriBackgroundProgress::EstablishingTorCircuits);

    // Clone the handle for the task
    let progress_handle_clone = progress_handle.clone();

    // Start a task to monitor bootstrap events
    let progress_task = tokio::spawn(async move {
        loop {
            match bootstrap_events.next().await {
                Some(event) => {
                    let status = event.to_tauri_bootstrap_status();
                    progress_handle_clone.update(status);
                }
                None => continue,
            }
        }
    });

    // Run the bootstrap until it's complete
    tokio::select! {
        _ = progress_task => unreachable!("Tor bootstrap progress handle should never exit"),
        res = tor_client.bootstrap() => {
            progress_handle.finish();
            res
        },
    }?;

    Ok(Arc::new(tor_client))
}

// A trait to convert the Tor bootstrap event into a TauriBootstrapStatus
trait ToTauriBootstrapStatus {
    fn to_tauri_bootstrap_status(&self) -> TorBootstrapStatus;
}

impl ToTauriBootstrapStatus for BootstrapStatus {
    fn to_tauri_bootstrap_status(&self) -> TorBootstrapStatus {
        TorBootstrapStatus {
            frac: self.as_frac(),
            ready_for_traffic: self.ready_for_traffic(),
            blockage: self.blocked().map(|b| b.to_string()),
        }
    }
}
