//! Apply integration for AI attribution system
//!
//! This module provides integration hooks for the apply system to preserve
//! and manage attribution metadata during patch application. It follows
//! a simplified approach that works with the existing apply functions
//! without overriding their type signatures.

use super::{
    sanakirja_impl::AttributionStore as SanakirjaAttributionStore, AIMetadata, AttributedPatch,
    AttributionError, AuthorId, AuthorInfo, PatchId, SuggestionType,
};
use crate::change::Change;
use crate::pristine::{sanakirja::Pristine, Base32, NodeId, Hash};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

/// Errors specific to apply integration
#[derive(Debug, Error)]
pub enum ApplyIntegrationError {
    #[error("Failed to extract attribution from change: {0}")]
    ExtractionFailed(String),

    #[error("Failed to store attribution: {0}")]
    StorageFailed(String),

    #[error("Attribution conflict detected: {0}")]
    ConflictDetected(String),

    #[error("General attribution error: {0}")]
    Attribution(#[from] AttributionError),
}

/// Configuration for apply integration
#[derive(Debug, Clone)]
pub struct ApplyIntegrationConfig {
    /// Whether attribution tracking is enabled
    pub enabled: bool,
    /// Whether to auto-detect AI assistance from commit messages
    pub auto_detect_ai: bool,
    /// Whether to validate attribution chains
    pub validate_chains: bool,
    /// Default author for patches without clear attribution
    pub default_author: AuthorInfo,
}

impl Default for ApplyIntegrationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_detect_ai: true,
            validate_chains: true,
            default_author: AuthorInfo {
                id: AuthorId::new(0),
                name: "Unknown User".to_string(),
                email: "unknown@localhost".to_string(),
                is_ai: false,
            },
        }
    }
}

/// Attribution context for apply operations
pub struct ApplyAttributionContext {
    config: ApplyIntegrationConfig,
    attribution_cache: HashMap<PatchId, AttributedPatch>,
    attribution_store: Option<SanakirjaAttributionStore>,
}

impl ApplyAttributionContext {
    /// Create a new apply attribution context without database persistence
    pub fn new(config: ApplyIntegrationConfig) -> Self {
        Self {
            config,
            attribution_cache: HashMap::new(),
            attribution_store: None,
        }
    }

    /// Create a new apply attribution context with database persistence
    pub fn with_database(
        config: ApplyIntegrationConfig,
        pristine: Pristine,
    ) -> Result<Self, ApplyIntegrationError> {
        let store = SanakirjaAttributionStore::new(pristine);

        // Initialize attribution tables if they don't exist
        store.initialize_tables().map_err(|e| {
            ApplyIntegrationError::StorageFailed(format!(
                "Failed to initialize attribution tables: {}",
                e
            ))
        })?;

        Ok(Self {
            config,
            attribution_cache: HashMap::new(),
            attribution_store: Some(store),
        })
    }

    /// Hook called before applying a change
    pub fn pre_apply_hook(
        &mut self,
        change: &Change,
        hash: &Hash,
    ) -> Result<Option<AttributedPatch>, ApplyIntegrationError> {
        if !self.config.enabled {
            return Ok(None);
        }

        // Create patch ID from hash using Base32 decoding
        let patch_id = if let Some(change_id) = NodeId::from_base32(hash.to_base32().as_bytes()) {
            PatchId::from(change_id)
        } else {
            PatchId::from(NodeId::ROOT)
        };

        // Try to extract attribution from change metadata
        if let Some(attributed_patch) = self.extract_attribution_from_change(change, hash)? {
            self.attribution_cache
                .insert(patch_id, attributed_patch.clone());
            return Ok(Some(attributed_patch));
        }

        // Create default attribution
        let attributed_patch = self.create_default_attribution(change, hash);
        self.attribution_cache
            .insert(patch_id, attributed_patch.clone());
        Ok(Some(attributed_patch))
    }

