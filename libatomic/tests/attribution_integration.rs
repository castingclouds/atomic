//! Integration tests for the attribution database functionality
//!
//! These tests verify that the attribution storage works correctly with
//! the Sanakirja database backend.

use chrono::Utc;
use libatomic::attribution::{
    AIMetadata, AttributedPatch, AttributionStats, AuthorId, AuthorInfo, ModelParameters, PatchId,
    SanakirjaAttributionStore, SuggestionType,
};
use libatomic::pristine::{sanakirja::Pristine, Base32, NodeId, Hash};
use std::collections::{HashMap, HashSet};
use tempfile::TempDir;

/// Create a test pristine database
fn create_test_pristine() -> (TempDir, Pristine) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let db_path = temp_dir.path().join("test.db");
    let pristine = Pristine::new(&db_path).expect("Failed to create pristine");
    (temp_dir, pristine)
}

/// Create a test attributed patch
fn create_test_patch(patch_id: u64, author_id: u64, ai_assisted: bool) -> AttributedPatch {
    let author = AuthorInfo {
        id: AuthorId::new(author_id),
        name: format!("Test Author {}", author_id),
        email: format!("author{}@example.com", author_id),
        is_ai: ai_assisted,
    };

    let ai_metadata = if ai_assisted {
        Some(AIMetadata {
            provider: "test-provider".to_string(),
            model: "test-model".to_string(),
            prompt_hash: Hash::NONE,
            suggestion_type: SuggestionType::Complete,
            human_review_time: None,
            acceptance_confidence: 0.95,
            generation_timestamp: Utc::now(),
            token_count: Some(100),
            model_params: Some(ModelParameters {
                temperature: Some(0.7),
                max_tokens: Some(1000),
                top_p: Some(0.9),
                frequency_penalty: None,
                presence_penalty: None,
                custom: HashMap::new(),
            }),
        })
    } else {
        None
    };

    AttributedPatch {
        patch_id: PatchId::new(
            NodeId::from_base32(&format!("{:016}", patch_id).as_bytes())
                .unwrap_or(NodeId::ROOT),
        ),
        author,
        timestamp: chrono::Utc::now(),
        ai_assisted,
        ai_metadata,
        description: format!("Test patch {}", patch_id),
        dependencies: HashSet::new(),
        conflicts_with: HashSet::new(),
        confidence: if ai_assisted { Some(0.95) } else { None },
    }
}

#[test]
fn test_attribution_store_initialization() {
    let (_temp_dir, pristine) = create_test_pristine();
    let store = SanakirjaAttributionStore::new(pristine);

    // Initialize tables should work without error
    store
        .initialize_tables()
        .expect("Failed to initialize tables");
}

#[test]
fn test_store_and_retrieve_attribution() {
    let (_temp_dir, pristine) = create_test_pristine();
    let store = SanakirjaAttributionStore::new(pristine);

    // Initialize tables
    store
        .initialize_tables()
        .expect("Failed to initialize tables");

    // Create a test patch
    let patch = create_test_patch(1, 1, false);

    // Store the attribution
    store
        .put_attribution(&patch)
        .expect("Failed to store attribution");

    // Retrieve the attribution
    let retrieved = store
        .get_attribution(&patch.patch_id)
        .expect("Failed to retrieve attribution");

    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.patch_id, patch.patch_id);
    assert_eq!(retrieved.author.id, patch.author.id);
    assert_eq!(retrieved.ai_assisted, patch.ai_assisted);
    assert_eq!(retrieved.description, patch.description);
}

