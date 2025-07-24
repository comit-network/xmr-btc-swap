//! Server-side components for eigensync

use crate::types::{Result, PeerId, ActorId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod database;
pub mod behaviour;
pub mod event_loop;

/// Configuration for the eigensync server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Path to server database
    pub database_path: PathBuf,
    /// Address to listen on
    pub listen_address: String,
    /// Port to listen on
    pub listen_port: u16,
    /// Maximum number of connected peers
    pub max_peers: u32,
    /// Rate limiting configuration
    pub rate_limit: crate::types::RateLimitConfig,
    /// Snapshot configuration
    pub snapshot_config: crate::types::SnapshotConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            database_path: PathBuf::from("server_patches.sqlite"),
            listen_address: "0.0.0.0".to_string(),
            listen_port: 9944,
            max_peers: 100,
            rate_limit: crate::types::RateLimitConfig::default(),
            snapshot_config: crate::types::SnapshotConfig::default(),
        }
    }
}

/// Main server struct (placeholder implementation)
pub struct Server {
    config: ServerConfig,
}

impl Server {
    /// Create a new server with the given configuration
    pub async fn new(config: ServerConfig) -> Result<Self> {
        tracing::info!("Creating eigensync server with config: {:?}", config);
        
        // TODO: Initialize database, networking, etc.
        
        Ok(Self { config })
    }

    /// Start the server
    pub async fn run(self) -> Result<()> {
        tracing::info!("Starting eigensync server on {}:{}", 
                      self.config.listen_address, self.config.listen_port);
        
        // TODO: Implement server event loop
        
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    /// Get server configuration
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.listen_address, "0.0.0.0");
        assert_eq!(config.listen_port, 9944);
        assert_eq!(config.max_peers, 100);
    }

    #[tokio::test]
    async fn test_server_creation() {
        let config = ServerConfig::default();
        let server = Server::new(config).await;
        assert!(server.is_ok());
    }
} 