    /// Hook called after successfully applying a change
    pub fn post_apply_hook(
        &mut self,
        patch_id: &PatchId,
        _result: &(u64, crate::pristine::Merkle),
    ) -> Result<(), ApplyIntegrationError> {
        if !self.config.enabled {
            return Ok(());
        }

        if let Some(attributed_patch) = self.attribution_cache.get(patch_id) {
            debug!(
                "Successfully applied attributed patch: {} by {} (AI: {})",
                attributed_patch.patch_id,
                attributed_patch.author.name,
                attributed_patch.ai_assisted
            );

            // Persist attribution to database if store is available
            if let Some(ref store) = self.attribution_store {
                store.put_attribution(attributed_patch).map_err(|e| {
                    ApplyIntegrationError::StorageFailed(format!(
                        "Failed to persist attribution: {}",
                        e
                    ))
                })?;

                debug!(
                    "Successfully persisted attribution for patch {} to database",
                    attributed_patch.patch_id
                );
            } else {
                debug!("Attribution store not available, skipping database persistence");
            }
        }

        Ok(())
    }

    /// Get attribution for a patch (from cache or database)
    pub fn get_attribution(&self, patch_id: &PatchId) -> Option<&AttributedPatch> {
        // First check cache
        if let Some(attribution) = self.attribution_cache.get(patch_id) {
            return Some(attribution);
        }

        None
    }

    /// Get attribution from database if store is available
    pub fn get_attribution_from_database(
        &self,
        patch_id: &PatchId,
    ) -> Result<Option<AttributedPatch>, ApplyIntegrationError> {
        if let Some(ref store) = self.attribution_store {
            store
                .get_attribution(patch_id)
                .map_err(|e| {
                    ApplyIntegrationError::StorageFailed(format!(
                        "Failed to retrieve attribution from database: {}",
                        e
                    ))
                })
                .map(|opt| opt)
        } else {
            Ok(None)
        }
    }

    /// Get all AI-assisted patches from database
    pub fn get_ai_patches(&self) -> Result<Vec<PatchId>, ApplyIntegrationError> {
        if let Some(ref store) = self.attribution_store {
            store.get_ai_patches().map_err(|e| {
                ApplyIntegrationError::StorageFailed(format!(
                    "Failed to retrieve AI patches from database: {}",
                    e
                ))
            })
        } else {
            Ok(Vec::new())
        }
    }

    /// Clear the attribution cache
    pub fn clear_cache(&mut self) {
        self.attribution_cache.clear();
    }

    /// Get the number of cached attributions
    pub fn cache_size(&self) -> usize {
        self.attribution_cache.len()
    }

    /// Check if database persistence is enabled
    pub fn has_database(&self) -> bool {
        self.attribution_store.is_some()
    }

    /// Extract attribution from change metadata
    fn extract_attribution_from_change(
        &self,
        change: &Change,
        hash: &Hash,
    ) -> Result<Option<AttributedPatch>, ApplyIntegrationError> {
        // Check if change metadata contains attribution information
        if !change.hashed.metadata.is_empty() {
            if let Ok(attribution_data) =
                bincode::deserialize::<SerializedAttribution>(&change.hashed.metadata)
            {
                return Ok(Some(self.create_attributed_patch_from_serialized(
                    hash,
                    change,
                    attribution_data,
                )?));
            }
        }

        // Auto-detect AI assistance if enabled
        if self.config.auto_detect_ai {
            if let Some(ai_metadata) = self.detect_ai_from_change(change) {
                return Ok(Some(self.create_ai_attributed_patch(
                    hash,
                    change,
                    ai_metadata,
                )?));
            }
        }

        Ok(None)
    }

