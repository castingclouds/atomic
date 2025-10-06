//! Apply Integration Example for AI Attribution System
//!
//! This example demonstrates how to use the apply integration system
//! to track AI attribution during patch application operations.

use chrono::Utc;
use libatomic::attribution::{
    helpers, ApplyAttributionContext, ApplyIntegrationConfig, AuthorId, AuthorInfo,
};
use libatomic::change::{Author, ChangeHeader_, Hashed, LocalChange};
use libatomic::pristine::Hash;
use std::collections::BTreeMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Apply Integration Example ===\n");

    // Configure apply integration
    let config = ApplyIntegrationConfig {
        enabled: true,
        auto_detect_ai: true,
        validate_chains: true,
        default_author: AuthorInfo {
            id: AuthorId::new(0),
            name: "Default User".to_string(),
            email: "default@example.com".to_string(),
            is_ai: false,
        },
    };

    let mut context = ApplyAttributionContext::new(config);

    println!("1. Creating sample changes with different attribution patterns...\n");

    // Example 1: Human-authored change
    let human_change = create_human_change()?;
    let human_hash = Hash::NONE; // Placeholder

    println!("Applying human-authored change...");
    if let Some(attribution) = context.pre_apply_hook(&human_change, &human_hash)? {
        println!("  - Patch ID: {}", attribution.patch_id);
        println!(
            "  - Author: {} ({})",
            attribution.author.name, attribution.author.email
        );
        println!("  - AI Assisted: {}", attribution.ai_assisted);
        println!("  - Description: {}\n", attribution.description);
    }

    // Example 2: AI-assisted change (auto-detected)
    let ai_change = create_ai_assisted_change()?;
    let ai_hash = Hash::NONE; // Placeholder

    println!("Applying AI-assisted change (auto-detected)...");
    if let Some(attribution) = context.pre_apply_hook(&ai_change, &ai_hash)? {
        println!("  - Patch ID: {}", attribution.patch_id);
        println!(
            "  - Author: {} ({})",
            attribution.author.name, attribution.author.email
        );
        println!("  - AI Assisted: {}", attribution.ai_assisted);
        println!(
            "  - AI Provider: {}",
            attribution
                .ai_metadata
                .as_ref()
                .map(|m| m.provider.as_str())
                .unwrap_or("None")
        );
        println!("  - Description: {}\n", attribution.description);
    }

    // Example 3: Show attribution statistics
    println!("2. Attribution Statistics:");
    let stats = context.get_attribution_stats();
    println!("  - Total patches: {}", stats.total_patches);
    println!("  - AI-assisted patches: {}", stats.ai_assisted_patches);
    println!("  - Human patches: {}", stats.human_patches);
    println!(
        "  - Average AI confidence: {:.2}",
        stats.average_ai_confidence
    );

    if !stats.provider_breakdown.is_empty() {
        println!("  - AI providers:");
        for (provider, count) in &stats.provider_breakdown {
            println!("    * {}: {}", provider, count);
        }
    }

    if !stats.suggestion_types.is_empty() {
        println!("  - Suggestion types:");
        for (suggestion_type, count) in &stats.suggestion_types {
            println!("    * {:?}: {}", suggestion_type, count);
        }
    }

    println!("\n3. Demonstrating environment variable integration...");

    // Set environment variables to simulate AI assistance
    std::env::set_var("ATOMIC_AI_ENABLED", "true");
    std::env::set_var("ATOMIC_AI_PROVIDER", "example-ai");
    std::env::set_var("ATOMIC_AI_MODEL", "example-model");
    std::env::set_var("ATOMIC_AI_CONFIDENCE", "0.95");

    // Use helper function to create attribution from environment
    if let Some(env_attribution) = helpers::create_attribution_from_env() {
        println!("Created attribution from environment variables:");
        println!("  - AI Enabled: {}", env_attribution.ai_assisted);
        if let Some(ai_metadata) = &env_attribution.ai_metadata {
            println!("  - Provider: {}", ai_metadata.provider);
            println!("  - Model: {}", ai_metadata.model);
            println!("  - Confidence: {}", ai_metadata.acceptance_confidence);
        }
    }

    println!("\n4. Testing serialization/deserialization of attribution...");

    if let Some(attribution) = helpers::create_attribution_from_env() {
        match helpers::serialize_attribution_for_metadata(&attribution) {
            Ok(serialized) => {
                println!(
                    "Successfully serialized attribution ({} bytes)",
                    serialized.len()
                );

                match helpers::deserialize_attribution_from_metadata(&serialized) {
                    Ok(deserialized) => {
                        println!("Successfully deserialized attribution");
                        println!("  - AI Assisted: {}", deserialized.ai_assisted);
                        println!("  - Version: {}", deserialized.attribution_version);
                    }
                    Err(e) => println!("Failed to deserialize: {}", e),
                }
            }
            Err(e) => println!("Failed to serialize: {}", e),
        }
    }

    println!("\n=== Apply Integration Example Complete ===");

    Ok(())
}

/// Create a sample human-authored change
fn create_human_change() -> Result<
    LocalChange<libatomic::change::Hunk<Option<Hash>, libatomic::change::Local>, Author>,
    Box<dyn std::error::Error>,
> {
    let mut author_map = BTreeMap::new();
    author_map.insert("display_name".to_string(), "Alice Developer".to_string());
    author_map.insert("email".to_string(), "alice@example.com".to_string());

    let header = ChangeHeader_ {
        message: "Fix bug in authentication module".to_string(),
        description: Some(
            "Resolved issue with token validation that was causing login failures.".to_string(),
        ),
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

    Ok(LocalChange {
        offsets: libatomic::change::Offsets::default(),
        hashed,
        unhashed: None,
        contents: vec![],
    })
}

/// Create a sample AI-assisted change (will be auto-detected)
fn create_ai_assisted_change() -> Result<
    LocalChange<libatomic::change::Hunk<Option<Hash>, libatomic::change::Local>, Author>,
    Box<dyn std::error::Error>,
> {
    let mut author_map = BTreeMap::new();
    author_map.insert("display_name".to_string(), "Bob Developer".to_string());
    author_map.insert("email".to_string(), "bob@example.com".to_string());

    let header = ChangeHeader_ {
        message: "AI-assisted refactoring of database queries".to_string(),
        description: Some(
            "Used GitHub Copilot to optimize SQL queries and improve performance.".to_string(),
        ),
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

    Ok(LocalChange {
        offsets: libatomic::change::Offsets::default(),
        hashed,
        unhashed: None,
        contents: vec![],
    })
}
