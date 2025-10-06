//! AI Attribution Detection Module
//!
//! This module implements environment-based AI attribution detection using the
//! factory pattern, following Atomic's architectural guidelines for configuration-driven
//! design and robust error handling.

use super::{AIMetadata, AttributedPatch, AuthorInfo, ModelParameters, PatchId, SuggestionType};
use crate::pristine::Hash;
use chrono::Utc;
use std::collections::HashMap;
use std::env;
use std::time::Duration;

/// Factory for creating attribution contexts from environment variables
/// and configuration settings
pub struct AttributionDetector {
    /// Configuration for AI attribution
    config: AttributionConfig,
    /// Cached environment variables
    env_cache: HashMap<String, String>,
}

/// Configuration for attribution detection
#[derive(Debug, Clone)]
pub struct AttributionConfig {
    /// Whether AI attribution is enabled
    pub enabled: bool,
    /// Default AI provider
    pub default_provider: String,
    /// Default AI model
    pub default_model: String,
    /// Whether to track prompts
    pub track_prompts: bool,
    /// Require explicit confirmation
    pub require_confirmation: bool,
}

/// Context for creating attributed patches
#[derive(Debug, Clone)]
pub struct AttributionContext {
    /// AI provider information
    pub ai_info: Option<AIProviderInfo>,
    /// Author information
    pub author_info: AuthorInfo,
    /// Whether this is an AI-assisted operation
    pub is_ai_assisted: bool,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// AI provider information detected from environment
#[derive(Debug, Clone)]
pub struct AIProviderInfo {
    /// Provider name (e.g., "openai", "anthropic", "github")
    pub provider: String,
    /// Model name (e.g., "gpt-4", "claude-3")
    pub model: String,
    /// Suggestion type
    pub suggestion_type: SuggestionType,
    /// Prompt hash for privacy
    pub prompt_hash: Option<Hash>,
    /// Confidence score if available
    pub confidence: Option<f64>,
    /// Token count if available
    pub token_count: Option<u32>,
    /// Model parameters used
    pub model_params: Option<ModelParameters>,
}

/// Environment variable names for AI attribution detection
pub mod env_vars {
    /// Enable AI attribution tracking
    pub const ATOMIC_AI_ENABLED: &str = "ATOMIC_AI_ENABLED";
    /// AI provider name
    pub const ATOMIC_AI_PROVIDER: &str = "ATOMIC_AI_PROVIDER";
    /// AI model name
    pub const ATOMIC_AI_MODEL: &str = "ATOMIC_AI_MODEL";
    /// AI suggestion type
    pub const ATOMIC_AI_SUGGESTION_TYPE: &str = "ATOMIC_AI_SUGGESTION_TYPE";
    /// AI confidence score
    pub const ATOMIC_AI_CONFIDENCE: &str = "ATOMIC_AI_CONFIDENCE";
    /// AI token count
    pub const ATOMIC_AI_TOKEN_COUNT: &str = "ATOMIC_AI_TOKEN_COUNT";
    /// AI prompt hash
    pub const ATOMIC_AI_PROMPT_HASH: &str = "ATOMIC_AI_PROMPT_HASH";
    /// Human review time in milliseconds
    pub const ATOMIC_AI_REVIEW_TIME: &str = "ATOMIC_AI_REVIEW_TIME";

    // Model parameters
    /// AI model temperature
    pub const ATOMIC_AI_TEMPERATURE: &str = "ATOMIC_AI_TEMPERATURE";
    /// AI model max tokens
    pub const ATOMIC_AI_MAX_TOKENS: &str = "ATOMIC_AI_MAX_TOKENS";
    /// AI model top_p
    pub const ATOMIC_AI_TOP_P: &str = "ATOMIC_AI_TOP_P";
}

impl Default for AttributionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_provider: String::new(),
            default_model: String::new(),
            track_prompts: true,
            require_confirmation: false,
        }
    }
}

impl AttributionDetector {
    /// Factory method to create a new attribution detector
    pub fn new(config: AttributionConfig) -> Self {
        let env_cache = Self::cache_environment_variables();

        Self { config, env_cache }
    }

    /// Factory method with default configuration
    pub fn with_defaults() -> Self {
        Self::new(AttributionConfig::default())
    }

    /// Factory method from configuration values
    pub fn from_config_values(
        enabled: bool,
        provider: String,
        model: String,
        track_prompts: bool,
        require_confirmation: bool,
    ) -> Self {
        let attribution_config = AttributionConfig {
            enabled,
            default_provider: provider,
            default_model: model,
            track_prompts,
            require_confirmation,
        };

        Self::new(attribution_config)
    }