    /// Create attributed patch from serialized metadata
    fn create_attributed_patch_from_serialized(
        &self,
        hash: &Hash,
        change: &Change,
        metadata: SerializedAttribution,
    ) -> Result<AttributedPatch, ApplyIntegrationError> {
        let patch_id = if let Some(change_id) = NodeId::from_base32(hash.to_base32().as_bytes()) {
            PatchId::from(change_id)
        } else {
            PatchId::from(NodeId::ROOT)
        };

        let author = metadata
            .author
            .unwrap_or_else(|| self.config.default_author.clone());

        Ok(AttributedPatch {
            patch_id,
            author,
            timestamp: change.hashed.header.timestamp,
            ai_assisted: metadata.ai_assisted,
            ai_metadata: metadata.ai_metadata,
            dependencies: change
                .hashed
                .dependencies
                .iter()
                .filter_map(|h| NodeId::from_base32(h.to_base32().as_bytes()).map(PatchId::from))
                .collect(),
            conflicts_with: HashSet::new(),
            description: change.hashed.header.message.clone(),
            confidence: metadata.confidence,
        })
    }

    /// Auto-detect AI assistance from change patterns
    fn detect_ai_from_change(&self, change: &Change) -> Option<AIMetadata> {
        let message = &change.hashed.header.message;
        let description = change.hashed.header.description.as_deref().unwrap_or("");

        // Look for common AI assistant indicators
        let ai_indicators = [
            "ai-assisted",
            "ai-generated",
            "copilot",
            "claude",
            "gpt",
            "chatgpt",
            "ai:",
            "assistant:",
            "auto-generated",
        ];

        let combined_text = format!("{} {}", message, description).to_lowercase();
        if ai_indicators
            .iter()
            .any(|indicator| combined_text.contains(indicator))
        {
            return Some(AIMetadata {
                provider: "auto-detected".to_string(),
                model: "unknown".to_string(),
                prompt_hash: Hash::NONE,
                suggestion_type: SuggestionType::Complete,
                human_review_time: None,
                acceptance_confidence: 0.5,
                generation_timestamp: change.hashed.header.timestamp,
                token_count: None,
                model_params: None,
            });
        }

        None
    }

    /// Create AI-attributed patch from detected metadata
    fn create_ai_attributed_patch(
        &self,
        hash: &Hash,
        change: &Change,
        ai_metadata: AIMetadata,
    ) -> Result<AttributedPatch, ApplyIntegrationError> {
        let patch_id = if let Some(change_id) = NodeId::from_base32(hash.to_base32().as_bytes()) {
            PatchId::from(change_id)
        } else {
            PatchId::from(NodeId::ROOT)
        };

        let author = AuthorInfo {
            id: AuthorId::new(1), // Special ID for auto-detected AI
            name: "AI Assistant (Auto-detected)".to_string(),
            email: "ai@auto-detected.local".to_string(),
            is_ai: true,
        };

        Ok(AttributedPatch {
            patch_id,
            author,
            timestamp: change.hashed.header.timestamp,
            ai_assisted: true,
            ai_metadata: Some(ai_metadata),
            dependencies: change
                .hashed
                .dependencies
                .iter()
                .filter_map(|h| NodeId::from_base32(h.to_base32().as_bytes()).map(PatchId::from))
                .collect(),
            conflicts_with: HashSet::new(),
            description: change.hashed.header.message.clone(),
            confidence: Some(0.6), // Default confidence for auto-detected AI
        })
    }

    /// Create default attribution for patches without explicit attribution
    fn create_default_attribution(&self, change: &Change, hash: &Hash) -> AttributedPatch {
        let patch_id = if let Some(change_id) = NodeId::from_base32(hash.to_base32().as_bytes()) {
            PatchId::from(change_id)
        } else {
            PatchId::from(NodeId::ROOT)
        };

        // Try to use first author from change
        let author = if let Some(first_author) = change.hashed.header.authors.first() {
            self.author_from_change_author(first_author)
        } else {
            self.config.default_author.clone()
        };

        AttributedPatch {
            patch_id,
            author,
            timestamp: change.hashed.header.timestamp,
            ai_assisted: false,
            ai_metadata: None,
            dependencies: change
                .hashed
                .dependencies
                .iter()
                .filter_map(|h| NodeId::from_base32(h.to_base32().as_bytes()).map(PatchId::from))
                .collect(),
            conflicts_with: HashSet::new(),
            description: change.hashed.header.message.clone(),
            confidence: None,
        }
    }