#[test]
fn test_store_ai_assisted_patch() {
    let (_temp_dir, pristine) = create_test_pristine();
    let store = SanakirjaAttributionStore::new(pristine);

    // Initialize tables
    store
        .initialize_tables()
        .expect("Failed to initialize tables");

    // Create an AI-assisted test patch
    let patch = create_test_patch(2, 1, true);

    // Store the attribution
    store
        .put_attribution(&patch)
        .expect("Failed to store attribution");

    // Retrieve the attribution
    let retrieved = store
        .get_attribution(&patch.patch_id)
        .expect("Failed to retrieve attribution");

    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert!(retrieved.ai_assisted);
    assert!(retrieved.ai_metadata.is_some());
    assert!(retrieved.confidence.is_some());

    // Check AI metadata
    let ai_metadata = retrieved.ai_metadata.unwrap();
    assert_eq!(ai_metadata.provider, "test-provider");
    assert_eq!(ai_metadata.suggestion_type, SuggestionType::Complete);
}

#[test]
fn test_get_author_patches() {
    let (_temp_dir, pristine) = create_test_pristine();
    let store = SanakirjaAttributionStore::new(pristine);

    // Initialize tables
    store
        .initialize_tables()
        .expect("Failed to initialize tables");

    let author_id = AuthorId::new(42);

    // Create multiple patches for the same author
    let patch1 = create_test_patch(10, 42, false);
    let patch2 = create_test_patch(11, 42, true);
    let patch3 = create_test_patch(12, 99, false); // Different author

    // Store all patches
    store
        .put_attribution(&patch1)
        .expect("Failed to store patch 1");
    store
        .put_attribution(&patch2)
        .expect("Failed to store patch 2");
    store
        .put_attribution(&patch3)
        .expect("Failed to store patch 3");

    // Get patches for author 42
    let author_patches = store
        .get_author_patches(&author_id)
        .expect("Failed to get author patches");

    // Should have 2 patches for author 42
    assert_eq!(author_patches.len(), 2);
    assert!(author_patches.contains(&patch1.patch_id));
    assert!(author_patches.contains(&patch2.patch_id));
    assert!(!author_patches.contains(&patch3.patch_id));
}

#[test]
fn test_get_ai_patches() {
    let (_temp_dir, pristine) = create_test_pristine();
    let store = SanakirjaAttributionStore::new(pristine);

    // Initialize tables
    store
        .initialize_tables()
        .expect("Failed to initialize tables");

    // Create a mix of human and AI patches
    let human_patch = create_test_patch(20, 1, false);
    let ai_patch1 = create_test_patch(21, 1, true);
    let ai_patch2 = create_test_patch(22, 2, true);

    // Store all patches
    store
        .put_attribution(&human_patch)
        .expect("Failed to store human patch");
    store
        .put_attribution(&ai_patch1)
        .expect("Failed to store AI patch 1");
    store
        .put_attribution(&ai_patch2)
        .expect("Failed to store AI patch 2");

    // Get all AI-assisted patches
    let ai_patches = store.get_ai_patches().expect("Failed to get AI patches");

    // Should have 2 AI patches
    assert_eq!(ai_patches.len(), 2);
    assert!(ai_patches.contains(&ai_patch1.patch_id));
    assert!(ai_patches.contains(&ai_patch2.patch_id));
    assert!(!ai_patches.contains(&human_patch.patch_id));
}

#[test]
fn test_author_statistics() {
    let (_temp_dir, pristine) = create_test_pristine();
    let store = SanakirjaAttributionStore::new(pristine);

    // Initialize tables
    store
        .initialize_tables()
        .expect("Failed to initialize tables");

    let author_id = AuthorId::new(100);

    // Create some statistics
    let mut stats = AttributionStats::new();
    stats.total_patches = 10;
    stats.ai_assisted_patches = 3;
    stats.human_patches = 7;
    stats.total_lines = 1000;
    stats.ai_assisted_lines = 250;
    stats.average_ai_confidence = 0.85;

    // Store the statistics
    store
        .update_author_stats(&author_id, &stats)
        .expect("Failed to update author stats");

    // Retrieve the statistics
    let retrieved_stats = store
        .get_author_stats(&author_id)
        .expect("Failed to get author stats");

    assert!(retrieved_stats.is_some());
    let retrieved_stats = retrieved_stats.unwrap();
    assert_eq!(retrieved_stats.total_patches, 10);
    assert_eq!(retrieved_stats.ai_assisted_patches, 3);
    assert_eq!(retrieved_stats.human_patches, 7);
    assert_eq!(retrieved_stats.total_lines, 1000);
    assert_eq!(retrieved_stats.ai_assisted_lines, 250);
    assert!((retrieved_stats.average_ai_confidence - 0.85).abs() < 0.001);
}

