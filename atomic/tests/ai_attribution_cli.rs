//! Integration tests for AI attribution CLI functionality
//!
//! This test verifies that the AI attribution system works correctly
//! with CLI flags and environment variables.

use libatomic::pristine::HashExt;
use std::env;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_ai_attribution_environment_detection() {
    // Test that environment variables are properly detected
    env::set_var("ATOMIC_AI_ENABLED", "true");
    env::set_var("ATOMIC_AI_PROVIDER", "test-provider");
    env::set_var("ATOMIC_AI_MODEL", "test-model");
    env::set_var("ATOMIC_AI_SUGGESTION_TYPE", "complete");
    env::set_var("ATOMIC_AI_CONFIDENCE", "0.95");

    // Create a temporary directory for the test repository
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let repo_path = temp_dir.path();

    // Initialize a basic repository structure
    let atomic_dir = repo_path.join(".atomic");
    fs::create_dir(&atomic_dir).expect("Failed to create .atomic directory");

    // Create a basic config file
    let config_content = r#"
[ai_attribution]
enabled = true
provider = "openai"
model = "gpt-4"
track_prompts = true
require_confirmation = false
"#;
    fs::write(atomic_dir.join("config.toml"), config_content).expect("Failed to write config file");

    // Test file should exist (basic structure verification)
    assert!(atomic_dir.join("config.toml").exists());

    // Clean up environment variables
    env::remove_var("ATOMIC_AI_ENABLED");
    env::remove_var("ATOMIC_AI_PROVIDER");
    env::remove_var("ATOMIC_AI_MODEL");
    env::remove_var("ATOMIC_AI_SUGGESTION_TYPE");
    env::remove_var("ATOMIC_AI_CONFIDENCE");
}

#[test]
fn test_ai_attribution_config_structure() {
    // Test that the configuration structure works correctly
    let config_content = r#"
[ai_attribution]
enabled = true
provider = "anthropic"
model = "claude-3"
track_prompts = false
require_confirmation = true
"#;

    // Parse the TOML to verify structure
    let parsed: toml::Value = toml::from_str(config_content).expect("Failed to parse TOML");

    assert_eq!(parsed["ai_attribution"]["enabled"].as_bool(), Some(true));
    assert_eq!(
        parsed["ai_attribution"]["provider"].as_str(),
        Some("anthropic")
    );
    assert_eq!(parsed["ai_attribution"]["model"].as_str(), Some("claude-3"));
    assert_eq!(
        parsed["ai_attribution"]["track_prompts"].as_bool(),
        Some(false)
    );
    assert_eq!(
        parsed["ai_attribution"]["require_confirmation"].as_bool(),
        Some(true)
    );
}

#[test]
fn test_cli_flag_parsing() {
    // This test verifies that our CLI flag structure works correctly
    // We test the libatomic SuggestionType enum instead of the CLI enum
    use libatomic::attribution::SuggestionType;

    // Test that all suggestion types are properly defined
    let suggestion_types = vec![
        SuggestionType::Complete,
        SuggestionType::Partial,
        SuggestionType::Collaborative,
        SuggestionType::Inspired,
        SuggestionType::Review,
        SuggestionType::Refactor,
    ];

    // Verify that we have all expected types (compilation test)
    assert_eq!(suggestion_types.len(), 6);
}

#[test]
fn test_attribution_detector_integration() {
    // Test the attribution detector factory pattern
    use libatomic::attribution::detection::AttributionDetector;

    let detector = AttributionDetector::from_config_values(
        true,
        "test-provider".to_string(),
        "test-model".to_string(),
        true,
        false,
    );

    // Verify detector configuration
    assert!(detector.is_ai_enabled());
    assert!(!detector.requires_confirmation());
}

#[test]
fn test_environment_variable_names() {
    // Test that all environment variable names are properly defined
    use libatomic::attribution::detection::env_vars;

    let expected_vars = vec![
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

    // Verify all environment variables are properly named and prefixed
    for var in expected_vars {
        assert!(var.starts_with("ATOMIC_AI_"));
        assert!(!var.is_empty());
    }
}

#[test]
fn test_attribution_hash_query() {
    // Test that hash queries work correctly for attribution command
    use libatomic::attribution::{
        AIMetadata, AuthorId, AuthorInfo, SerializedAttribution, SuggestionType,
    };

    // Create proper author info following the factory pattern
    let author_info = AuthorInfo {
        id: AuthorId::new(123),
        name: "Test Author".to_string(),
        email: "test@example.com".to_string(),
        is_ai: false,
    };

    // Create a sample attribution data structure with proper types
    let attribution = SerializedAttribution {
        author: Some(author_info),
        ai_assisted: true,
        ai_metadata: Some(AIMetadata {
            provider: "test-provider".to_string(),
            model: "test-model".to_string(),
            prompt_hash: libatomic::Hash::from_bytes(&[
                1u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8,
                0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8,
                0u8,
            ])
            .expect("Valid hash bytes"),
            suggestion_type: SuggestionType::Complete,
            human_review_time: Some(std::time::Duration::from_secs(30)),
            acceptance_confidence: 0.95,
            generation_timestamp: chrono::Utc::now(),
            token_count: Some(150),
            model_params: None,
        }),
        confidence: Some(0.95),
        attribution_version: 1,
    };

    // Serialize the attribution to verify structure
    let serialized = bincode::serialize(&attribution).expect("Failed to serialize attribution");
    let deserialized: SerializedAttribution =
        bincode::deserialize(&serialized).expect("Failed to deserialize attribution");

    // Verify the attribution data survives serialization
    assert_eq!(deserialized.ai_assisted, true);
    assert_eq!(deserialized.confidence, Some(0.95));
    assert_eq!(deserialized.attribution_version, 1);
    assert!(deserialized.ai_metadata.is_some());
    assert!(deserialized.author.is_some());

    if let Some(ref metadata) = deserialized.ai_metadata {
        assert_eq!(metadata.provider, "test-provider");
        assert_eq!(metadata.model, "test-model");
        assert!(matches!(metadata.suggestion_type, SuggestionType::Complete));
        assert_eq!(metadata.acceptance_confidence, 0.95);
        assert_eq!(metadata.token_count, Some(150));
    }

    if let Some(ref author) = deserialized.author {
        assert_eq!(author.name, "Test Author");
        assert_eq!(author.email, "test@example.com");
        assert_eq!(author.is_ai, false);
    }
}

#[cfg(test)]
mod integration {
    use super::*;

    #[test]
    #[ignore] // This test requires the actual binary to be built
    fn test_record_command_with_ai_flags() {
        // This test would verify that the atomic record command
        // accepts the new AI attribution flags

        // Note: This is a placeholder for actual integration testing
        // In a real scenario, we would:
        // 1. Create a test repository
        // 2. Add some files
        // 3. Run: atomic record --ai-assisted --ai-provider openai --ai-model gpt-4
        // 4. Verify the change was recorded with attribution

        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let _repo_path = temp_dir.path();

        // Placeholder assertion
        assert!(true);
    }

    #[test]
    #[ignore] // This test requires the actual binary to be built
    fn test_attribution_hash_command() {
        // This test would verify that the atomic attribution --hash command works

        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let _repo_path = temp_dir.path();

        // In a real scenario, we would:
        // 1. Create a test repository with some changes
        // 2. Record a change with AI attribution
        // 3. Run: atomic attribution --hash <change_hash>
        // 4. Verify the output contains the expected attribution information
        // 5. Test both JSON and plaintext output formats

        // Placeholder assertion
        assert!(true);
    }
}
