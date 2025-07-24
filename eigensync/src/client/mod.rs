//! Client-side components for eigensync

use crate::types::{Result, PeerId, ActorId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

pub mod database;
pub mod behaviour;
pub mod sync_loop;
pub mod document;

/// Configuration for the eigensync client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    /// Path to client cache database
    pub cache_database_path: PathBuf,
    /// Server address to connect to
    pub server_address: String,
    /// Server port to connect to
    pub server_port: u16,
    /// Sync interval
    pub sync_interval: Duration,
    /// Connection timeout
    pub connection_timeout: Duration,
    /// Actor ID for this client
    pub actor_id: ActorId,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            cache_database_path: PathBuf::from("eigensync_cache.sqlite"),
            server_address: "127.0.0.1".to_string(),
            server_port: 9944,
            sync_interval: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(10),
            actor_id: ActorId(automerge::ActorId::random()),
        }
    }
}

/// Main client struct (placeholder implementation)
pub struct Client {
    config: ClientConfig,
}

impl Client {
    /// Create a new client with the given configuration
    pub async fn new(config: ClientConfig) -> Result<Self> {
        tracing::info!("Creating eigensync client with config: {:?}", config);
        
        // TODO: Initialize database, networking, etc.
        
        Ok(Self { config })
    }

    /// Start the client sync loop
    pub async fn start_sync(&mut self) -> Result<()> {
        tracing::info!("Starting eigensync client sync");
        
        // TODO: Implement client sync loop
        
        Ok(())
    }

    /// Append a swap state to the local document
    pub async fn append_swap_state(
        &mut self,
        swap_id: uuid::Uuid,
        state_json: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        tracing::debug!("Appending swap state for {}: {:?}", swap_id, state_json);
        
        // TODO: Implement state appending
        // 1. Load Automerge document for swap_id
        // 2. Add new state entry with timestamp
        // 3. Generate patch
        // 4. Store locally and mark for sync
        
        Ok(())
    }

    /// Get the latest state for a swap
    pub async fn get_latest_swap_state(
        &self,
        swap_id: uuid::Uuid,
    ) -> Result<Option<serde_json::Value>> {
        tracing::debug!("Getting latest swap state for {}", swap_id);
        
        // TODO: Implement state retrieval
        // 1. Load Automerge document for swap_id
        // 2. Find entry with latest timestamp
        // 3. Return state JSON
        
        Ok(None)
    }

    /// Get client configuration
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_default() {
        let config = ClientConfig::default();
        assert_eq!(config.server_address, "127.0.0.1");
        assert_eq!(config.server_port, 9944);
        assert_eq!(config.sync_interval, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_client_creation() {
        let config = ClientConfig::default();
        let client = Client::new(config).await;
        assert!(client.is_ok());
    }
} 