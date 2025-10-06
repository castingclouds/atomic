//! Attribution module for tracking AI-assisted patches in Atomic VCS
//!
//! This module extends Atomic's patch-based architecture to include attribution
//! as first-class patch metadata, enabling tracking of AI contributions while
//! maintaining the mathematical properties of commutative patches.

use crate::pristine::{Base32, NodeId, Hash, TxnErr, L64};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

// Submodules
pub mod apply_integration;
pub mod detection;
pub mod remote_integration;
pub mod sanakirja_impl;
pub mod sync;
pub mod tables;

// Re-exports
pub use apply_integration::{
    helpers, ApplyAttributionContext, ApplyIntegrationConfig, ApplyIntegrationError,
    SerializedAttribution,
};
pub use detection::{env_vars, AIProviderInfo, AttributionContext, AttributionDetector};
pub use sanakirja_impl::AttributionStore as SanakirjaAttributionStore;
pub use sync::{
    AttributedPatchBundle, AttributionConflictDetector, AttributionProtocol, AttributionRemoteSync,
    AttributionSyncManager, AttributionSyncState, PatchSignature, ProtocolFeature,
    RemoteAttributionStats, SignatureAlgorithm,
};
pub use tables::{
    queries, AttributionMutTxnT, AttributionStore, AttributionTxnT, ConflictResolutionStrategy,
};

/// Core attribution information for a patch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributedPatch {
    /// Unique identifier for this patch
    pub patch_id: PatchId,
    /// Author information (human or AI)
    pub author: AuthorInfo,
    /// When patch was created
    pub timestamp: DateTime<Utc>,
    /// Whether this patch was AI-assisted
    pub ai_assisted: bool,
    /// Optional AI metadata if AI was involved
    pub ai_metadata: Option<AIMetadata>,
    /// Set of patches this patch depends on
    pub dependencies: HashSet<PatchId>,
    /// Set of patches this semantically conflicts with
    pub conflicts_with: HashSet<PatchId>,
    /// Human-readable description of the patch
    pub description: String,
    /// Confidence score for AI-generated patches (0.0 to 1.0)
    pub confidence: Option<f64>,
}

/// Unique identifier for patches with attribution
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PatchId(pub NodeId);

impl std::fmt::Display for PatchId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_base32())
    }
}

impl PatchId {
    pub fn new(change_id: NodeId) -> Self {
        PatchId(change_id)
    }

    pub fn to_base32(&self) -> String {
        self.0.to_base32()
    }

    pub fn from_base32(s: &str) -> Option<Self> {
        NodeId::from_base32(s.as_bytes()).map(PatchId)
    }
}

impl From<NodeId> for PatchId {
    fn from(id: NodeId) -> Self {
        PatchId(id)
    }
}

impl From<PatchId> for NodeId {
    fn from(id: PatchId) -> Self {
        id.0
    }
}

/// Author information for attribution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AuthorInfo {
    /// Unique identifier for the author
    pub id: AuthorId,
    /// Display name (human name or AI model name)
    pub name: String,
    /// Email or identifier
    pub email: String,
    /// Whether this is an AI author
    pub is_ai: bool,
}

/// Unique identifier for authors (humans or AI)
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AuthorId(pub L64);

impl std::fmt::Display for AuthorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_base32())
    }
}

impl AuthorId {
    pub fn new(id: u64) -> Self {
        AuthorId(L64(id))
    }

    pub fn to_base32(&self) -> String {
        let mut b = [0; 8];
        self.0.to_slice_le(&mut b);
        crate::pristine::BASE32.encode(&b)
    }
}

/// Metadata specific to AI-generated or AI-assisted patches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIMetadata {
    /// AI provider (e.g., "openai", "anthropic", "github")
    pub provider: String,
    /// Model identifier (e.g., "gpt-4", "claude-3")
    pub model: String,
    /// Privacy-preserving hash of the prompt
    pub prompt_hash: Hash,
    /// Type of AI suggestion
    pub suggestion_type: SuggestionType,
    /// Time taken for human review (if applicable)
    pub human_review_time: Option<Duration>,
    /// Confidence score for acceptance (0.0 to 1.0)
    pub acceptance_confidence: f64,
    /// Timestamp when AI generated the suggestion
    pub generation_timestamp: DateTime<Utc>,
    /// Optional tokens/cost tracking
    pub token_count: Option<u32>,
    /// Model parameters used (temperature, etc.)
    pub model_params: Option<ModelParameters>,
}

