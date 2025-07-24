//! Automerge document management for client

use crate::types::{Result, ActorId};
use automerge::{AutoCommit, ChangeHash};
use serde_json::Value;
use std::collections::HashMap;

/// Manager for Automerge documents
pub struct DocumentManager {
    documents: HashMap<String, AutoCommit>,
    actor_id: ActorId,
}

impl DocumentManager {
    /// Create a new document manager
    pub fn new(actor_id: ActorId) -> Self {
        tracing::debug!("Creating document manager for actor {}", actor_id);
        
        Self {
            documents: HashMap::new(),
            actor_id,
        }
    }

    /// Get or create a document for a swap
    pub fn get_or_create_document(&mut self, document_id: &str) -> Result<&mut AutoCommit> {
        tracing::debug!("Getting or creating document {}", document_id);
        
        if !self.documents.contains_key(document_id) {
            // TODO: Try to load from cache first
            let mut doc = AutoCommit::new();
            doc.set_actor(self.actor_id.0.clone());
            self.documents.insert(document_id.to_string(), doc);
        }
        
        Ok(self.documents.get_mut(document_id).unwrap())
    }

    /// Append a swap state to a document
    pub fn append_swap_state(
        &mut self,
        document_id: &str,
        state_json: Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<automerge::Change>> {
        tracing::debug!("Appending swap state to document {}", document_id);
        
        let doc = self.get_or_create_document(document_id)?;
        
        // TODO: Implement state appending to Automerge document
        // Structure: { "states": [ { "timestamp": "...", "state": {...} } ] }
        
        // For now, return empty changes
        Ok(vec![])
    }

    /// Get the latest state from a document
    pub fn get_latest_state(&self, document_id: &str) -> Result<Option<Value>> {
        tracing::debug!("Getting latest state from document {}", document_id);
        
        if let Some(doc) = self.documents.get(document_id) {
            // TODO: Implement state retrieval from Automerge document
            // Find entry with latest timestamp
        }
        
        Ok(None)
    }

    /// Apply changes to a document
    pub fn apply_changes(
        &mut self,
        document_id: &str,
        changes: &[automerge::Change],
    ) -> Result<()> {
        tracing::debug!("Applying {} changes to document {}", changes.len(), document_id);
        
        let doc = self.get_or_create_document(document_id)?;
        
        for change in changes {
            doc.load_incremental(change.raw_bytes())?;
        }
        
        Ok(())
    }

    /// Get document heads
    pub fn get_heads(&mut self, document_id: &str) -> Vec<ChangeHash> {
        if let Some(doc) = self.documents.get_mut(document_id) {
            doc.get_heads()
        } else {
            vec![]
        }
    }

    /// Generate changes since given heads
    pub fn changes_since(
        &mut self,
        document_id: &str,
        heads: &[ChangeHash],
    ) -> Result<Vec<automerge::Change>> {
        if let Some(doc) = self.documents.get_mut(document_id) {
            let changes = doc.get_changes(heads);
            Ok(changes.into_iter().cloned().collect())
        } else {
            Ok(vec![])
        }
    }

    /// Serialize a document for storage
    pub fn serialize_document(&mut self, document_id: &str) -> Result<Option<Vec<u8>>> {
        if let Some(doc) = self.documents.get_mut(document_id) {
            let bytes = doc.save();
            Ok(Some(bytes))
        } else {
            Ok(None)
        }
    }

    /// Load a document from serialized data
    pub fn load_document(&mut self, document_id: &str, data: &[u8]) -> Result<()> {
        tracing::debug!("Loading document {} from {} bytes", document_id, data.len());
        
        let mut doc = AutoCommit::load(data)?;
        doc.set_actor(self.actor_id.0.clone());
        self.documents.insert(document_id.to_string(), doc);
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_manager_creation() {
        let actor_id = ActorId(automerge::ActorId::random());
        let _manager = DocumentManager::new(actor_id);
        // Manager creation should not panic
    }

    #[test]
    fn test_get_or_create_document() {
        let actor_id = ActorId(automerge::ActorId::random());
        let mut manager = DocumentManager::new(actor_id);
        
        // First call should create the document
        let _doc1 = manager.get_or_create_document("test-doc").unwrap();
        
        // Second call should return the existing document (test that no panic occurs)
        let _doc2 = manager.get_or_create_document("test-doc").unwrap();
        
        // Document should exist in the manager
        assert!(manager.documents.contains_key("test-doc"));
    }
} 