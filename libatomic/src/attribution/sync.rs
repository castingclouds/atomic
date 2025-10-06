//! Distributed synchronization for attribution metadata
//!
//! This module handles the synchronization of attribution data across
//! distributed repositories, ensuring attribution travels with patches
//! during pull/push operations.

use super::*;
use crate::pristine::TxnErr;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Remote sync trait for attribution-aware repositories
#[async_trait]
pub trait AttributionRemoteSync: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Pull patches with their attribution metadata
    async fn pull_attributed_patches(
        &mut self,
        from: u64,
        channel: &str,
    ) -> Result<Vec<AttributedPatchBundle>, Self::Error>;

    /// Push patches with their attribution metadata
    async fn push_attributed_patches(
        &mut self,
        patches: Vec<AttributedPatchBundle>,
        channel: &str,
    ) -> Result<(), Self::Error>;

    /// Get remote attribution statistics
    async fn get_remote_attribution_stats(
        &self,
        channel: &str,
    ) -> Result<RemoteAttributionStats, Self::Error>;

    /// Negotiate attribution protocol version
    async fn negotiate_attribution_version(&mut self) -> Result<u32, Self::Error>;
}

/// Bundle containing a patch and its attribution metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributedPatchBundle {
    /// The actual patch/change data
    pub patch_data: Vec<u8>,
    /// Attribution metadata
    pub attribution: AttributedPatch,
    /// Optional signature for verification
    pub signature: Option<PatchSignature>,
}

/// Digital signature for patch attribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchSignature {
    /// Author's public key
    pub public_key: Vec<u8>,
    /// Signature over patch_data + attribution
    pub signature: Vec<u8>,
    /// Signature algorithm used
    pub algorithm: SignatureAlgorithm,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SignatureAlgorithm {
    Ed25519,
    RSA2048,
    RSA4096,
}

/// Statistics about attribution in a remote repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAttributionStats {
    pub total_patches: u64,
    pub ai_assisted_patches: u64,
    pub unique_authors: u64,
    pub unique_ai_providers: HashSet<String>,
    pub last_sync_timestamp: Option<u64>,
}

/// Attribution sync manager
pub struct AttributionSyncManager<T: AttributionTxnT> {
    /// Transaction handle
    txn: T,
    /// Cache of recently synced patches
    sync_cache: HashMap<PatchId, AttributedPatch>,
    /// Protocol version for attribution
    #[allow(dead_code)]
    protocol_version: u32,
}

impl<T: AttributionTxnT> AttributionSyncManager<T> {
    pub fn new(txn: T) -> Self {
        AttributionSyncManager {
            txn,
            sync_cache: HashMap::new(),
            protocol_version: 1,
        }
    }

    /// Prepare patches for push with attribution
    pub fn prepare_push_bundles(
        &self,
        patch_ids: Vec<PatchId>,
    ) -> Result<Vec<AttributedPatchBundle>, TxnErr<<T as crate::pristine::GraphTxnT>::GraphError>>
    {
        let mut bundles = Vec::new();

        for patch_id in patch_ids {
            if let Some(attribution) = self.txn.get_attribution(&patch_id)? {
                // In real implementation, would get actual patch data
                let patch_data = Vec::new(); // Placeholder

                bundles.push(AttributedPatchBundle {
                    patch_data,
                    attribution,
                    signature: None, // Would add signature if configured
                });
            }
        }

        Ok(bundles)
    }

    /// Process pulled bundles and store attribution
    pub fn process_pull_bundles<M: AttributionMutTxnT>(
        &mut self,
        txn: &mut M,
        bundles: Vec<AttributedPatchBundle>,
    ) -> Result<Vec<PatchId>, TxnErr<<M as crate::pristine::GraphTxnT>::GraphError>> {
        let mut processed_ids = Vec::new();

        for bundle in bundles {
            // Verify signature if present
            if let Some(sig) = &bundle.signature {
                if !self.verify_signature(&bundle, sig) {
                    continue; // Skip patches with invalid signatures
                }
            }

            // Store attribution
            txn.put_attribution(&bundle.attribution)?;

            // Add to author's patch list
            txn.add_author_patch(&bundle.attribution.author.id, &bundle.attribution.patch_id)?;

            // Store AI metadata if present
            if let Some(ref ai_meta) = bundle.attribution.ai_metadata {
                txn.put_ai_metadata(&bundle.attribution.patch_id, ai_meta)?;
            }

            // Cache for quick access
            self.sync_cache
                .insert(bundle.attribution.patch_id, bundle.attribution.clone());

            processed_ids.push(bundle.attribution.patch_id);
        }

        Ok(processed_ids)
    }

