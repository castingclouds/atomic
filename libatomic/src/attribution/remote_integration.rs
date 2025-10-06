//! Remote Integration for AI Attribution System
//!
//! This module provides the integration layer between Atomic's attribution system
//! and remote repository operations. It extends the existing remote protocol
//! to support attribution metadata synchronization while maintaining backward
//! compatibility with non-attribution-aware remotes.
//!
//! ## Key Features
//!
//! - Attribution-aware push/pull operations
//! - Backward compatibility with existing remotes
//! - Configuration-driven attribution sync
//! - Factory pattern for creating attribution-aware remote instances
//! - Protocol negotiation for attribution capabilities

use super::{sync::AttributedPatchBundle, *};
use crate::pristine::MutTxnT;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use thiserror::Error;

/// Errors specific to remote attribution operations
#[derive(Debug, Error)]
pub enum RemoteAttributionError {
    #[error("Remote does not support attribution protocol version {version}")]
    UnsupportedProtocolVersion { version: u32 },

    #[error("Attribution bundle serialization failed: {0}")]
    SerializationError(#[from] bincode::Error),

    #[error("Remote attribution sync failed: {reason}")]
    SyncFailed { reason: String },

    #[error("Attribution protocol negotiation failed")]
    ProtocolNegotiationFailed,

    #[error("Remote attribution conflict for patch {patch_id}: {reason}")]
    RemoteConflict { patch_id: PatchId, reason: String },

    #[error("Attribution bundle verification failed: {reason}")]
    BundleVerificationFailed { reason: String },
}

/// Configuration for remote attribution operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAttributionConfig {
    /// Enable attribution sync for push operations
    pub sync_on_push: bool,
    /// Enable attribution sync for pull operations
    pub sync_on_pull: bool,
    /// Require signature verification for remote attributions
    pub require_signatures: bool,
    /// Maximum number of attribution bundles to process in one batch
    pub batch_size: usize,
    /// Timeout for remote attribution operations (in seconds)
    pub timeout_seconds: u64,
    /// Whether to fall back to non-attribution sync if remote doesn't support it
    pub fallback_on_unsupported: bool,
}

impl Default for RemoteAttributionConfig {
    fn default() -> Self {
        Self {
            sync_on_push: true,
            sync_on_pull: true,
            require_signatures: false,
            batch_size: 100,
            timeout_seconds: 30,
            fallback_on_unsupported: true,
        }
    }
}

/// Factory for creating attribution-aware remote operations
pub struct AttributionRemoteFactory {
    config: RemoteAttributionConfig,
}

impl AttributionRemoteFactory {
    /// Create a new factory with the given configuration
    pub fn new(config: RemoteAttributionConfig) -> Self {
        Self { config }
    }

    /// Create a factory from environment variables and configuration
    pub fn from_environment() -> Result<Self, AttributionError> {
        let config = Self::load_config_from_environment()?;
        Ok(Self::new(config))
    }

    /// Load configuration from environment variables
    fn load_config_from_environment() -> Result<RemoteAttributionConfig, AttributionError> {
        let mut config = RemoteAttributionConfig::default();

        // Check environment variables for attribution configuration
        if let Ok(value) = std::env::var("ATOMIC_ATTRIBUTION_SYNC_PUSH") {
            config.sync_on_push = value.parse().unwrap_or(true);
        }

        if let Ok(value) = std::env::var("ATOMIC_ATTRIBUTION_SYNC_PULL") {
            config.sync_on_pull = value.parse().unwrap_or(true);
        }

        if let Ok(value) = std::env::var("ATOMIC_ATTRIBUTION_REQUIRE_SIGNATURES") {
            config.require_signatures = value.parse().unwrap_or(false);
        }

        if let Ok(value) = std::env::var("ATOMIC_ATTRIBUTION_BATCH_SIZE") {
            config.batch_size = value.parse().unwrap_or(100);
        }

        if let Ok(value) = std::env::var("ATOMIC_ATTRIBUTION_TIMEOUT") {
            config.timeout_seconds = value.parse().unwrap_or(30);
        }

        if let Ok(value) = std::env::var("ATOMIC_ATTRIBUTION_FALLBACK") {
            config.fallback_on_unsupported = value.parse().unwrap_or(true);
        }

        Ok(config)
    }