    /// Convert change::Author to AuthorInfo
    fn author_from_change_author(&self, change_author: &crate::change::Author) -> AuthorInfo {
        // The change::Author is a BTreeMap<String, String>
        let display_name = change_author
            .0
            .get("display_name")
            .or_else(|| change_author.0.get("name"))
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string());

        let email = change_author
            .0
            .get("email")
            .cloned()
            .unwrap_or_else(|| "unknown@localhost".to_string());

        AuthorInfo {
            id: AuthorId::new(0), // Would need proper ID generation in real implementation
            name: display_name,
            email,
            is_ai: false,
        }
    }

    /// Get attribution statistics
    pub fn get_attribution_stats(&self) -> AttributionStats {
        let total_patches = self.attribution_cache.len();
        let ai_patches = self
            .attribution_cache
            .values()
            .filter(|p| p.ai_assisted)
            .count();
        let human_patches = total_patches - ai_patches;

        let average_ai_confidence = if ai_patches > 0 {
            let confidence_sum: f64 = self
                .attribution_cache
                .values()
                .filter(|p| p.ai_assisted)
                .filter_map(|p| p.confidence)
                .sum();
            confidence_sum / ai_patches as f64
        } else {
            0.0
        };

        let mut suggestion_types = HashMap::new();
        let mut provider_breakdown = HashMap::new();

        for patch in self.attribution_cache.values() {
            if let Some(ai_metadata) = &patch.ai_metadata {
                *suggestion_types
                    .entry(ai_metadata.suggestion_type.clone())
                    .or_insert(0) += 1;
                *provider_breakdown
                    .entry(ai_metadata.provider.clone())
                    .or_insert(0) += 1;
            }
        }

        AttributionStats {
            total_patches,
            ai_assisted_patches: ai_patches,
            human_patches,
            average_ai_confidence,
            total_lines: 0, // Would need line counting in real implementation
            ai_assisted_lines: 0,
            suggestion_types,
            provider_breakdown,
        }
    }
}

/// Serializable attribution metadata for embedding in change metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedAttribution {
    pub author: Option<AuthorInfo>,
    pub ai_assisted: bool,
    pub ai_metadata: Option<AIMetadata>,
    pub confidence: Option<f64>,
    pub attribution_version: u32,
}

/// Statistics for attribution tracking during apply operations
#[derive(Debug, Clone)]
pub struct AttributionStats {
    pub total_patches: usize,
    pub ai_assisted_patches: usize,
    pub human_patches: usize,
    pub average_ai_confidence: f64,
    pub total_lines: usize,
    pub ai_assisted_lines: usize,
    pub suggestion_types: HashMap<SuggestionType, usize>,
    pub provider_breakdown: HashMap<String, usize>,
}

/// Helper functions for integration with existing apply functions
pub mod helpers {
    use super::*;

