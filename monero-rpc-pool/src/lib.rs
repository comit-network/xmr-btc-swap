use std::sync::Arc;

use anyhow::Result;
use axum::{
    routing::{any, get},
    Router,
};
use monero::Network;

use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

pub trait ToNetworkString {
    fn to_network_string(&self) -> String;
}

impl ToNetworkString for Network {
    fn to_network_string(&self) -> String {
        match self {
            Network::Mainnet => "mainnet".to_string(),
            Network::Stagenet => "stagenet".to_string(),
            Network::Testnet => "testnet".to_string(),
        }
    }
}

pub mod config;
pub mod database;
pub mod pool;
pub mod proxy;
pub mod types;

use config::Config;
use database::Database;
use pool::{NodePool, PoolStatus};
use proxy::{proxy_handler, stats_handler};

#[derive(Clone)]
pub struct AppState {
    pub node_pool: Arc<NodePool>,
}

/// Manages background tasks for the RPC pool
pub struct PoolHandle {
    pub status_update_handle: JoinHandle<()>,
}

impl Drop for PoolHandle {
    fn drop(&mut self) {
        self.status_update_handle.abort();
    }
}

/// Information about a running RPC pool server
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub port: u16,
    pub host: String,
}

async fn create_app_with_receiver(
    config: Config,
    network: Network,
) -> Result<(
    Router,
    tokio::sync::broadcast::Receiver<PoolStatus>,
    PoolHandle,
)> {
    // Initialize database
    let db = Database::new(config.data_dir.clone()).await?;

    // Initialize node pool with network
    let network_str = network.to_network_string();
    let (node_pool, status_receiver) = NodePool::new(db.clone(), network_str.clone());
    let node_pool = Arc::new(node_pool);

    // Publish initial status immediately to ensure first event is sent
    if let Err(e) = node_pool.publish_status_update().await {
        error!("Failed to publish initial status update: {}", e);
    }

    // Send status updates every 10 seconds
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
    let node_pool_for_health_check = node_pool.clone();
    let status_update_handle = tokio::spawn(async move {
        loop {
            if let Err(e) = node_pool_for_health_check.publish_status_update().await {
                error!("Failed to publish status update: {}", e);
            }

            interval.tick().await;
        }
    });

    let pool_handle = PoolHandle {
        status_update_handle,
    };

    let app_state = AppState { node_pool };

    // Build the app
    let app = Router::new()
        .route("/stats", get(stats_handler))
        .route("/*path", any(proxy_handler))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    Ok((app, status_receiver, pool_handle))
}

pub async fn create_app(config: Config, network: Network) -> Result<Router> {
    let (app, _, _pool_handle) = create_app_with_receiver(config, network).await?;
    // Note: pool_handle is dropped here, so tasks will be aborted when this function returns
    // This is intentional for the simple create_app use case
    Ok(app)
}

/// Create an app with a custom data directory for the database
pub async fn create_app_with_data_dir(
    config: Config,
    network: Network,
    data_dir: std::path::PathBuf,
) -> Result<Router> {
    let config_with_data_dir = Config::new_with_port(config.host, config.port, data_dir);
    create_app(config_with_data_dir, network).await
}

pub async fn run_server(config: Config, network: Network) -> Result<()> {
    let app = create_app(config.clone(), network).await?;

    let bind_address = format!("{}:{}", config.host, config.port);
    info!("Starting server on {}", bind_address);

    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    info!("Server listening on {}", bind_address);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Run a server with a custom data directory
pub async fn run_server_with_data_dir(
    config: Config,
    network: Network,
    data_dir: std::path::PathBuf,
) -> Result<()> {
    let config_with_data_dir = Config::new_with_port(config.host, config.port, data_dir);
    run_server(config_with_data_dir, network).await
}

/// Start a server with a random port for library usage
/// Returns the server info with the actual port used, a receiver for pool status updates, and pool handle
pub async fn start_server_with_random_port(
    config: Config,
    network: Network,
) -> Result<(
    ServerInfo,
    tokio::sync::broadcast::Receiver<PoolStatus>,
    PoolHandle,
)> {
    // Clone the host before moving config
    let host = config.host.clone();

    // If port is 0, the system will assign a random available port
    let config_with_random_port = Config::new_random_port(config.host, config.data_dir);

    let (app, status_receiver, pool_handle) =
        create_app_with_receiver(config_with_random_port, network).await?;

    // Bind to port 0 to get a random available port
    let listener = tokio::net::TcpListener::bind(format!("{}:0", host)).await?;
    let actual_addr = listener.local_addr()?;

    let server_info = ServerInfo {
        port: actual_addr.port(),
        host: host.clone(),
    };

    info!(
        "Started server on {}:{} (random port)",
        server_info.host, server_info.port
    );

    // Start the server in a background task
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("Server error: {}", e);
        }
    });

    Ok((server_info, status_receiver, pool_handle))
}

/// Start a server with a random port and custom data directory for library usage
/// Returns the server info with the actual port used, a receiver for pool status updates, and pool handle
pub async fn start_server_with_random_port_and_data_dir(
    config: Config,
    network: Network,
    data_dir: std::path::PathBuf,
) -> Result<(
    ServerInfo,
    tokio::sync::broadcast::Receiver<PoolStatus>,
    PoolHandle,
)> {
    let config_with_data_dir = Config::new_random_port(config.host, data_dir);
    start_server_with_random_port(config_with_data_dir, network).await
}
