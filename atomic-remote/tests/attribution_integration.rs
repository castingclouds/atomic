//! Integration tests for remote attribution functionality
//!
//! This test suite verifies the complete remote attribution workflow,
//! including protocol negotiation, push/pull operations, and error handling.

use anyhow::Result;
use atomic_remote::attribution::{AttributionRemoteExt, RemoteAttributionConfig};
use libatomic::attribution::{
    sync::{AttributedPatchBundle, RemoteAttributionStats},
    *,
};
use std::collections::HashSet;

use tokio;

/// Mock remote that implements AttributionRemoteExt for testing
struct MockRemote {
    supports_attribution: bool,
    protocol_version: u32,
    bundles: Vec<AttributedPatchBundle>,
    stats: RemoteAttributionStats,
    should_fail: bool,
}

impl MockRemote {
    fn new() -> Self {
        Self {
            supports_attribution: true,
            protocol_version: 1,
            bundles: Vec::new(),
            stats: RemoteAttributionStats {
                total_patches: 0,
                ai_assisted_patches: 0,
                unique_authors: 0,
                unique_ai_providers: HashSet::new(),
                last_sync_timestamp: Some(chrono::Utc::now().timestamp() as u64),
            },
            should_fail: false,
        }
    }

    fn with_no_attribution_support() -> Self {
        let mut mock = Self::new();
        mock.supports_attribution = false;
        mock
    }

    fn with_failure() -> Self {
        let mut mock = Self::new();
        mock.should_fail = true;
        mock
    }

    fn add_test_bundles(&mut self, count: usize) {
        for i in 0..count {
            let bundle = create_test_bundle(format!("test-patch-{}", i));
            self.bundles.push(bundle);
        }
        self.update_stats();
    }

    fn update_stats(&mut self) {
        self.stats.total_patches = self.bundles.len() as u64;
        self.stats.ai_assisted_patches = self
            .bundles
            .iter()
            .filter(|b| b.attribution.ai_assisted)
            .count() as u64;
        self.stats.unique_authors = self
            .bundles
            .iter()
            .map(|b| &b.attribution.author.id)
            .collect::<HashSet<_>>()
            .len() as u64;
        self.stats.unique_ai_providers = self
            .bundles
            .iter()
            .filter_map(|b| b.attribution.ai_metadata.as_ref())
            .map(|ai| ai.provider.clone())
            .collect();
    }
}

#[async_trait::async_trait]
impl AttributionRemoteExt for MockRemote {
    async fn supports_attribution(&mut self) -> Result<bool> {
        if self.should_fail {
            return Err(anyhow::anyhow!("Mock failure"));
        }
        Ok(self.supports_attribution)
    }

    async fn negotiate_attribution_protocol(&mut self) -> Result<u32> {
        if self.should_fail {
            return Err(anyhow::anyhow!("Protocol negotiation failed"));
        }
        if !self.supports_attribution {
            return Err(anyhow::anyhow!("Attribution not supported"));
        }
        Ok(self.protocol_version)
    }

    async fn push_with_attribution(
        &mut self,
        bundles: Vec<AttributedPatchBundle>,
        _channel: &str,
    ) -> Result<()> {
        if self.should_fail {
            return Err(anyhow::anyhow!("Push failed"));
        }
        self.bundles.extend(bundles);
        self.update_stats();
        Ok(())
    }

    async fn pull_with_attribution(
        &mut self,
        _from: u64,
        _channel: &str,
    ) -> Result<Vec<AttributedPatchBundle>> {
        if self.should_fail {
            return Err(anyhow::anyhow!("Pull failed"));
        }
        Ok(self.bundles.clone())
    }

    async fn get_attribution_stats(&mut self, _channel: &str) -> Result<RemoteAttributionStats> {
        if self.should_fail {
            return Err(anyhow::anyhow!("Stats failed"));
        }
        Ok(self.stats.clone())
    }
}