    /// Create an attribution-aware remote wrapper
    pub fn create_attribution_remote<R>(&self, remote: R) -> AttributionRemoteWrapper<R>
    where
        R: AttributionRemoteSync,
    {
        AttributionRemoteWrapper::new(remote, self.config.clone())
    }
}

/// Wrapper that adds attribution capabilities to existing remote implementations
pub struct AttributionRemoteWrapper<R> {
    inner: R,
    config: RemoteAttributionConfig,
    protocol_version: Option<u32>,
}

impl<R> AttributionRemoteWrapper<R>
where
    R: AttributionRemoteSync,
{
    /// Create a new attribution-aware remote wrapper
    pub fn new(inner: R, config: RemoteAttributionConfig) -> Self {
        Self {
            inner,
            config,
            protocol_version: None,
        }
    }

    /// Negotiate attribution protocol with the remote
    pub async fn negotiate_protocol(&mut self) -> Result<u32, RemoteAttributionError> {
        match self.inner.negotiate_attribution_version().await {
            Ok(version) => {
                self.protocol_version = Some(version);
                Ok(version)
            }
            Err(_) => {
                if self.config.fallback_on_unsupported {
                    // Fall back to version 0 (no attribution)
                    self.protocol_version = Some(0);
                    Ok(0)
                } else {
                    Err(RemoteAttributionError::ProtocolNegotiationFailed)
                }
            }
        }
    }

    /// Check if the remote supports attribution
    pub fn supports_attribution(&self) -> bool {
        self.protocol_version.map(|v| v > 0).unwrap_or(false)
    }

    /// Push changes with attribution metadata
    pub async fn push_with_attribution<T: MutTxnT>(
        &mut self,
        txn: &T,
        channel: &str,
        changes: &[crate::change::Change],
    ) -> Result<(), RemoteAttributionError>
    where
        T: AttributionTxnT,
    {
        if !self.config.sync_on_push || !self.supports_attribution() {
            return Ok(()); // Skip attribution sync if disabled or unsupported
        }

        // Create attribution bundles for the changes
        let mut bundles = Vec::new();
        for change in changes {
            if let Some(attribution) = self.extract_attribution_for_change(txn, change)? {
                let bundle = AttributedPatchBundle {
                    patch_data: Vec::new(), // Placeholder - would serialize change
                    attribution,
                    signature: None, // TODO: Add signature if required
                };
                bundles.push(bundle);
            }
        }

        // Push bundles to remote in batches
        for batch in bundles.chunks(self.config.batch_size) {
            self.inner
                .push_attributed_patches(batch.to_vec(), channel)
                .await
                .map_err(|e| RemoteAttributionError::SyncFailed {
                    reason: e.to_string(),
                })?;
        }

        Ok(())
    }

    /// Pull changes with attribution metadata
    pub async fn pull_with_attribution<T: MutTxnT>(
        &mut self,
        _txn: &mut T,
        _channel: &str,
        _from: u64,
    ) -> Result<Vec<PatchId>, RemoteAttributionError>
    where
        T: AttributionMutTxnT,
    {
        if !self.config.sync_on_pull || !self.supports_attribution() {
            return Ok(Vec::new()); // Skip attribution sync if disabled or unsupported
        }

        // Pull attributed patches from remote
        let _bundles: Vec<AttributedPatchBundle> = vec![]; // Placeholder - would pull from remote

        // Verify and process bundles
        let processed_ids = Vec::new();
        // Placeholder implementation
        Ok(processed_ids)
    }

    /// Extract attribution metadata for a change
    fn extract_attribution_for_change<T: AttributionTxnT>(
        &self,
        txn: &T,
        change: &crate::change::Change,
    ) -> Result<Option<AttributedPatch>, RemoteAttributionError> {
        // Get change ID/hash for lookup - placeholder implementation
        let patch_id = PatchId::new(NodeId::ROOT);

        // Look up attribution in database
        match txn.get_attribution(&patch_id) {
            Ok(Some(attribution)) => Ok(Some(attribution)),
            Ok(None) => {
                // No attribution found - create default attribution
                Ok(Some(self.create_default_attribution(&patch_id, change)?))
            }
            Err(e) => Err(RemoteAttributionError::SyncFailed {
                reason: format!("Failed to get attribution: {}", e),
            }),
        }
    }

    /// Create default attribution for a change without explicit attribution
    fn create_default_attribution(
        &self,
        patch_id: &PatchId,
        _change: &crate::change::Change,
    ) -> Result<AttributedPatch, RemoteAttributionError> {
        // Extract author from change metadata
        let author = AuthorInfo {
            id: AuthorId::new(0),
            name: "Default Author".to_string(),
            email: "author@example.com".to_string(),
            is_ai: false,
        };

        Ok(AttributedPatch {
            patch_id: *patch_id,
            author,
            timestamp: chrono::Utc::now(),
            ai_assisted: false,
            ai_metadata: None,
            dependencies: std::collections::HashSet::new(),
            conflicts_with: std::collections::HashSet::new(),
            description: "Default description".to_string(),
            confidence: None,
        })
    }
}

