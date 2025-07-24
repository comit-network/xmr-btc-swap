//! Server event loop for handling network events

use crate::server::behaviour::ServerBehaviour;
use crate::server::database::ServerDatabase;
use crate::types::{Result, PeerId};
use std::time::Duration;

/// Server event loop for handling network events and database operations
pub struct ServerEventLoop {
    behaviour: ServerBehaviour,
    database: ServerDatabase,
}

impl ServerEventLoop {
    /// Create a new server event loop
    pub fn new(behaviour: ServerBehaviour, database: ServerDatabase) -> Self {
        tracing::debug!("Creating server event loop");
        
        Self {
            behaviour,
            database,
        }
    }

    /// Run the server event loop
    pub async fn run(mut self) -> Result<()> {
        tracing::info!("Starting server event loop");
        
        // TODO: Implement actual event loop with libp2p swarm
        loop {
            // Placeholder: just sleep for now
            tokio::time::sleep(Duration::from_secs(1)).await;
            
            // TODO: 
            // - Poll swarm for events
            // - Handle incoming requests
            // - Manage peer connections
            // - Perform periodic maintenance tasks
        }
    }

    /// Handle peer connection
    pub async fn handle_peer_connected(&mut self, peer_id: PeerId) -> Result<()> {
        tracing::info!("Peer connected: {}", peer_id);
        
        // TODO: Implement peer connection handling
        Ok(())
    }

    /// Handle peer disconnection
    pub async fn handle_peer_disconnected(&mut self, peer_id: PeerId) -> Result<()> {
        tracing::info!("Peer disconnected: {}", peer_id);
        
        // TODO: Implement peer disconnection handling
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_event_loop_creation() {
        let behaviour = ServerBehaviour::new();
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let database = ServerDatabase::open(db_path).await.unwrap();
        
        let _event_loop = ServerEventLoop::new(behaviour, database);
        // Event loop creation should not panic
    }
} 