    /// Create attribution metadata from environment variables
    pub fn create_attribution_from_env() -> Option<SerializedAttribution> {
        use std::env;

        let ai_enabled = env::var("ATOMIC_AI_ENABLED")
            .unwrap_or_else(|_| "false".to_string())
            .parse::<bool>()
            .unwrap_or(false);

        if !ai_enabled {
            return None;
        }

        let provider = env::var("ATOMIC_AI_PROVIDER").unwrap_or_else(|_| "unknown".to_string());
        let model = env::var("ATOMIC_AI_MODEL").unwrap_or_else(|_| "unknown".to_string());

        let suggestion_type = env::var("ATOMIC_AI_SUGGESTION_TYPE")
            .unwrap_or_else(|_| "complete".to_string())
            .parse::<String>()
            .unwrap_or_else(|_| "complete".to_string());

        let suggestion_type_enum = match suggestion_type.as_str() {
            "complete" => SuggestionType::Complete,
            "partial" => SuggestionType::Partial,
            "collaborative" => SuggestionType::Collaborative,
            "inspired" => SuggestionType::Inspired,
            "review" => SuggestionType::Review,
            "refactor" => SuggestionType::Refactor,
            _ => SuggestionType::Complete,
        };

        let ai_metadata = AIMetadata {
            provider,
            model,
            prompt_hash: Hash::NONE,
            suggestion_type: suggestion_type_enum,
            human_review_time: None,
            acceptance_confidence: env::var("ATOMIC_AI_CONFIDENCE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.8),
            generation_timestamp: Utc::now(),
            token_count: env::var("ATOMIC_AI_TOKEN_COUNT")
                .ok()
                .and_then(|s| s.parse().ok()),
            model_params: None,
        };

        Some(SerializedAttribution {
            author: None, // Will be filled from change author
            ai_assisted: true,
            ai_metadata: Some(ai_metadata),
            confidence: Some(0.8),
            attribution_version: 1,
        })
    }

    /// Serialize attribution for embedding in change metadata
    pub fn serialize_attribution_for_metadata(
        attribution: &SerializedAttribution,
    ) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(attribution)
    }