fn create_test_bundle(description: String) -> AttributedPatchBundle {
    let patch_id = PatchId::new(libatomic::pristine::NodeId::ROOT);
    let author = AuthorInfo {
        id: AuthorId::new(0),
        name: "Test User".to_string(),
        email: "test@example.com".to_string(),
        is_ai: false,
    };

    let ai_metadata = if description.contains("ai") {
        Some(AIMetadata {
            provider: "openai".to_string(),
            model: "gpt-4".to_string(),
            prompt_hash: libatomic::pristine::Hash::NONE,
            suggestion_type: SuggestionType::Complete,
            human_review_time: Some(std::time::Duration::from_secs(120)),
            acceptance_confidence: 0.95,
            generation_timestamp: chrono::Utc::now(),
            token_count: Some(500),
            model_params: None,
        })
    } else {
        None
    };

    let attribution = AttributedPatch {
        patch_id,
        author,
        timestamp: chrono::Utc::now(),
        ai_assisted: ai_metadata.is_some(),
        ai_metadata,
        dependencies: std::collections::HashSet::new(),
        conflicts_with: std::collections::HashSet::new(),
        description,
        confidence: Some(0.95),
    };

    AttributedPatchBundle {
        patch_data: b"mock patch data".to_vec(),
        attribution,
        signature: None,
    }
}

#[tokio::test]
async fn test_attribution_support_detection() -> Result<()> {
    let mut remote = MockRemote::new();
    assert!(remote.supports_attribution().await?);

    let mut remote_no_support = MockRemote::with_no_attribution_support();
    assert!(!remote_no_support.supports_attribution().await?);

    Ok(())
}

#[tokio::test]
async fn test_protocol_negotiation() -> Result<()> {
    let mut remote = MockRemote::new();
    let version = remote.negotiate_attribution_protocol().await?;
    assert_eq!(version, 1);

    let mut remote_no_support = MockRemote::with_no_attribution_support();
    assert!(remote_no_support
        .negotiate_attribution_protocol()
        .await
        .is_err());

    Ok(())
}

#[tokio::test]
async fn test_push_with_attribution() -> Result<()> {
    let mut remote = MockRemote::new();
    let bundles = vec![
        create_test_bundle("human-patch".to_string()),
        create_test_bundle("ai-assisted-patch".to_string()),
    ];

    assert_eq!(remote.bundles.len(), 0);
    remote.push_with_attribution(bundles, "main").await?;
    assert_eq!(remote.bundles.len(), 2);

    Ok(())
}

#[tokio::test]
async fn test_pull_with_attribution() -> Result<()> {
    let mut remote = MockRemote::new();
    remote.add_test_bundles(3);

    let pulled_bundles = remote.pull_with_attribution(0, "main").await?;
    assert_eq!(pulled_bundles.len(), 3);

    // Verify bundle content
    assert_eq!(pulled_bundles[0].attribution.description, "test-patch-0");
    assert_eq!(pulled_bundles[1].attribution.description, "test-patch-1");
    assert_eq!(pulled_bundles[2].attribution.description, "test-patch-2");

    Ok(())
}

#[tokio::test]
async fn test_attribution_stats() -> Result<()> {
    let mut remote = MockRemote::new();
    remote.add_test_bundles(5);

    let stats = remote.get_attribution_stats("main").await?;
    assert_eq!(stats.total_patches, 5);
    assert_eq!(stats.unique_authors, 1); // All from same test user
    assert!(stats.last_sync_timestamp.is_some());

    Ok(())
}

#[tokio::test]
async fn test_ai_assisted_attribution() -> Result<()> {
    let mut remote = MockRemote::new();

    // Add mix of human and AI-assisted patches
    let human_bundle = create_test_bundle("human-patch".to_string());
    let ai_bundle = create_test_bundle("ai-patch".to_string());

    remote
        .push_with_attribution(vec![human_bundle, ai_bundle], "main")
        .await?;

    let bundles = remote.pull_with_attribution(0, "main").await?;
    assert_eq!(bundles.len(), 2);

    let human_patch = &bundles[0];
    let ai_patch = &bundles[1];

    // Human patch should not be AI-assisted
    assert!(!human_patch.attribution.ai_assisted);
    assert!(human_patch.attribution.ai_metadata.is_none());

    // AI patch should be AI-assisted
    assert!(ai_patch.attribution.ai_assisted);
    assert!(ai_patch.attribution.ai_metadata.is_some());

    if let Some(ref ai_meta) = ai_patch.attribution.ai_metadata {
        assert_eq!(ai_meta.provider, "openai");
        assert_eq!(ai_meta.model, "gpt-4");
        assert_eq!(ai_meta.suggestion_type, SuggestionType::Complete);
    }

    Ok(())
}