    /// Cache relevant environment variables for performance
    fn cache_environment_variables() -> HashMap<String, String> {
        let mut cache = HashMap::new();

        let env_vars = [
            env_vars::ATOMIC_AI_ENABLED,
            env_vars::ATOMIC_AI_PROVIDER,
            env_vars::ATOMIC_AI_MODEL,
            env_vars::ATOMIC_AI_SUGGESTION_TYPE,
            env_vars::ATOMIC_AI_CONFIDENCE,
            env_vars::ATOMIC_AI_TOKEN_COUNT,
            env_vars::ATOMIC_AI_PROMPT_HASH,
            env_vars::ATOMIC_AI_REVIEW_TIME,
            env_vars::ATOMIC_AI_TEMPERATURE,
            env_vars::ATOMIC_AI_MAX_TOKENS,
            env_vars::ATOMIC_AI_TOP_P,
        ];

        for var in &env_vars {
            if let Ok(value) = env::var(var) {
                cache.insert(var.to_string(), value);
            }
        }

        cache
    }

    /// Detect if AI assistance is enabled
    pub fn is_ai_enabled(&self) -> bool {
        if !self.config.enabled {
            return false;
        }

        // Check environment variable override
        self.env_cache
            .get(env_vars::ATOMIC_AI_ENABLED)
            .and_then(|v| v.parse().ok())
            .unwrap_or(self.config.enabled)
    }

    /// Detect AI provider information from environment
    pub fn detect_ai_provider(&self) -> Option<AIProviderInfo> {
        if !self.is_ai_enabled() {
            return None;
        }

        let provider = self
            .env_cache
            .get(env_vars::ATOMIC_AI_PROVIDER)
            .cloned()
            .or_else(|| {
                if !self.config.default_provider.is_empty() {
                    Some(self.config.default_provider.clone())
                } else {
                    None
                }
            })?;

        let model = self
            .env_cache
            .get(env_vars::ATOMIC_AI_MODEL)
            .cloned()
            .or_else(|| {
                if !self.config.default_model.is_empty() {
                    Some(self.config.default_model.clone())
                } else {
                    None
                }
            })?;

        let suggestion_type = self
            .env_cache
            .get(env_vars::ATOMIC_AI_SUGGESTION_TYPE)
            .and_then(|s| self.parse_suggestion_type(s))
            .unwrap_or(SuggestionType::Complete);

        let confidence = self
            .env_cache
            .get(env_vars::ATOMIC_AI_CONFIDENCE)
            .and_then(|s| s.parse().ok());

        let token_count = self
            .env_cache
            .get(env_vars::ATOMIC_AI_TOKEN_COUNT)
            .and_then(|s| s.parse().ok());

        let prompt_hash = if self.config.track_prompts {
            self.env_cache
                .get(env_vars::ATOMIC_AI_PROMPT_HASH)
                .and_then(|s| self.parse_hash(s))
        } else {
            None
        };

        let model_params = self.parse_model_parameters();

        Some(AIProviderInfo {
            provider,
            model,
            suggestion_type,
            prompt_hash,
            confidence,
            token_count,
            model_params,
        })
    }

    /// Create attribution context for recording
    pub fn create_context(&self, author_info: AuthorInfo) -> AttributionContext {
        let ai_info = self.detect_ai_provider();
        let is_ai_assisted = ai_info.is_some();

        let mut metadata = HashMap::new();
        if let Some(review_time) = self.env_cache.get(env_vars::ATOMIC_AI_REVIEW_TIME) {
            metadata.insert("review_time_ms".to_string(), review_time.clone());
        }

        AttributionContext {
            ai_info,
            author_info,
            is_ai_assisted,
            metadata,
        }
    }

    /// Create an AttributedPatch from context and patch information
    pub fn create_attributed_patch(
        &self,
        context: &AttributionContext,
        patch_id: PatchId,
        description: String,
    ) -> AttributedPatch {
        let ai_metadata = context.ai_info.as_ref().map(|ai| {
            let human_review_time = context
                .metadata
                .get("review_time_ms")
                .and_then(|ms| ms.parse::<u64>().ok())
                .map(Duration::from_millis);

            AIMetadata {
                provider: ai.provider.clone(),
                model: ai.model.clone(),
                prompt_hash: ai.prompt_hash.unwrap_or(Hash::NONE),
                suggestion_type: ai.suggestion_type,
                human_review_time,
                acceptance_confidence: ai.confidence.unwrap_or(1.0),
                generation_timestamp: Utc::now(),
                token_count: ai.token_count,
                model_params: ai.model_params.clone(),
            }
        });

        AttributedPatch {
            patch_id,
            author: context.author_info.clone(),
            timestamp: Utc::now(),
            ai_assisted: context.is_ai_assisted,
            ai_metadata,
            description,
            dependencies: std::collections::HashSet::new(),
            conflicts_with: std::collections::HashSet::new(),
            confidence: context.ai_info.as_ref().and_then(|ai| ai.confidence),
        }
    }