#[test]
fn test_delete_attribution() {
    let (_temp_dir, pristine) = create_test_pristine();
    let store = SanakirjaAttributionStore::new(pristine);

    // Initialize tables
    store
        .initialize_tables()
        .expect("Failed to initialize tables");

    // Create and store a patch
    let patch = create_test_patch(30, 5, true);
    store
        .put_attribution(&patch)
        .expect("Failed to store attribution");

    // Verify it exists
    let retrieved = store
        .get_attribution(&patch.patch_id)
        .expect("Failed to retrieve attribution");
    assert!(retrieved.is_some());

    // Delete the attribution
    store
        .delete_attribution(&patch.patch_id)
        .expect("Failed to delete attribution");

    // Verify it's gone
    let retrieved = store
        .get_attribution(&patch.patch_id)
        .expect("Failed to retrieve attribution after deletion");
    assert!(retrieved.is_none());
}

#[test]
fn test_get_patches_by_suggestion_type() {
    let (_temp_dir, pristine) = create_test_pristine();
    let store = SanakirjaAttributionStore::new(pristine);

    // Initialize tables
    store
        .initialize_tables()
        .expect("Failed to initialize tables");

    // Create patches with different suggestion types
    let mut generate_patch = create_test_patch(40, 1, true);
    if let Some(ref mut metadata) = generate_patch.ai_metadata {
        metadata.suggestion_type = SuggestionType::Complete;
    }

    let mut complete_patch = create_test_patch(41, 1, true);
    if let Some(ref mut metadata) = complete_patch.ai_metadata {
        metadata.suggestion_type = SuggestionType::Partial;
    }

    let mut refactor_patch = create_test_patch(42, 1, true);
    if let Some(ref mut metadata) = refactor_patch.ai_metadata {
        metadata.suggestion_type = SuggestionType::Refactor;
    }

    // Store all patches
    store
        .put_attribution(&generate_patch)
        .expect("Failed to store generate patch");
    store
        .put_attribution(&complete_patch)
        .expect("Failed to store complete patch");
    store
        .put_attribution(&refactor_patch)
        .expect("Failed to store refactor patch");

    // Get patches by suggestion type
    let generate_patches = store
        .get_patches_by_suggestion_type(SuggestionType::Complete)
        .expect("Failed to get generate patches");

    assert_eq!(generate_patches.len(), 1);
    assert!(generate_patches.contains(&generate_patch.patch_id));

    let refactor_patches = store
        .get_patches_by_suggestion_type(SuggestionType::Refactor)
        .expect("Failed to get refactor patches");

    assert_eq!(refactor_patches.len(), 1);
    assert!(refactor_patches.contains(&refactor_patch.patch_id));
}

#[test]
fn test_multiple_operations() {
    let (_temp_dir, pristine) = create_test_pristine();
    let store = SanakirjaAttributionStore::new(pristine);

    // Initialize tables
    store
        .initialize_tables()
        .expect("Failed to initialize tables");

    // Perform multiple operations to test robustness
    for i in 0..5 {
        let patch = create_test_patch(50 + i, 1 + (i % 3), i % 2 == 0);
        store
            .put_attribution(&patch)
            .expect(&format!("Failed to store patch {}", i));

        let retrieved = store
            .get_attribution(&patch.patch_id)
            .expect(&format!("Failed to retrieve patch {}", i));
        assert!(retrieved.is_some());
    }

    // Get all AI patches
    let ai_patches = store.get_ai_patches().expect("Failed to get AI patches");
    // Should have approximately half of the patches as AI-assisted
    assert!(ai_patches.len() >= 2 && ai_patches.len() <= 3);
}
