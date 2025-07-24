//! Eigensync: Distributed State Synchronization using Automerge CRDTs
//!
//! This crate provides a distributed state synchronization system built on top of
//! Automerge CRDTs and libp2p networking. It enables synchronizing append-only state
//! machines across multiple devices.
//!
//! # Features
//!
//! - **Append-only state synchronization**: Designed for state machines that only add states
//! - **Conflict-free replication**: Uses Automerge CRDTs to handle concurrent updates
//! - **Peer-to-peer networking**: Built on libp2p for reliable P2P communication
//! - **Persistent storage**: SQLite-based persistence for both server and client
//! - **Authentication**: PeerId-based authentication with ActorId mapping
//!
//! # Architecture
//!
//! The system consists of:
//! - **Server**: Stores and distributes patches per PeerId
//! - **Client**: Maintains local Automerge document and syncs with server
//! - **Protocol**: Request/response protocol for patch exchange

pub mod types;
pub mod protocol;

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "metrics")]
pub mod metrics;

// Re-export commonly used types
pub use types::{Error, Result, ActorId, PeerId, DocumentState, PatchInfo};
pub use protocol::{EigensyncMessage, EigensyncRequest, EigensyncResponse};

#[cfg(feature = "client")]
pub use client::{Client, ClientConfig};

#[cfg(feature = "server")]
pub use server::{Server, ServerConfig};

/// Version of the eigensync protocol
pub const PROTOCOL_VERSION: u32 = 1;

/// Protocol name for libp2p
pub const PROTOCOL_NAME: &str = "/eigensync/1.0.0";

/// Main entry point for integrating eigensync with swap state machine
#[cfg(feature = "client")]
pub async fn append_state(
    client: &mut Client,
    swap_id: uuid::Uuid,
    state_json: serde_json::Value,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    client.append_swap_state(swap_id, state_json, timestamp).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_constants() {
        assert_eq!(PROTOCOL_VERSION, 1);
        assert_eq!(PROTOCOL_NAME, "/eigensync/1.0.0");
    }
}