    /// Parse suggestion type from string
    fn parse_suggestion_type(&self, s: &str) -> Option<SuggestionType> {
        match s.to_lowercase().as_str() {
            "complete" => Some(SuggestionType::Complete),
            "partial" => Some(SuggestionType::Partial),
            "collaborative" => Some(SuggestionType::Collaborative),
            "inspired" => Some(SuggestionType::Inspired),
            "review" => Some(SuggestionType::Review),
            "refactor" => Some(SuggestionType::Refactor),
            _ => None,
        }
    }

    /// Parse hash from string (placeholder implementation)
    fn parse_hash(&self, _s: &str) -> Option<Hash> {
        // For now, return Hash::NONE
        // In a real implementation, this would parse the hash string
        Some(Hash::NONE)
    }

    /// Parse model parameters from environment variables
    fn parse_model_parameters(&self) -> Option<ModelParameters> {
        let mut params = ModelParameters {
            temperature: None,
            max_tokens: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            custom: HashMap::new(),
        };

        let mut has_params = false;

        if let Some(temp) = self.env_cache.get(env_vars::ATOMIC_AI_TEMPERATURE) {
            if let Ok(val) = temp.parse() {
                params.temperature = Some(val);
                has_params = true;
            }
        }

        if let Some(max_tokens) = self.env_cache.get(env_vars::ATOMIC_AI_MAX_TOKENS) {
            if let Ok(val) = max_tokens.parse() {
                params.max_tokens = Some(val);
                has_params = true;
            }
        }

        if let Some(top_p) = self.env_cache.get(env_vars::ATOMIC_AI_TOP_P) {
            if let Ok(val) = top_p.parse() {
                params.top_p = Some(val);
                has_params = true;
            }
        }

        if has_params {
            Some(params)
        } else {
            None
        }
    }

    /// Check if confirmation is required for AI-assisted changes
    pub fn requires_confirmation(&self) -> bool {
        self.config.require_confirmation
    }

    /// Refresh environment variable cache
    pub fn refresh_cache(&mut self) {
        self.env_cache = Self::cache_environment_variables();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribution::AuthorId;
    use crate::pristine::NodeId;

    #[test]
    fn test_attribution_detector_creation() {
        let detector = AttributionDetector::with_defaults();
        assert!(!detector.is_ai_enabled());
    }

    #[test]
    fn test_attribution_context_creation() {
        let detector = AttributionDetector::with_defaults();
        let author = AuthorInfo {
            id: AuthorId::new(1),
            name: "Test Author".to_string(),
            email: "test@example.com".to_string(),
            is_ai: false,
        };

        let context = detector.create_context(author.clone());
        assert_eq!(context.author_info.name, "Test Author");
        assert!(!context.is_ai_assisted);
    }

    #[test]
    fn test_attributed_patch_creation() {
        let detector = AttributionDetector::with_defaults();
        let author = AuthorInfo {
            id: AuthorId::new(1),
            name: "Test Author".to_string(),
            email: "test@example.com".to_string(),
            is_ai: false,
        };

        let context = detector.create_context(author);
        let patch_id = PatchId::new(NodeId::ROOT);
        let patch = detector.create_attributed_patch(&context, patch_id, "Test patch".to_string());

        assert_eq!(patch.patch_id, patch_id);
        assert_eq!(patch.description, "Test patch");
        assert!(!patch.ai_assisted);
        assert!(patch.ai_metadata.is_none());
    }

    #[test]
    fn test_suggestion_type_parsing() {
        let detector = AttributionDetector::with_defaults();

        assert_eq!(
            detector.parse_suggestion_type("complete"),
            Some(SuggestionType::Complete)
        );
        assert_eq!(
            detector.parse_suggestion_type("PARTIAL"),
            Some(SuggestionType::Partial)
        );
        assert_eq!(detector.parse_suggestion_type("invalid"), None);
    }

    #[test]
    fn test_config_integration() {
        let detector = AttributionDetector::from_config_values(
            true,
            "test-provider".to_string(),
            "test-model".to_string(),
            false,
            true,
        );
        assert!(detector.config.enabled);
        assert_eq!(detector.config.default_provider, "test-provider");
        assert!(!detector.config.track_prompts);
        assert!(detector.config.require_confirmation);
    }
}
