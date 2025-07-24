//! Client sync loop for periodic synchronization with server

use crate::client::behaviour::ClientBehaviour;
use crate::client::database::ClientDatabase;
use crate::types::{Result, PeerId};
use std::time::Duration;

/// Client sync loop for periodic synchronization
pub struct ClientSyncLoop {
    behaviour: ClientBehaviour,
    database: ClientDatabase,
    server_peer_id: PeerId,
    sync_interval: Duration,
}

impl ClientSyncLoop {
    /// Create a new client sync loop
    pub fn new(
        behaviour: ClientBehaviour,
        database: ClientDatabase,
        server_peer_id: PeerId,
        sync_interval: Duration,
    ) -> Self {
        tracing::debug!("Creating client sync loop with interval {:?}", sync_interval);
        
        Self {
            behaviour,
            database,
            server_peer_id,
            sync_interval,
        }
    }

    /// Run the sync loop
    pub async fn run(mut self) -> Result<()> {
        tracing::info!("Starting client sync loop");
        
        let mut interval = tokio::time::interval(self.sync_interval);
        
        loop {
            interval.tick().await;
            
            match self.perform_sync().await {
                Ok(_) => {
                    tracing::debug!("Sync completed successfully");
                },
                Err(e) => {
                    tracing::warn!("Sync failed: {}", e);
                    // Continue syncing despite errors
                }
            }
        }
    }

    /// Perform a single sync operation
    async fn perform_sync(&mut self) -> Result<()> {
        tracing::debug!("Performing sync with server {}", self.server_peer_id);
        
        // TODO: Implement sync logic:
        // 1. Get list of documents to sync
        // 2. For each document:
        //    - Get local heads/metadata
        //    - Request changes since last sync
        //    - Apply received changes
        //    - Submit any local changes
        //    - Update local metadata
        
        Ok(())
    }

    /// Sync a specific document
    pub async fn sync_document(&mut self, document_id: &str) -> Result<()> {
        tracing::debug!("Syncing document {}", document_id);
        
        // TODO: Implement document-specific sync
        Ok(())
    }

    /// Force immediate sync (outside of normal interval)
    pub async fn sync_now(&mut self) -> Result<()> {
        tracing::info!("Forcing immediate sync");
        
        self.perform_sync().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_sync_loop_creation() {
        let behaviour = ClientBehaviour::new();
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test_cache.db");
        let database = crate::client::database::ClientDatabase::open(db_path).await.unwrap();
        let server_peer_id = PeerId::random();
        let sync_interval = Duration::from_secs(1);
        
        let _sync_loop = ClientSyncLoop::new(behaviour, database, server_peer_id, sync_interval);
        // Sync loop creation should not panic
    }
} 