#[tokio::test]
async fn test_error_handling() -> Result<()> {
    let mut remote = MockRemote::with_failure();

    // Test support detection failure
    assert!(remote.supports_attribution().await.is_err());

    // Test protocol negotiation failure
    assert!(remote.negotiate_attribution_protocol().await.is_err());

    // Test push failure
    let bundle = create_test_bundle("test".to_string());
    assert!(remote
        .push_with_attribution(vec![bundle], "main")
        .await
        .is_err());

    // Test pull failure
    assert!(remote.pull_with_attribution(0, "main").await.is_err());

    // Test stats failure
    assert!(remote.get_attribution_stats("main").await.is_err());

    Ok(())
}

#[tokio::test]
async fn test_configuration_loading() -> Result<()> {
    // Test default configuration
    let config = RemoteAttributionConfig::default();
    assert!(config.enabled);
    assert!(!config.require_signatures);
    assert_eq!(config.batch_size, 50);
    assert_eq!(config.timeout_seconds, 30);
    assert!(config.fallback_enabled);

    // Test environment variable loading
    std::env::set_var("ATOMIC_ATTRIBUTION_REMOTE_ENABLED", "false");
    std::env::set_var("ATOMIC_ATTRIBUTION_BATCH_SIZE", "25");
    std::env::set_var("ATOMIC_ATTRIBUTION_REQUIRE_SIGNATURES", "true");

    let env_config = RemoteAttributionConfig::from_environment();
    assert!(!env_config.enabled);
    assert_eq!(env_config.batch_size, 25);
    assert!(env_config.require_signatures);

    // Clean up environment variables
    std::env::remove_var("ATOMIC_ATTRIBUTION_REMOTE_ENABLED");
    std::env::remove_var("ATOMIC_ATTRIBUTION_BATCH_SIZE");
    std::env::remove_var("ATOMIC_ATTRIBUTION_REQUIRE_SIGNATURES");

    Ok(())
}

#[tokio::test]
async fn test_batch_operations() -> Result<()> {
    let mut remote = MockRemote::new();
    let large_batch: Vec<AttributedPatchBundle> = (0..100)
        .map(|i| create_test_bundle(format!("batch-patch-{}", i)))
        .collect();

    // Push large batch
    remote
        .push_with_attribution(large_batch.clone(), "main")
        .await?;
    assert_eq!(remote.bundles.len(), 100);

    // Pull and verify all patches are present
    let pulled_bundles = remote.pull_with_attribution(0, "main").await?;
    assert_eq!(pulled_bundles.len(), 100);

    // Verify stats reflect the batch
    let stats = remote.get_attribution_stats("main").await?;
    assert_eq!(stats.total_patches, 100);

    Ok(())
}