    /// Deserialize attribution from change metadata
    pub fn deserialize_attribution_from_metadata(
        metadata: &[u8],
    ) -> Result<SerializedAttribution, bincode::Error> {
        bincode::deserialize(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::{ChangeHeader_, Hashed};
    use chrono::Utc;

    fn create_test_change() -> Change {
        use crate::change::{Author, LocalChange};
        use std::collections::BTreeMap;

        let mut author_map = BTreeMap::new();
        author_map.insert("display_name".to_string(), "Test User".to_string());
        author_map.insert("email".to_string(), "test@example.com".to_string());

        let header = ChangeHeader_ {
            message: "Test change".to_string(),
            description: None,
            timestamp: Utc::now(),
            authors: vec![Author(author_map)],
        };

        let hashed = Hashed {
            version: 1,
            header,
            dependencies: vec![],
            extra_known: vec![],
            metadata: vec![],
            changes: vec![],
            contents_hash: Hash::NONE,
            tag: None,
        };

        LocalChange {
            offsets: crate::change::Offsets::default(),
            hashed,
            unhashed: None,
            contents: vec![],
        }
    }

    fn create_test_pristine() -> (tempfile::TempDir, crate::pristine::sanakirja::Pristine) {
        let tmp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let mut pristine_path = tmp_dir.path().to_path_buf();
        pristine_path.push("pristine");

        let pristine = crate::pristine::sanakirja::Pristine::new(&pristine_path)
            .expect("Failed to create pristine");

        (tmp_dir, pristine)
    }

    #[test]
    fn test_apply_context_creation_without_database() {
        let config = ApplyIntegrationConfig::default();
        let context = ApplyAttributionContext::new(config);

        assert!(!context.has_database());
        assert_eq!(context.cache_size(), 0);
    }

    #[test]
    fn test_apply_context_creation_with_database() {
        let (_temp_dir, pristine) = create_test_pristine();
        let config = ApplyIntegrationConfig::default();

        let context = ApplyAttributionContext::with_database(config, pristine);
        assert!(context.is_ok());

        let context = context.unwrap();
        assert!(context.has_database());
        assert_eq!(context.cache_size(), 0);
    }

    #[test]
    fn test_database_persistence_in_post_apply_hook() {
        let (_temp_dir, pristine) = create_test_pristine();
        let config = ApplyIntegrationConfig::default();

        let mut context = ApplyAttributionContext::with_database(config, pristine)
            .expect("Failed to create context with database");

        // Create a test change
        let change = create_test_change();
        let hash = Hash::NONE;

        // Run pre-apply hook to populate cache
        let result = context.pre_apply_hook(&change, &hash);
        assert!(result.is_ok());

        // Verify attribution was cached
        let patch_id = PatchId::from(NodeId::ROOT);
        assert!(context.get_attribution(&patch_id).is_some());

        // Run post-apply hook (should persist to database)
        let apply_result = (0u64, crate::pristine::Merkle::zero());
        let result = context.post_apply_hook(&patch_id, &apply_result);
        assert!(result.is_ok());

        // Verify attribution can be retrieved from database
        let db_attribution = context.get_attribution_from_database(&patch_id);
        assert!(db_attribution.is_ok());
        assert!(db_attribution.unwrap().is_some());
    }

    #[test]
    fn test_ai_detection_and_persistence() {
        let (_temp_dir, pristine) = create_test_pristine();
        let mut config = ApplyIntegrationConfig::default();
        config.auto_detect_ai = true;

        let mut context = ApplyAttributionContext::with_database(config, pristine)
            .expect("Failed to create context with database");

        // Create a change with AI indicators
        let mut change = create_test_change();
        change.hashed.header.message = "AI-assisted implementation of new feature".to_string();
        let hash = Hash::NONE;

        // Run pre-apply hook
        let result = context.pre_apply_hook(&change, &hash);
        assert!(result.is_ok());

        // Verify AI assistance was detected
        let patch_id = PatchId::from(NodeId::ROOT);
        let attribution = context.get_attribution(&patch_id);
        assert!(attribution.is_some());
        assert!(attribution.unwrap().ai_assisted);

        // Run post-apply hook to persist
        let apply_result = (0u64, crate::pristine::Merkle::zero());
        let result = context.post_apply_hook(&patch_id, &apply_result);
        assert!(result.is_ok());

        // Verify AI patch can be found in database
        let ai_patches = context.get_ai_patches();
        assert!(ai_patches.is_ok());
        assert!(!ai_patches.unwrap().is_empty());
    }

    #[test]
    fn test_environment_variable_integration() {
        use std::env;

        // Set up environment variables
        env::set_var("ATOMIC_AI_ENABLED", "true");
        env::set_var("ATOMIC_AI_PROVIDER", "openai");
        env::set_var("ATOMIC_AI_MODEL", "gpt-4");
        env::set_var("ATOMIC_AI_CONFIDENCE", "0.9");

        let attribution = helpers::create_attribution_from_env();
        assert!(attribution.is_some());

        let attribution = attribution.unwrap();
        assert!(attribution.ai_assisted);
        assert!(attribution.ai_metadata.is_some());

        let metadata = attribution.ai_metadata.unwrap();
        assert_eq!(metadata.provider, "openai");
        assert_eq!(metadata.model, "gpt-4");
        assert_eq!(metadata.acceptance_confidence, 0.9);

        // Clean up environment variables
        env::remove_var("ATOMIC_AI_ENABLED");
        env::remove_var("ATOMIC_AI_PROVIDER");
        env::remove_var("ATOMIC_AI_MODEL");
        env::remove_var("ATOMIC_AI_CONFIDENCE");
    }

    #[test]
    fn test_cache_management() {
        let config = ApplyIntegrationConfig::default();
        let mut context = ApplyAttributionContext::new(config);

        // Initially empty
        assert_eq!(context.cache_size(), 0);

        // Add some attribution to cache via pre_apply_hook
        let change = create_test_change();
        let hash = Hash::NONE;
        let _ = context.pre_apply_hook(&change, &hash);

        // Should have something in cache now
        assert!(context.cache_size() > 0);

        // Clear cache
        context.clear_cache();
        assert_eq!(context.cache_size(), 0);
    }

    #[test]
    fn test_serialization_helpers() {
        let author = AuthorInfo {
            id: AuthorId::new(1),
            name: "Test Author".to_string(),
            email: "test@example.com".to_string(),
            is_ai: false,
        };

        let attribution = SerializedAttribution {
            author: Some(author),
            ai_assisted: true,
            ai_metadata: None,
            confidence: Some(0.8),
            attribution_version: 1,
        };

        // Test serialization
        let serialized = helpers::serialize_attribution_for_metadata(&attribution);
        assert!(serialized.is_ok());

        // Test deserialization
        let deserialized = helpers::deserialize_attribution_from_metadata(&serialized.unwrap());
        assert!(deserialized.is_ok());

        let deserialized = deserialized.unwrap();
        assert_eq!(deserialized.ai_assisted, attribution.ai_assisted);
        assert_eq!(deserialized.confidence, attribution.confidence);
        assert_eq!(
            deserialized.attribution_version,
            attribution.attribution_version
        );
    }

    #[test]
    fn test_end_to_end_database_persistence() {
        let (_temp_dir, pristine) = create_test_pristine();
        let config = ApplyIntegrationConfig {
            enabled: true,
            auto_detect_ai: true,
            validate_chains: true,
            default_author: AuthorInfo {
                id: AuthorId::new(0),
                name: "Test User".to_string(),
                email: "test@example.com".to_string(),
                is_ai: false,
            },
        };

        // Create context with database
        let mut context = ApplyAttributionContext::with_database(config, pristine.clone())
            .expect("Failed to create context with database");

        // Create a test change with AI indicators
        let mut change = create_test_change();
        change.hashed.header.message = "AI-assisted: Implement new feature with GPT-4".to_string();
        change.hashed.header.description = Some("This change was generated with AI assistance using OpenAI's GPT-4 model for code completion and optimization.".to_string());

        let hash = Hash::NONE;
        let patch_id = PatchId::from(NodeId::ROOT);

        // Step 1: Pre-apply hook (should capture attribution)
        let pre_result = context.pre_apply_hook(&change, &hash);
        assert!(pre_result.is_ok());
        assert!(context.cache_size() > 0);

        // Step 2: Verify attribution in cache
        let cached_description = {
            let cached_attribution = context.get_attribution(&patch_id);
            assert!(cached_attribution.is_some());
            let cached_attribution = cached_attribution.unwrap();
            assert!(cached_attribution.ai_assisted);
            assert!(cached_attribution.ai_metadata.is_some());
            assert_eq!(
                cached_attribution.author.name,
                "AI Assistant (Auto-detected)"
            );
            cached_attribution.description.clone()
        };

        // Step 3: Post-apply hook (should persist to database)
        let apply_result = (0u64, crate::pristine::Merkle::zero());
        let post_result = context.post_apply_hook(&patch_id, &apply_result);
        assert!(post_result.is_ok());

        // Step 4: Verify persistence by retrieving from database
        let db_attribution = context.get_attribution_from_database(&patch_id);
        assert!(db_attribution.is_ok());
        let db_attribution = db_attribution.unwrap();
        assert!(db_attribution.is_some());

        let db_attribution = db_attribution.unwrap();
        assert!(db_attribution.ai_assisted);
        assert!(db_attribution.ai_metadata.is_some());
        assert_eq!(db_attribution.description, cached_description);

        // Step 5: Verify AI metadata persisted correctly
        let ai_metadata = db_attribution.ai_metadata.unwrap();
        assert_eq!(ai_metadata.provider, "auto-detected");
        assert_eq!(ai_metadata.model, "unknown");
        assert_eq!(ai_metadata.suggestion_type, SuggestionType::Complete);
        assert_eq!(ai_metadata.acceptance_confidence, 0.5);

        // Step 6: Verify patch appears in AI patches list
        let ai_patches = context.get_ai_patches();
        assert!(ai_patches.is_ok());
        let ai_patches = ai_patches.unwrap();
        assert!(!ai_patches.is_empty());
        assert!(ai_patches.contains(&patch_id));

        // Step 7: Test creating a new context and retrieving the same data
        let new_context =
            ApplyAttributionContext::with_database(ApplyIntegrationConfig::default(), pristine)
                .expect("Failed to create new context");

        let retrieved_attribution = new_context.get_attribution_from_database(&patch_id);
        assert!(retrieved_attribution.is_ok());
        assert!(retrieved_attribution.unwrap().is_some());

        let retrieved_ai_patches = new_context.get_ai_patches();
        assert!(retrieved_ai_patches.is_ok());
        assert!(!retrieved_ai_patches.unwrap().is_empty());
    }

    #[test]
    fn test_apply_attribution_context_creation() {
        let config = ApplyIntegrationConfig::default();
        let context = ApplyAttributionContext::new(config);
        assert_eq!(context.attribution_cache.len(), 0);
    }

    #[test]
    fn test_ai_detection_from_message() {
        let config = ApplyIntegrationConfig::default();
        let context = ApplyAttributionContext::new(config);

        let mut change = create_test_change();
        change.hashed.header.message = "AI-assisted refactoring of the database layer".to_string();

        let ai_metadata = context.detect_ai_from_change(&change);
        assert!(ai_metadata.is_some());

        let metadata = ai_metadata.unwrap();
        assert_eq!(metadata.provider, "auto-detected");
        assert_eq!(metadata.suggestion_type, SuggestionType::Complete);
    }

    #[test]
    fn test_default_attribution_creation() {
        let config = ApplyIntegrationConfig::default();
        let context = ApplyAttributionContext::new(config);

        let change = create_test_change();
        let hash = Hash::NONE;

        let attributed_patch = context.create_default_attribution(&change, &hash);
        assert!(!attributed_patch.ai_assisted);
        assert_eq!(attributed_patch.author.name, "Test User");
        assert_eq!(attributed_patch.author.email, "test@example.com");
    }

    #[test]
    fn test_attribution_stats() {
        let config = ApplyIntegrationConfig::default();
        let mut context = ApplyAttributionContext::new(config);

        // Add a human patch
        let human_patch = AttributedPatch {
            patch_id: PatchId::from(NodeId::ROOT),
            author: AuthorInfo {
                id: AuthorId::new(0),
                name: "Human User".to_string(),
                email: "human@example.com".to_string(),
                is_ai: false,
            },
            timestamp: Utc::now(),
            ai_assisted: false,
            ai_metadata: None,
            dependencies: HashSet::new(),
            conflicts_with: HashSet::new(),
            description: "Human-written patch".to_string(),
            confidence: None,
        };
        context
            .attribution_cache
            .insert(human_patch.patch_id, human_patch);

        // Add an AI patch
        let ai_patch = AttributedPatch {
            patch_id: PatchId::from(NodeId(crate::pristine::L64(2))),
            author: AuthorInfo {
                id: AuthorId::new(1),
                name: "AI Assistant".to_string(),
                email: "ai@example.com".to_string(),
                is_ai: true,
            },
            timestamp: Utc::now(),
            ai_assisted: true,
            ai_metadata: Some(AIMetadata {
                provider: "test-ai".to_string(),
                model: "test-model".to_string(),
                prompt_hash: Hash::NONE,
                suggestion_type: SuggestionType::Complete,
                human_review_time: None,
                acceptance_confidence: 0.9,
                generation_timestamp: Utc::now(),
                token_count: Some(100),
                model_params: None,
            }),
            dependencies: HashSet::new(),
            conflicts_with: HashSet::new(),
            description: "AI-generated patch".to_string(),
            confidence: Some(0.85),
        };
        context
            .attribution_cache
            .insert(ai_patch.patch_id, ai_patch);

        let stats = context.get_attribution_stats();
        assert_eq!(stats.total_patches, 2);
        assert_eq!(stats.ai_assisted_patches, 1);
        assert_eq!(stats.human_patches, 1);
        assert_eq!(stats.average_ai_confidence, 0.85);
        assert_eq!(
            *stats
                .suggestion_types
                .get(&SuggestionType::Complete)
                .unwrap(),
            1
        );
        assert_eq!(*stats.provider_breakdown.get("test-ai").unwrap(), 1);
    }
}
