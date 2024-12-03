use std::path::Path;
use std::sync::Arc;

use arti_client::{config::TorClientConfigBuilder, Error, TorClient};
use tor_rtcompat::tokio::TokioRustlsRuntime;

pub async fn init_tor_client(data_dir: &Path) -> Result<Arc<TorClient<TokioRustlsRuntime>>, Error> {
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
        .create_bootstrapped()
        .await?;

    Ok(Arc::new(tor_client))
}