/// Type of AI suggestion or collaboration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SuggestionType {
    /// AI generated the entire patch
    Complete,
    /// AI suggested, human modified
    Partial,
    /// Human started, AI completed
    Collaborative,
    /// Human wrote based on AI suggestion
    Inspired,
    /// AI reviewed human code
    Review,
    /// AI refactored existing code
    Refactor,
}

/// Model parameters used for generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelParameters {
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f64>,
    pub frequency_penalty: Option<f64>,
    pub presence_penalty: Option<f64>,
    /// Additional provider-specific parameters
    pub custom: HashMap<String, serde_json::Value>,
}

/// Attribution weight for dependency relationships
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct AttributionWeight {
    /// How much of the dependent patch came from the dependency (0.0 to 1.0)
    pub weight: f64,
    /// Type of dependency relationship
    pub dependency_type: DependencyType,
}

/// Type of dependency between patches
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DependencyType {
    /// Direct code dependency
    Direct,
    /// Semantic dependency (relies on functionality)
    Semantic,
    /// Inspired by but not directly dependent
    Inspired,
    /// Conflict resolution dependency
    ConflictResolution,
}

/// Statistics for tracking attribution over time
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AttributionStats {
    /// Total number of patches by this author
    pub total_patches: u64,
    /// Number of AI-assisted patches
    pub ai_assisted_patches: u64,
    /// Number of purely human patches
    pub human_patches: u64,
    /// Average confidence score for AI patches
    pub average_ai_confidence: f64,
    /// Total lines of code contributed
    pub total_lines: u64,
    /// Lines contributed with AI assistance
    pub ai_assisted_lines: u64,
    /// Breakdown by suggestion type
    pub suggestion_types: HashMap<SuggestionType, u64>,
    /// Breakdown by AI provider
    pub provider_breakdown: HashMap<String, u64>,
}

impl AttributionStats {
    pub fn new() -> Self {
        AttributionStats {
            total_patches: 0,
            ai_assisted_patches: 0,
            human_patches: 0,
            average_ai_confidence: 0.0,
            total_lines: 0,
            ai_assisted_lines: 0,
            suggestion_types: HashMap::new(),
            provider_breakdown: HashMap::new(),
        }
    }

    pub fn update(&mut self, patch: &AttributedPatch, lines_changed: u64) {
        self.total_patches += 1;
        self.total_lines += lines_changed;

        if patch.ai_assisted {
            self.ai_assisted_patches += 1;
            self.ai_assisted_lines += lines_changed;

            if let Some(ref metadata) = patch.ai_metadata {
                *self
                    .suggestion_types
                    .entry(metadata.suggestion_type)
                    .or_insert(0) += 1;
                *self
                    .provider_breakdown
                    .entry(metadata.provider.clone())
                    .or_insert(0) += 1;

                // Update average confidence
                let n = self.ai_assisted_patches as f64;
                self.average_ai_confidence =
                    (self.average_ai_confidence * (n - 1.0) + metadata.acceptance_confidence) / n;
            }
        } else {
            self.human_patches += 1;
        }
    }
}

/// Factory for creating attributed patches
pub struct AttributedPatchFactory {
    /// Default author for patches
    default_author: AuthorInfo,
    /// AI configuration if enabled
    ai_config: Option<AIConfig>,
}

/// Configuration for AI assistance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIConfig {
    pub provider: String,
    pub model: String,
    pub default_params: ModelParameters,
    pub enabled: bool,
}

impl AttributedPatchFactory {
    /// Create a new factory with the given author
    pub fn new(author: AuthorInfo) -> Self {
        AttributedPatchFactory {
            default_author: author,
            ai_config: None,
        }
    }

    /// Set AI configuration
    pub fn with_ai_config(mut self, config: AIConfig) -> Self {
        self.ai_config = Some(config);
        self
    }

