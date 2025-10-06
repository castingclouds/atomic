//! Example demonstrating the AI attribution system for Atomic VCS
//!
//! This example shows how to:
//! 1. Create attributed patches with AI metadata
//! 2. Store attribution in the database
//! 3. Query attribution information
//! 4. Sync attribution across repositories

use libatomic::attribution::{
    AIConfig, AIMetadata, AttributedPatch, AttributedPatchFactory, AttributionBatch,
    AttributionStats, AuthorId, AuthorInfo, ModelParameters, PatchId, SuggestionType,
};
use libatomic::pristine::NodeId;
use std::collections::{HashMap, HashSet};

// Helper macro for creating HashSet
macro_rules! hashset {
    () => {
        HashSet::new()
    };
    ($($x:expr),+ $(,)?) => {
        {
            let mut set = HashSet::new();
            $(set.insert($x);)+
            set
        }
    };
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    println!("=== Atomic AI Attribution Example ===\n");

    // Create author information
    let human_author = AuthorInfo {
        id: AuthorId::new(1),
        name: "Alice Developer".to_string(),
        email: "alice@example.com".to_string(),
        is_ai: false,
    };

    let _ai_author = AuthorInfo {
        id: AuthorId::new(2),
        name: "GPT-4".to_string(),
        email: "ai@openai.com".to_string(),
        is_ai: true,
    };

    // Configure AI assistance
    let ai_config = AIConfig {
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        default_params: ModelParameters {
            temperature: Some(0.7),
            max_tokens: Some(2000),
            top_p: Some(0.95),
            frequency_penalty: None,
            presence_penalty: None,
            custom: HashMap::new(),
        },
        enabled: true,
    };

    // Create factory for attributed patches
    let factory = AttributedPatchFactory::new(human_author.clone()).with_ai_config(ai_config);

    // Example 1: Create a human-authored patch
    println!("1. Creating human-authored patch...");
    let human_patch = factory.create_human_patch(
        PatchId::new(NodeId::ROOT),
        "Fix: Resolve null pointer exception in parser".to_string(),
        HashSet::new(),
    );
    print_patch_info(&human_patch);

    // Example 2: Create an AI-assisted patch (partial suggestion)
    println!("\n2. Creating AI-assisted patch (partial)...");
    let ai_partial_patch = factory.create_ai_patch(
        PatchId::new(NodeId(libatomic::pristine::L64(1))),
        "Refactor: Extract validation logic into separate method".to_string(),
        hashset![human_patch.patch_id], // Depends on the human patch
        "prompt_hash_123".to_string(),
        SuggestionType::Partial,
        0.75,
    );
    print_patch_info(&ai_partial_patch);

    // Example 3: Create a collaborative patch
    println!("\n3. Creating collaborative patch...");
    let collab_patch = factory.create_ai_patch(
        PatchId::new(NodeId(libatomic::pristine::L64(2))),
        "Feature: Add caching layer with AI-suggested optimizations".to_string(),
        hashset![human_patch.patch_id, ai_partial_patch.patch_id],
        "prompt_hash_456".to_string(),
        SuggestionType::Collaborative,
        0.90,
    );
    print_patch_info(&collab_patch);

    // Example 4: Create AI-generated complete patch
    println!("\n4. Creating AI-generated complete patch...");
    let ai_complete_patch = factory.create_ai_patch(
        PatchId::new(NodeId(libatomic::pristine::L64(3))),
        "Tests: Generated unit tests for validation module".to_string(),
        hashset![ai_partial_patch.patch_id],
        "prompt_hash_789".to_string(),
        SuggestionType::Complete,
        0.95,
    );
    print_patch_info(&ai_complete_patch);

    // Example 5: Track attribution statistics
    println!("\n5. Attribution Statistics:");
    let mut stats = AttributionStats::new();

    // Update stats with our patches
    stats.update(&human_patch, 50); // 50 lines changed
    stats.update(&ai_partial_patch, 30); // 30 lines changed
    stats.update(&collab_patch, 100); // 100 lines changed
    stats.update(&ai_complete_patch, 75); // 75 lines changed

    print_statistics(&stats);

    // Example 6: Batch operations
    println!("\n6. Batch Attribution Operations:");
    let mut batch = AttributionBatch::new();

    batch.add(human_patch.clone(), 50);
    batch.add(ai_partial_patch.clone(), 30);
    batch.add(collab_patch.clone(), 100);
    batch.add(ai_complete_patch.clone(), 75);

    println!("  Batch created with {} patches", 4);
    println!("  Ready to commit to database");

    // Example 7: Dependency analysis
    println!("\n7. Dependency Analysis:");
    analyze_dependencies(&[
        human_patch.clone(),
        ai_partial_patch.clone(),
        collab_patch.clone(),
        ai_complete_patch.clone(),
    ]);

    // Example 8: AI metadata details
    println!("\n8. AI Metadata Details:");
    if let Some(ref metadata) = collab_patch.ai_metadata {
        print_ai_metadata(metadata);
    }

    println!("\n=== Example Complete ===");
    Ok(())
}