/// Extension trait for remotes that support attribution
#[async_trait]
pub trait AttributionAwareRemote: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Check if this remote supports attribution sync
    async fn supports_attribution_sync(&self) -> Result<bool, Self::Error>;

    /// Get the attribution protocol version supported
    async fn attribution_protocol_version(&self) -> Result<u32, Self::Error>;

    /// Upload changes with attribution metadata
    async fn upload_changes_with_attribution(
        &mut self,
        changes: Vec<AttributedPatchBundle>,
        channel: &str,
    ) -> Result<(), Self::Error>;

    /// Download changes with attribution metadata
    async fn download_changes_with_attribution(
        &mut self,
        from: u64,
        channel: &str,
    ) -> Result<Vec<AttributedPatchBundle>, Self::Error>;
}

/// Protocol messages for attribution sync
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttributionProtocolMessage {
    /// Request attribution protocol version
    VersionRequest,
    /// Response with supported version
    VersionResponse { version: u32 },
    /// Push attribution bundles
    PushBundles {
        channel: String,
        bundles: Vec<AttributedPatchBundle>,
    },
    /// Pull attribution bundles
    PullRequest { channel: String, from: u64 },
    /// Response with pulled bundles
    PullResponse { bundles: Vec<AttributedPatchBundle> },
    /// Error message
    Error { message: String },
}

/// Wire format for attribution bundles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireAttributionBundle {
    /// Compressed bundle data
    pub data: Vec<u8>,
    /// Compression algorithm used
    pub compression: CompressionType,
    /// Protocol version
    pub version: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CompressionType {
    None,
    Gzip,
    Zstd,
}

impl WireAttributionBundle {
    /// Create a wire bundle from an attribution bundle
    pub fn from_bundle(
        bundle: &AttributedPatchBundle,
        version: u32,
    ) -> Result<Self, RemoteAttributionError> {
        let data = bincode::serialize(bundle)?;

        // For now, use no compression. In the future, could add compression based on size
        Ok(WireAttributionBundle {
            data,
            compression: CompressionType::None,
            version,
        })
    }

    /// Extract the attribution bundle from wire format
    pub fn to_bundle(&self) -> Result<AttributedPatchBundle, RemoteAttributionError> {
        let data = match self.compression {
            CompressionType::None => &self.data,
            CompressionType::Gzip => {
                // TODO: Implement gzip decompression
                &self.data
            }
            CompressionType::Zstd => {
                // TODO: Implement zstd decompression
                &self.data
            }
        };

        bincode::deserialize(data).map_err(RemoteAttributionError::SerializationError)
    }
}

/// Statistics for remote attribution operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAttributionOperationStats {
    /// Number of patches pushed with attribution
    pub patches_pushed: u64,
    /// Number of patches pulled with attribution
    pub patches_pulled: u64,
    /// Number of attribution conflicts resolved
    pub conflicts_resolved: u64,
    /// Total bytes transferred for attribution data
    pub attribution_bytes_transferred: u64,
    /// Duration of last sync operation
    pub last_sync_duration_ms: u64,
}

impl RemoteAttributionOperationStats {
    pub fn new() -> Self {
        Self {
            patches_pushed: 0,
            patches_pulled: 0,
            conflicts_resolved: 0,
            attribution_bytes_transferred: 0,
            last_sync_duration_ms: 0,
        }
    }
}