    /// Create a human-authored patch
    pub fn create_human_patch(
        &self,
        patch_id: PatchId,
        description: String,
        dependencies: HashSet<PatchId>,
    ) -> AttributedPatch {
        AttributedPatch {
            patch_id,
            author: self.default_author.clone(),
            timestamp: Utc::now(),
            ai_assisted: false,
            ai_metadata: None,
            dependencies,
            conflicts_with: HashSet::new(),
            description,
            confidence: None,
        }
    }

    /// Create an AI-assisted patch
    pub fn create_ai_patch(
        &self,
        patch_id: PatchId,
        description: String,
        dependencies: HashSet<PatchId>,
        prompt_id: String,
        suggestion_type: SuggestionType,
        confidence: f64,
    ) -> AttributedPatch {
        // Create a hash from the prompt ID string
        let mut hasher = crate::pristine::Hasher::default();
        hasher.update(prompt_id.as_bytes());
        let prompt_hash = hasher.finish();

        let ai_metadata = self.ai_config.as_ref().map(|config| AIMetadata {
            provider: config.provider.clone(),
            model: config.model.clone(),
            prompt_hash,
            suggestion_type,
            human_review_time: None,
            acceptance_confidence: confidence,
            generation_timestamp: Utc::now(),
            token_count: None,
            model_params: Some(config.default_params.clone()),
        });

        AttributedPatch {
            patch_id,
            author: self.default_author.clone(),
            timestamp: Utc::now(),
            ai_assisted: true,
            ai_metadata,
            dependencies,
            conflicts_with: HashSet::new(),
            description,
            confidence: Some(confidence),
        }
    }
}

/// Trait for types that can provide attribution
pub trait Attributable {
    /// Get the attribution information
    fn attribution(&self) -> Option<&AttributedPatch>;

    /// Get mutable attribution information
    fn attribution_mut(&mut self) -> Option<&mut AttributedPatch>;

    /// Check if this is AI-assisted
    fn is_ai_assisted(&self) -> bool {
        self.attribution().map(|a| a.ai_assisted).unwrap_or(false)
    }

    /// Get the author information
    fn author(&self) -> Option<&AuthorInfo> {
        self.attribution().map(|a| &a.author)
    }
}

/// Error types for attribution operations
#[derive(Debug, thiserror::Error)]
pub enum AttributionError {
    #[error("Patch not found: {0}")]
    PatchNotFound(PatchId),

    #[error("Author not found: {0:?}")]
    AuthorNotFound(AuthorId),

    #[error("Invalid attribution weight: {0}")]
    InvalidWeight(f64),

    #[error("Circular dependency detected")]
    CircularDependency,

    #[error("Database error: {0}")]
    Database(String),

    #[error("Serialization error: {0}")]
    Serialization(String),
}

impl From<bincode::Error> for AttributionError {
    fn from(e: bincode::Error) -> Self {
        AttributionError::Serialization(e.to_string())
    }
}