fn print_patch_info(patch: &AttributedPatch) {
    println!("  Patch ID: {}", patch.patch_id.to_base32());
    println!("  Author: {} <{}>", patch.author.name, patch.author.email);
    println!("  Description: {}", patch.description);
    println!("  AI Assisted: {}", patch.ai_assisted);

    if let Some(confidence) = patch.confidence {
        println!("  Confidence: {:.1}%", confidence * 100.0);
    }

    if !patch.dependencies.is_empty() {
        println!("  Dependencies: {} patches", patch.dependencies.len());
    }

    if let Some(ref ai_meta) = patch.ai_metadata {
        println!("  AI Type: {:?}", ai_meta.suggestion_type);
        println!("  Model: {}/{}", ai_meta.provider, ai_meta.model);
    }
}

fn print_statistics(stats: &AttributionStats) {
    println!("  Total Patches: {}", stats.total_patches);
    println!("  Human Patches: {}", stats.human_patches);
    println!("  AI-Assisted Patches: {}", stats.ai_assisted_patches);
    println!("  Total Lines: {}", stats.total_lines);
    println!("  AI-Assisted Lines: {}", stats.ai_assisted_lines);
    println!(
        "  AI Contribution: {:.1}%",
        (stats.ai_assisted_lines as f64 / stats.total_lines as f64) * 100.0
    );
    println!(
        "  Average AI Confidence: {:.1}%",
        stats.average_ai_confidence * 100.0
    );

    if !stats.suggestion_types.is_empty() {
        println!("  Suggestion Types:");
        for (suggestion_type, count) in &stats.suggestion_types {
            println!("    {:?}: {} patches", suggestion_type, count);
        }
    }
}

fn analyze_dependencies(patches: &[AttributedPatch]) {
    println!("  Dependency Graph:");

    for patch in patches {
        if patch.dependencies.is_empty() {
            println!("    {} -> (no dependencies)", patch.patch_id.to_base32());
        } else {
            for dep in &patch.dependencies {
                println!("    {} -> {}", patch.patch_id.to_base32(), dep.to_base32());
            }
        }
    }

    // Find root patches (no dependencies)
    let root_patches: Vec<_> = patches
        .iter()
        .filter(|p| p.dependencies.is_empty())
        .collect();

    println!("  Root Patches: {}", root_patches.len());
    for root in root_patches {
        println!("    - {}", root.description);
    }

    // Find leaf patches (not depended on by others)
    let all_deps: HashSet<_> = patches
        .iter()
        .flat_map(|p| p.dependencies.clone())
        .collect();

    let leaf_patches: Vec<_> = patches
        .iter()
        .filter(|p| !all_deps.contains(&p.patch_id))
        .collect();

    println!("  Leaf Patches: {}", leaf_patches.len());
    for leaf in leaf_patches {
        println!("    - {}", leaf.description);
    }
}

fn print_ai_metadata(metadata: &AIMetadata) {
    println!("  AI Provider: {}", metadata.provider);
    println!("  Model: {}", metadata.model);
    println!("  Suggestion Type: {:?}", metadata.suggestion_type);
    println!(
        "  Acceptance Confidence: {:.1}%",
        metadata.acceptance_confidence * 100.0
    );
    println!("  Generation Time: {}", metadata.generation_timestamp);

    if let Some(ref params) = metadata.model_params {
        println!("  Model Parameters:");
        if let Some(temp) = params.temperature {
            println!("    Temperature: {}", temp);
        }
        if let Some(max_tokens) = params.max_tokens {
            println!("    Max Tokens: {}", max_tokens);
        }
        if let Some(top_p) = params.top_p {
            println!("    Top P: {}", top_p);
        }
    }
}