    /// Verify a patch signature
    fn verify_signature(&self, _bundle: &AttributedPatchBundle, sig: &PatchSignature) -> bool {
        // Placeholder - would implement actual signature verification
        // using the specified algorithm
        match sig.algorithm {
            SignatureAlgorithm::Ed25519 => {
                // Verify Ed25519 signature
                true
            }
            SignatureAlgorithm::RSA2048 | SignatureAlgorithm::RSA4096 => {
                // Verify RSA signature
                true
            }
        }
    }

    /// Merge attribution from multiple sources
    pub fn merge_attributions(
        &self,
        local: &AttributedPatch,
        remote: &AttributedPatch,
    ) -> Result<AttributedPatch, AttributionError> {
        // If patches are identical, use the one with earlier timestamp
        if local.patch_id != remote.patch_id {
            return Err(AttributionError::PatchNotFound(local.patch_id));
        }

        // Merge strategy: combine dependencies and conflicts
        let mut merged = local.clone();

        // Union of dependencies
        merged.dependencies.extend(remote.dependencies.clone());

        // Union of conflicts
        merged.conflicts_with.extend(remote.conflicts_with.clone());

        // Use earlier timestamp
        merged.timestamp = local.timestamp.min(remote.timestamp);

        // If one has AI metadata and the other doesn't, prefer the one with metadata
        if merged.ai_metadata.is_none() && remote.ai_metadata.is_some() {
            merged.ai_metadata = remote.ai_metadata.clone();
            merged.ai_assisted = remote.ai_assisted;
        }

        // Average confidence scores if both exist
        if let (Some(local_conf), Some(remote_conf)) = (local.confidence, remote.confidence) {
            merged.confidence = Some((local_conf + remote_conf) / 2.0);
        }

        Ok(merged)
    }
}

/// Sync state for tracking attribution synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionSyncState {
    /// Last synced patch ID for each remote
    pub last_synced: HashMap<String, PatchId>,
    /// Timestamp of last sync
    pub last_sync_time: HashMap<String, u64>,
    /// Pending attributions to push
    pub pending_push: HashSet<PatchId>,
    /// Protocol versions supported by remotes
    pub remote_versions: HashMap<String, u32>,
}

impl AttributionSyncState {
    pub fn new() -> Self {
        AttributionSyncState {
            last_synced: HashMap::new(),
            last_sync_time: HashMap::new(),
            pending_push: HashSet::new(),
            remote_versions: HashMap::new(),
        }
    }

    pub fn mark_synced(&mut self, remote: String, patch_id: PatchId) {
        self.last_synced.insert(remote.clone(), patch_id);
        self.last_sync_time
            .insert(remote, chrono::Utc::now().timestamp() as u64);
        self.pending_push.remove(&patch_id);
    }

    pub fn add_pending(&mut self, patch_id: PatchId) {
        self.pending_push.insert(patch_id);
    }

    pub fn is_synced(&self, remote: &str, patch_id: &PatchId) -> bool {
        self.last_synced
            .get(remote)
            .map(|last| last >= patch_id)
            .unwrap_or(false)
    }
}

/// Conflict detection for attribution during sync
pub struct AttributionConflictDetector {
    /// Known attribution conflicts
    conflicts: HashMap<PatchId, Vec<AttributionConflict>>,
}

#[derive(Debug, Clone)]
pub struct AttributionConflict {
    pub patch_id: PatchId,
    pub conflict_type: ConflictType,
    pub local_attribution: AttributedPatch,
    pub remote_attribution: AttributedPatch,
}

#[derive(Debug, Clone, Copy)]
pub enum ConflictType {
    /// Different authors claimed for same patch
    AuthorMismatch,
    /// Different AI metadata for same patch
    AIMetadataMismatch,
    /// Dependency graph inconsistency
    DependencyInconsistency,
    /// Timestamp inconsistency
    TimestampInconsistency,
}

impl AttributionConflictDetector {
    pub fn new() -> Self {
        AttributionConflictDetector {
            conflicts: HashMap::new(),
        }
    }