impl From<serde_json::Error> for AttributionError {
    fn from(e: serde_json::Error) -> Self {
        AttributionError::Serialization(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_patch_id_conversion() {
        let change_id = NodeId::ROOT;
        let patch_id = PatchId::from(change_id);
        assert_eq!(NodeId::from(patch_id), change_id);
    }

    #[test]
    fn test_attribution_stats() {
        let mut stats = AttributionStats::new();

        let author = AuthorInfo {
            id: AuthorId::new(1),
            name: "Test Author".to_string(),
            email: "test@example.com".to_string(),
            is_ai: false,
        };

        let factory = AttributedPatchFactory::new(author);
        let patch = factory.create_human_patch(
            PatchId::new(NodeId::ROOT),
            "Test patch".to_string(),
            HashSet::new(),
        );

        stats.update(&patch, 100);

        assert_eq!(stats.total_patches, 1);
        assert_eq!(stats.human_patches, 1);
        assert_eq!(stats.ai_assisted_patches, 0);
        assert_eq!(stats.total_lines, 100);
    }

    #[test]
    fn test_ai_patch_creation() {
        let author = AuthorInfo {
            id: AuthorId::new(1),
            name: "Test Author".to_string(),
            email: "test@example.com".to_string(),
            is_ai: false,
        };

        let ai_config = AIConfig {
            provider: "openai".to_string(),
            model: "gpt-4".to_string(),
            default_params: ModelParameters {
                temperature: Some(0.7),
                max_tokens: Some(1000),
                top_p: None,
                frequency_penalty: None,
                presence_penalty: None,
                custom: HashMap::new(),
            },
            enabled: true,
        };

        let factory = AttributedPatchFactory::new(author).with_ai_config(ai_config);

        let patch = factory.create_ai_patch(
            PatchId::new(NodeId::ROOT),
            "AI-assisted patch".to_string(),
            HashSet::new(),
            "test_prompt_123".to_string(),
            SuggestionType::Collaborative,
            0.85,
        );

        assert!(patch.ai_assisted);
        assert!(patch.ai_metadata.is_some());
        assert_eq!(patch.confidence, Some(0.85));
    }
}

/// Integration helpers for working with existing Atomic structures
pub mod integration {
    use super::*;
    use crate::change::{Author, ChangeHeader, LocalChange};

    /// Convert a LocalChange to an AttributedPatch with a pre-computed NodeId
    /// Note: The NodeId should be obtained from the transaction using make_changeid
    pub fn local_change_to_attributed_patch<H>(
        change: &LocalChange<H, Author>,
        change_id: NodeId,
        author: AuthorInfo,
        ai_metadata: Option<AIMetadata>,
    ) -> AttributedPatch {
        let patch_id = PatchId::from(change_id);

        AttributedPatch {
            patch_id,
            author,
            timestamp: change.hashed.header.timestamp,
            ai_assisted: ai_metadata.is_some(),
            ai_metadata,
            dependencies: HashSet::new(), // Would need to be populated from transaction
            conflicts_with: HashSet::new(),
            description: change.hashed.header.message.clone(),
            confidence: None,
        }
    }

    /// Extract attribution metadata from a change header
    pub fn extract_attribution_from_header(header: &ChangeHeader) -> Option<AuthorInfo> {
        header.authors.first().map(|author| {
            // The Author type is a BTreeMap<String, String>
            let name = author.0.get("name").cloned().unwrap_or_default();
            let email = author.0.get("email").cloned().unwrap_or_default();

            AuthorInfo {
                id: AuthorId::new(0), // Would need proper ID generation
                name,
                email,
                is_ai: false,
            }
        })
    }

    /// Check if a change description indicates AI assistance
    pub fn detect_ai_assistance(description: &str) -> bool {
        let ai_indicators = [
            "ai-generated",
            "ai-assisted",
            "copilot",
            "gpt",
            "claude",
            "suggested by ai",
            "auto-generated",
        ];

        let lower = description.to_lowercase();
        ai_indicators
            .iter()
            .any(|indicator| lower.contains(indicator))
    }
}

/// Batch operations for attribution
pub struct AttributionBatch {
    patches: Vec<AttributedPatch>,
    stats: HashMap<AuthorId, AttributionStats>,
}

impl AttributionBatch {
    pub fn new() -> Self {
        AttributionBatch {
            patches: Vec::new(),
            stats: HashMap::new(),
        }
    }

    pub fn add(&mut self, patch: AttributedPatch, lines_changed: u64) {
        let author_id = patch.author.id;
        let stats = self
            .stats
            .entry(author_id)
            .or_insert_with(AttributionStats::new);
        stats.update(&patch, lines_changed);
        self.patches.push(patch);
    }

    pub fn commit<T: AttributionMutTxnT>(
        self,
        txn: &mut T,
    ) -> Result<(), TxnErr<<T as crate::pristine::GraphTxnT>::GraphError>> {
        // Store all patches
        for patch in self.patches {
            txn.put_attribution(&patch)?;
            txn.add_author_patch(&patch.author.id, &patch.patch_id)?;

            if let Some(ref metadata) = patch.ai_metadata {
                txn.put_ai_metadata(&patch.patch_id, metadata)?;
            }
        }

        // Update statistics
        for (author_id, stats) in self.stats {
            txn.update_author_stats(&author_id, &stats)?;
        }

        Ok(())
    }
}
