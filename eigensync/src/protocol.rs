//! Protocol definitions for eigensync communication
//!
//! This module defines the wire protocol used for communication between
//! eigensync clients and servers. The protocol is versioned and uses
//! serde_cbor for serialization with length-prefixed frames.

use crate::types::{ActorId, PeerId, Result, Error};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Protocol version for version negotiation
pub const CURRENT_VERSION: u32 = 1;

/// Maximum message size to prevent DoS attacks (10 MB)
pub const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

/// Default request timeout
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Main message envelope for all eigensync communications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EigensyncMessage {
    /// Protocol version
    pub version: u32,
    /// Unique request identifier for matching responses
    pub request_id: uuid::Uuid,
    /// Message payload
    pub payload: EigensyncPayload,
}

/// Union type for all message payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum EigensyncPayload {
    Request(EigensyncRequest),
    Response(EigensyncResponse),
}

/// All possible request types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "params")]
pub enum EigensyncRequest {
    /// Get changes from server since a given point
    GetChanges(GetChangesParams),
    /// Submit new changes to server
    SubmitChanges(SubmitChangesParams),
    /// Ping for connectivity testing
    Ping(PingParams),
    /// Get server status/info
    GetStatus(GetStatusParams),
}

/// All possible response types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "result")]
pub enum EigensyncResponse {
    /// Response to GetChanges request
    GetChanges(GetChangesResult),
    /// Response to SubmitChanges request
    SubmitChanges(SubmitChangesResult),
    /// Response to Ping request
    Ping(PingResult),
    /// Response to GetStatus request
    GetStatus(GetStatusResult),
    /// Error response for any request
    Error(ErrorResult),
}