    pub fn detect_conflicts(
        &mut self,
        local: &AttributedPatch,
        remote: &AttributedPatch,
    ) -> Vec<AttributionConflict> {
        let mut conflicts = Vec::new();

        if local.patch_id != remote.patch_id {
            return conflicts;
        }

        // Check author mismatch
        if local.author.id != remote.author.id {
            conflicts.push(AttributionConflict {
                patch_id: local.patch_id,
                conflict_type: ConflictType::AuthorMismatch,
                local_attribution: local.clone(),
                remote_attribution: remote.clone(),
            });
        }

        // Check AI metadata consistency
        if local.ai_assisted != remote.ai_assisted {
            conflicts.push(AttributionConflict {
                patch_id: local.patch_id,
                conflict_type: ConflictType::AIMetadataMismatch,
                local_attribution: local.clone(),
                remote_attribution: remote.clone(),
            });
        }

        // Check timestamp consistency (allowing small differences)
        const TIMESTAMP_TOLERANCE: u64 = 60; // 60 seconds
        if (local.timestamp - remote.timestamp).num_seconds().abs() > TIMESTAMP_TOLERANCE as i64 {
            conflicts.push(AttributionConflict {
                patch_id: local.patch_id,
                conflict_type: ConflictType::TimestampInconsistency,
                local_attribution: local.clone(),
                remote_attribution: remote.clone(),
            });
        }

        // Store conflicts for later resolution
        self.conflicts.insert(local.patch_id, conflicts.clone());

        conflicts
    }

    pub fn resolve_conflict(
        &mut self,
        conflict: &AttributionConflict,
        resolution: ConflictResolution,
    ) -> AttributedPatch {
        match resolution {
            ConflictResolution::KeepLocal => conflict.local_attribution.clone(),
            ConflictResolution::KeepRemote => conflict.remote_attribution.clone(),
            ConflictResolution::Merge => {
                // Implement merging logic
                let mut merged = conflict.local_attribution.clone();

                // Take union of dependencies
                merged
                    .dependencies
                    .extend(conflict.remote_attribution.dependencies.clone());

                // Take union of conflicts
                merged
                    .conflicts_with
                    .extend(conflict.remote_attribution.conflicts_with.clone());

                merged
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ConflictResolution {
    KeepLocal,
    KeepRemote,
    Merge,
}

/// Protocol for attribution synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionProtocol {
    pub version: u32,
    pub features: HashSet<ProtocolFeature>,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolFeature {
    /// Support for AI metadata
    AIMetadata,
    /// Support for patch signatures
    Signatures,
    /// Support for dependency attribution weights
    DependencyWeights,
    /// Support for compressed attribution data
    Compression,
    /// Support for incremental sync
    IncrementalSync,
}

impl AttributionProtocol {
    pub fn new(version: u32) -> Self {
        let mut features = HashSet::new();

        // Version 1 features
        if version >= 1 {
            features.insert(ProtocolFeature::AIMetadata);
            features.insert(ProtocolFeature::DependencyWeights);
        }

        // Version 2 features
        if version >= 2 {
            features.insert(ProtocolFeature::Signatures);
            features.insert(ProtocolFeature::Compression);
        }

        // Version 3 features
        if version >= 3 {
            features.insert(ProtocolFeature::IncrementalSync);
        }

        AttributionProtocol { version, features }
    }

    pub fn is_compatible(&self, other: &AttributionProtocol) -> bool {
        // Check if we have at least one common version
        self.version == other.version || (self.version > 0 && other.version > 0)
    }

    pub fn negotiate(&self, other: &AttributionProtocol) -> AttributionProtocol {
        let version = self.version.min(other.version);
        let features = self
            .features
            .intersection(&other.features)
            .copied()
            .collect();

        AttributionProtocol { version, features }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_negotiation() {
        let proto1 = AttributionProtocol::new(3);
        let proto2 = AttributionProtocol::new(2);

        let negotiated = proto1.negotiate(&proto2);
        assert_eq!(negotiated.version, 2);
        assert!(negotiated.features.contains(&ProtocolFeature::AIMetadata));
        assert!(negotiated.features.contains(&ProtocolFeature::Signatures));
        assert!(!negotiated
            .features
            .contains(&ProtocolFeature::IncrementalSync));
    }

    #[test]
    fn test_sync_state() {
        let mut state = AttributionSyncState::new();
        let patch_id = PatchId::new(NodeId::ROOT);

        state.add_pending(patch_id);
        assert!(state.pending_push.contains(&patch_id));

        state.mark_synced("origin".to_string(), patch_id);
        assert!(!state.pending_push.contains(&patch_id));
        assert!(state.is_synced("origin", &patch_id));
    }
}
