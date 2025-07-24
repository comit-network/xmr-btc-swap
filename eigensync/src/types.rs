//! Common types and error definitions for eigensync

use serde::{Deserialize, Serialize};

/// Result type alias for eigensync operations
pub type Result<T> = std::result::Result<T, Error>;

/// PeerId type alias
pub type PeerId = libp2p::PeerId;

/// ActorId uniquely identifies an actor in the Automerge document
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActorId(pub automerge::ActorId);

impl From<automerge::ActorId> for ActorId {
    fn from(actor_id: automerge::ActorId) -> Self {
        Self(actor_id)
    }
}

impl From<ActorId> for automerge::ActorId {
    fn from(actor_id: ActorId) -> Self {
        actor_id.0
    }
}

impl std::fmt::Display for ActorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Comprehensive error types for eigensync operations
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_cbor::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Automerge error: {0}")]
    Automerge(#[from] automerge::AutomergeError),

    #[error("Network error: {0}")]
    Network(#[from] libp2p::swarm::DialError),

    #[error("Protocol error: {message}")]
    Protocol { message: String },

    #[error("Authentication failed for peer {peer_id}: {reason}")]
    Authentication { peer_id: PeerId, reason: String },

    #[error("Document not found: {document_id}")]
    DocumentNotFound { document_id: String },

    #[error("Invalid configuration: {message}")]
    InvalidConfig { message: String },

    #[error("Timeout: {operation}")]
    Timeout { operation: String },

    #[error("Actor mapping conflict: peer {peer_id} tried to use actor {actor_id} already mapped to different peer")]
    ActorMappingConflict { peer_id: PeerId, actor_id: ActorId },

    #[error("Storage quota exceeded: {current_size} bytes")]
    StorageQuotaExceeded { current_size: u64 },
}

/// Information about a patch/change in the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchInfo {
    /// Unique identifier for this patch
    pub id: uuid::Uuid,
    /// Actor that created this patch
    pub actor_id: ActorId,
    /// Timestamp when patch was created
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Size of the patch data in bytes
    pub size_bytes: u64,
    /// Hash of the patch content for integrity checking
    pub content_hash: String,
}

/// Current state of a document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentState {
    /// Document identifier (typically swap_id)
    pub document_id: String,
    /// Current number of patches applied
    pub patch_count: u64,
    /// Total size of all patches in bytes
    pub total_size_bytes: u64,
    /// Timestamp of last update
    pub last_updated: chrono::DateTime<chrono::Utc>,
    /// Current document heads
    pub heads: Vec<automerge::ChangeHash>,
}

/// Configuration for snapshot and garbage collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotConfig {
    /// Trigger snapshot after this many changes
    pub max_changes: u64,
    /// Trigger snapshot after this many bytes
    pub max_size_bytes: u64,
    /// Whether to compress snapshots above this size
    pub compress_threshold_bytes: u64,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            max_changes: 10_000,
            max_size_bytes: 10 * 1024 * 1024, // 10 MB
            compress_threshold_bytes: 1024 * 1024, // 1 MB
        }
    }
}

/// Metrics for monitoring sync operations
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SyncMetrics {
    /// Total number of changes sent
    pub changes_sent: u64,
    /// Total number of changes received
    pub changes_received: u64,
    /// Total number of bytes sent
    pub bytes_sent: u64,
    /// Total number of bytes received
    pub bytes_received: u64,
    /// Number of sync operations performed
    pub sync_operations: u64,
    /// Number of failed sync operations
    pub sync_failures: u64,
    /// Average round-trip time in milliseconds
    pub avg_rtt_ms: f64,
    /// Number of conflicts resolved
    pub conflicts_resolved: u64,
}

/// State of a peer connection
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerState {
    /// Peer is disconnected
    Disconnected,
    /// Peer is connecting
    Connecting,
    /// Peer is connected and authenticated
    Connected,
    /// Peer authentication failed
    AuthenticationFailed,
    /// Peer connection failed
    ConnectionFailed,
}

/// Information about a connected peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Peer ID (as string)
    pub peer_id: String,
    /// Associated actor ID
    pub actor_id: Option<ActorId>,
    /// Current connection state
    pub state: PeerState,
    /// When the peer was first seen
    pub first_seen: chrono::DateTime<chrono::Utc>,
    /// When the peer was last seen
    pub last_seen: chrono::DateTime<chrono::Utc>,
    /// Sync metrics for this peer
    pub metrics: SyncMetrics,
}

/// A batch of changes/patches for efficient transmission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeBatch {
    /// Unique identifier for this batch
    pub batch_id: uuid::Uuid,
    /// Document this batch applies to
    pub document_id: String,
    /// The actual changes (serialized)
    pub changes: Vec<Vec<u8>>,
    /// Metadata about each change
    pub patch_info: Vec<PatchInfo>,
}

/// Configuration for rate limiting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum requests per second per peer
    pub max_requests_per_second: u32,
    /// Maximum bytes per second per peer
    pub max_bytes_per_second: u64,
    /// Burst allowance
    pub burst_size: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests_per_second: 10,
            max_bytes_per_second: 1024 * 1024, // 1 MB/s
            burst_size: 5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actor_id_conversion() {
        let automerge_actor = automerge::ActorId::random();
        let actor_id = ActorId::from(automerge_actor.clone());
        let converted_back: automerge::ActorId = actor_id.into();
        assert_eq!(automerge_actor, converted_back);
    }

    #[test]
    fn test_snapshot_config_defaults() {
        let config = SnapshotConfig::default();
        assert_eq!(config.max_changes, 10_000);
        assert_eq!(config.max_size_bytes, 10 * 1024 * 1024);
        assert_eq!(config.compress_threshold_bytes, 1024 * 1024);
    }

    #[test]
    fn test_rate_limit_config_defaults() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_requests_per_second, 10);
        assert_eq!(config.max_bytes_per_second, 1024 * 1024);
        assert_eq!(config.burst_size, 5);
    }
} 