#[tokio::test]
async fn test_signature_handling() -> Result<()> {
    let mut bundle = create_test_bundle("signed-patch".to_string());

    // Add mock signature
    bundle.signature = Some(libatomic::attribution::sync::PatchSignature {
        public_key: b"mock_public_key".to_vec(),
        signature: b"mock_signature".to_vec(),
        algorithm: libatomic::attribution::sync::SignatureAlgorithm::Ed25519,
    });

    let mut remote = MockRemote::new();
    remote.push_with_attribution(vec![bundle], "main").await?;

    let pulled_bundles = remote.pull_with_attribution(0, "main").await?;
    assert_eq!(pulled_bundles.len(), 1);

    let pulled_bundle = &pulled_bundles[0];
    assert!(pulled_bundle.signature.is_some());

    if let Some(ref sig) = pulled_bundle.signature {
        assert_eq!(sig.public_key, b"mock_public_key");
        assert_eq!(sig.signature, b"mock_signature");
        assert_eq!(
            sig.algorithm,
            libatomic::attribution::sync::SignatureAlgorithm::Ed25519
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_dependency_tracking() -> Result<()> {
    let mut remote = MockRemote::new();

    // Create patches with dependencies
    let base_bundle = create_test_bundle("base-patch".to_string());
    let base_id = base_bundle.attribution.patch_id;

    let mut dependent_bundle = create_test_bundle("dependent-patch".to_string());
    dependent_bundle.attribution.dependencies = [base_id].into_iter().collect();

    remote
        .push_with_attribution(vec![base_bundle, dependent_bundle], "main")
        .await?;

    let pulled_bundles = remote.pull_with_attribution(0, "main").await?;
    assert_eq!(pulled_bundles.len(), 2);

    // Find the dependent patch and verify its dependencies
    let dependent_patch = pulled_bundles
        .iter()
        .find(|b| b.attribution.description == "dependent-patch")
        .unwrap();

    assert_eq!(dependent_patch.attribution.dependencies.len(), 1);
    assert!(dependent_patch.attribution.dependencies.contains(&base_id));

    Ok(())
}

#[tokio::test]
async fn test_conflict_tracking() -> Result<()> {
    let mut remote = MockRemote::new();

    // Create patches with conflicts
    let mut patch1 = create_test_bundle("patch-1".to_string());
    let patch1_id = patch1.attribution.patch_id;

    let mut patch2 = create_test_bundle("patch-2".to_string());
    let patch2_id = patch2.attribution.patch_id;

    // Set up conflicts
    patch1.attribution.conflicts_with = [patch2_id].into_iter().collect();
    patch2.attribution.conflicts_with = [patch1_id].into_iter().collect();

    remote
        .push_with_attribution(vec![patch1, patch2], "main")
        .await?;

    let pulled_bundles = remote.pull_with_attribution(0, "main").await?;
    assert_eq!(pulled_bundles.len(), 2);

    // Verify conflicts are preserved
    for bundle in &pulled_bundles {
        assert_eq!(bundle.attribution.conflicts_with.len(), 1);
    }

    Ok(())
}

#[tokio::test]
async fn test_multi_provider_attribution() -> Result<()> {
    let mut remote = MockRemote::new();

    // Create patches from different AI providers
    let mut openai_bundle = create_test_bundle("openai-patch".to_string());
    if let Some(ref mut ai_meta) = openai_bundle.attribution.ai_metadata {
        ai_meta.provider = "openai".to_string();
        ai_meta.model = "gpt-4".to_string();
    }

    let mut anthropic_bundle = create_test_bundle("anthropic-ai-patch".to_string());
    if let Some(ref mut ai_meta) = anthropic_bundle.attribution.ai_metadata {
        ai_meta.provider = "anthropic".to_string();
        ai_meta.model = "claude-3".to_string();
    }

    remote
        .push_with_attribution(vec![openai_bundle, anthropic_bundle], "main")
        .await?;

    let stats = remote.get_attribution_stats("main").await?;
    assert_eq!(stats.unique_ai_providers.len(), 2);
    assert!(stats.unique_ai_providers.contains("openai"));
    assert!(stats.unique_ai_providers.contains("anthropic"));

    Ok(())
}

#[tokio::test]
async fn test_confidence_tracking() -> Result<()> {
    let mut remote = MockRemote::new();

    let mut high_confidence_bundle = create_test_bundle("high-confidence-ai-patch".to_string());
    high_confidence_bundle.attribution.confidence = Some(0.95);
    if let Some(ref mut ai_meta) = high_confidence_bundle.attribution.ai_metadata {
        ai_meta.acceptance_confidence = 0.95;
    }

    let mut low_confidence_bundle = create_test_bundle("low-confidence-ai-patch".to_string());
    low_confidence_bundle.attribution.confidence = Some(0.65);
    if let Some(ref mut ai_meta) = low_confidence_bundle.attribution.ai_metadata {
        ai_meta.acceptance_confidence = 0.65;
    }

    remote
        .push_with_attribution(vec![high_confidence_bundle, low_confidence_bundle], "main")
        .await?;

    let pulled_bundles = remote.pull_with_attribution(0, "main").await?;
    assert_eq!(pulled_bundles.len(), 2);

    // Verify confidence values are preserved
    for bundle in &pulled_bundles {
        assert!(bundle.attribution.confidence.is_some());
        let confidence = bundle.attribution.confidence.unwrap();
        assert!(confidence >= 0.0 && confidence <= 1.0);
    }

    Ok(())
}