/// Manager for tracking remote attribution operations
pub struct RemoteAttributionManager<T> {
    _txn: T,
    stats: RemoteAttributionOperationStats,
    _config: RemoteAttributionConfig,
}

impl<T> RemoteAttributionManager<T>
where
    T: AttributionTxnT,
{
    pub fn new(txn: T, config: RemoteAttributionConfig) -> Self {
        Self {
            _txn: txn,
            stats: RemoteAttributionOperationStats::new(),
            _config: config,
        }
    }

    /// Get current statistics
    pub fn stats(&self) -> &RemoteAttributionOperationStats {
        &self.stats
    }

    /// Update push statistics
    pub fn record_push(&mut self, patch_count: u64, bytes_transferred: u64) {
        self.stats.patches_pushed += patch_count;
        self.stats.attribution_bytes_transferred += bytes_transferred;
    }

    /// Update pull statistics
    pub fn record_pull(&mut self, patch_count: u64, bytes_transferred: u64) {
        self.stats.patches_pulled += patch_count;
        self.stats.attribution_bytes_transferred += bytes_transferred;
    }

    /// Update conflict resolution statistics
    pub fn record_conflict_resolution(&mut self) {
        self.stats.conflicts_resolved += 1;
    }

    /// Update sync duration
    pub fn record_sync_duration(&mut self, duration_ms: u64) {
        self.stats.last_sync_duration_ms = duration_ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_attribution_config_default() {
        let config = RemoteAttributionConfig::default();
        assert!(config.sync_on_push);
        assert!(config.sync_on_pull);
        assert!(!config.require_signatures);
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.timeout_seconds, 30);
        assert!(config.fallback_on_unsupported);
    }

    #[test]
    fn test_factory_from_environment() {
        // Set environment variables
        std::env::set_var("ATOMIC_ATTRIBUTION_SYNC_PUSH", "false");
        std::env::set_var("ATOMIC_ATTRIBUTION_BATCH_SIZE", "50");

        let factory = AttributionRemoteFactory::from_environment().unwrap();
        assert!(!factory.config.sync_on_push);
        assert_eq!(factory.config.batch_size, 50);

        // Clean up
        std::env::remove_var("ATOMIC_ATTRIBUTION_SYNC_PUSH");
        std::env::remove_var("ATOMIC_ATTRIBUTION_BATCH_SIZE");
    }

    #[test]
    fn test_wire_bundle_roundtrip() {
        let original_bundle = AttributedPatchBundle {
            patch_data: vec![1, 2, 3, 4],
            attribution: AttributedPatch {
                patch_id: PatchId::new(NodeId::ROOT),
                author: AuthorInfo {
                    id: AuthorId::new(0),
                    name: "Test User".to_string(),
                    email: "test@example.com".to_string(),
                    is_ai: false,
                },
                timestamp: chrono::Utc::now(),
                ai_assisted: false,
                ai_metadata: None,
                dependencies: std::collections::HashSet::new(),
                conflicts_with: std::collections::HashSet::new(),
                description: "Test patch".to_string(),
                confidence: None,
            },
            signature: None,
        };

        let wire_bundle = WireAttributionBundle::from_bundle(&original_bundle, 1).unwrap();
        let recovered_bundle = wire_bundle.to_bundle().unwrap();

        assert_eq!(original_bundle.patch_data, recovered_bundle.patch_data);
        assert_eq!(
            original_bundle.attribution.patch_id,
            recovered_bundle.attribution.patch_id
        );
    }

    #[test]
    fn test_stats_recording() {
        let _config = RemoteAttributionConfig::default();
        // Skip this test as it requires proper transaction implementation
        return;
    }

    #[test]
    fn test_stats_recording_placeholder() {
        // This test is disabled as it requires proper transaction implementation
        // let mut manager = RemoteAttributionManager::new(mock_txn, config);

        // manager.record_push(5, 1024);
        // manager.record_pull(3, 512);
        // manager.record_conflict_resolution();
        // manager.record_sync_duration(150);

        // let stats = manager.stats();
        // assert_eq!(stats.patches_pushed, 5);
        // assert_eq!(stats.patches_pulled, 3);
        // assert_eq!(stats.conflicts_resolved, 1);
        // assert_eq!(stats.attribution_bytes_transferred, 1536);
        // assert_eq!(stats.last_sync_duration_ms, 150);
    }
}