// Request parameters

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetChangesParams {
    /// Document to get changes for (typically swap_id)
    pub document_id: String,
    /// Only return changes after this sequence number
    pub since_sequence: Option<u64>,
    /// Only return changes after this timestamp
    pub since_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    /// Maximum number of changes to return (for pagination)
    pub limit: Option<u32>,
    /// Automerge heads we already have (to optimize sync)
    pub have_heads: Vec<automerge::ChangeHash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitChangesParams {
    /// Document these changes apply to
    pub document_id: String,
    /// Serialized Automerge changes
    pub changes: Vec<Vec<u8>>,
    /// Actor ID that created these changes
    pub actor_id: ActorId,
    /// Expected sequence number for optimistic concurrency control
    pub expected_sequence: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingParams {
    /// Timestamp when ping was sent
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Optional payload for bandwidth testing
    pub payload: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetStatusParams {
    /// Include detailed statistics
    pub include_stats: bool,
    /// Include information about other peers
    pub include_peers: bool,
}

// Response results

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetChangesResult {
    /// Document ID these changes apply to
    pub document_id: String,
    /// Serialized Automerge changes
    pub changes: Vec<Vec<u8>>,
    /// Sequence numbers for each change
    pub sequences: Vec<u64>,
    /// Whether there are more changes available
    pub has_more: bool,
    /// Current document heads after applying these changes
    pub new_heads: Vec<automerge::ChangeHash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitChangesResult {
    /// Document ID changes were applied to
    pub document_id: String,
    /// Sequence numbers assigned to the submitted changes
    pub assigned_sequences: Vec<u64>,
    /// Number of changes that were actually new (not duplicates)
    pub new_changes_count: u32,
    /// Current document heads after applying changes
    pub new_heads: Vec<automerge::ChangeHash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResult {
    /// Timestamp from the request
    pub request_timestamp: chrono::DateTime<chrono::Utc>,
    /// Timestamp when server processed the request
    pub response_timestamp: chrono::DateTime<chrono::Utc>,
    /// Echo back any payload that was sent
    pub payload: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetStatusResult {
    /// Server version/build info
    pub server_version: String,
    /// Protocol versions supported
    pub supported_versions: Vec<u32>,
    /// Server uptime in seconds
    pub uptime_seconds: u64,
    /// Number of connected peers
    pub connected_peers: u32,
    /// Number of documents being tracked
    pub document_count: u64,
    /// Total number of changes stored
    pub total_changes: u64,
    /// Optional detailed statistics
    pub stats: Option<ServerStats>,
    /// Optional peer information
    pub peers: Option<Vec<PeerStatus>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResult {
    /// Error code for programmatic handling
    pub code: ErrorCode,
    /// Human-readable error message
    pub message: String,
    /// Optional additional details
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStats {
    /// Total bytes stored
    pub total_bytes: u64,
    /// Bytes sent since startup
    pub bytes_sent: u64,
    /// Bytes received since startup
    pub bytes_received: u64,
    /// Number of requests processed
    pub requests_processed: u64,
    /// Average request processing time in milliseconds
    pub avg_request_time_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerStatus {
    /// Peer ID
    pub peer_id: String,
    /// Associated actor ID if authenticated
    pub actor_id: Option<String>,
    /// When peer connected
    pub connected_at: chrono::DateTime<chrono::Utc>,
    /// Last activity timestamp
    pub last_activity: chrono::DateTime<chrono::Utc>,
    /// Number of documents this peer is syncing
    pub document_count: u32,
}

/// Error codes for programmatic error handling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum ErrorCode {
    /// Unknown or internal server error
    InternalError = 1000,
    /// Invalid request format or parameters
    InvalidRequest = 1001,
    /// Authentication failed
    AuthenticationFailed = 1002,
    /// Requested resource not found
    NotFound = 1003,
    /// Rate limit exceeded
    RateLimitExceeded = 1004,
    /// Storage quota exceeded
    QuotaExceeded = 1005,
    /// Version not supported
    UnsupportedVersion = 1006,
    /// Request timeout
    Timeout = 1007,
    /// Conflict in optimistic concurrency control
    Conflict = 1008,
    /// Invalid actor/peer mapping
    InvalidActorMapping = 1009,
}

impl ErrorCode {
    /// Convert error code to human-readable string
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::InternalError => "internal_error",
            ErrorCode::InvalidRequest => "invalid_request",
            ErrorCode::AuthenticationFailed => "authentication_failed",
            ErrorCode::NotFound => "not_found",
            ErrorCode::RateLimitExceeded => "rate_limit_exceeded",
            ErrorCode::QuotaExceeded => "quota_exceeded",
            ErrorCode::UnsupportedVersion => "unsupported_version",
            ErrorCode::Timeout => "timeout",
            ErrorCode::Conflict => "conflict",
            ErrorCode::InvalidActorMapping => "invalid_actor_mapping",
        }
    }
}

impl From<ErrorCode> for Error {
    fn from(code: ErrorCode) -> Self {
        Error::Protocol {
            message: format!("Protocol error: {}", code.as_str()),
        }
    }
}

/// Codec for serializing/deserializing eigensync messages
pub struct EigensyncCodec;

impl EigensyncCodec {
    /// Serialize message to bytes with length prefix
    pub fn encode(message: &EigensyncMessage) -> Result<Vec<u8>> {
        let payload = serde_cbor::to_vec(message)?;
        
        if payload.len() > MAX_MESSAGE_SIZE {
            return Err(Error::Protocol {
                message: format!(
                    "Message too large: {} bytes > {} max",
                    payload.len(),
                    MAX_MESSAGE_SIZE
                ),
            });
        }

        let mut result = Vec::with_capacity(4 + payload.len());
        result.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        result.extend_from_slice(&payload);
        Ok(result)
    }

    /// Deserialize message from bytes (assumes length prefix already read)
    pub fn decode(data: &[u8]) -> Result<EigensyncMessage> {
        if data.len() > MAX_MESSAGE_SIZE {
            return Err(Error::Protocol {
                message: format!(
                    "Message too large: {} bytes > {} max",
                    data.len(),
                    MAX_MESSAGE_SIZE
                ),
            });
        }

        let message: EigensyncMessage = serde_cbor::from_slice(data)?;
        
        // Validate version
        if message.version > CURRENT_VERSION {
            return Err(Error::Protocol {
                message: format!(
                    "Unsupported protocol version: {} > {}",
                    message.version,
                    CURRENT_VERSION
                ),
            });
        }

        Ok(message)
    }

    /// Create a request message
    pub fn create_request(request: EigensyncRequest) -> EigensyncMessage {
        EigensyncMessage {
            version: CURRENT_VERSION,
            request_id: uuid::Uuid::new_v4(),
            payload: EigensyncPayload::Request(request),
        }
    }

    /// Create a response message
    pub fn create_response(
        request_id: uuid::Uuid,
        response: EigensyncResponse,
    ) -> EigensyncMessage {
        EigensyncMessage {
            version: CURRENT_VERSION,
            request_id,
            payload: EigensyncPayload::Response(response),
        }
    }

    /// Create an error response
    pub fn create_error_response(
        request_id: uuid::Uuid,
        code: ErrorCode,
        message: String,
    ) -> EigensyncMessage {
        Self::create_response(
            request_id,
            EigensyncResponse::Error(ErrorResult {
                code,
                message,
                details: None,
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codec_roundtrip() {
        let request = EigensyncRequest::Ping(PingParams {
            timestamp: chrono::Utc::now(),
            payload: Some(b"test".to_vec()),
        });
        
        let message = EigensyncCodec::create_request(request);
        let encoded = EigensyncCodec::encode(&message).unwrap();
        
        // Skip the length prefix for decoding
        let decoded = EigensyncCodec::decode(&encoded[4..]).unwrap();
        
        assert_eq!(message.version, decoded.version);
        assert_eq!(message.request_id, decoded.request_id);
    }

    #[test]
    fn test_message_size_limit() {
        let large_payload = vec![0u8; MAX_MESSAGE_SIZE + 1];
        let request = EigensyncRequest::SubmitChanges(SubmitChangesParams {
            document_id: "test".to_string(),
            changes: vec![large_payload],
            actor_id: ActorId(automerge::ActorId::random()),
            expected_sequence: None,
        });
        
        let message = EigensyncCodec::create_request(request);
        let result = EigensyncCodec::encode(&message);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Message too large"));
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(ErrorCode::InternalError.as_str(), "internal_error");
        assert_eq!(ErrorCode::AuthenticationFailed.as_str(), "authentication_failed");
        assert_eq!(ErrorCode::NotFound.as_str(), "not_found");
    }
} 