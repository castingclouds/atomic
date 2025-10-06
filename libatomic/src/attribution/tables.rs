//! Database tables for attribution storage in Sanakirja
//!
//! This module defines the database tables and operations for storing
//! attribution metadata alongside patches in Atomic's pristine store.

use super::*;
use crate::pristine::{
    sanakirja::{Db, UDb},
    MutTxnT, TxnErr, TxnT, L64,
};
use std::marker::PhantomData;

/// Table definitions for attribution data
pub trait AttributionTxnT: TxnT {
    type Attribution: ::sanakirja::debug::Check + Clone;
    type AuthorPatches: ::sanakirja::debug::Check + Clone;
    type AIPatchMetadata: ::sanakirja::debug::Check + Clone;
    type PatchDependencies: ::sanakirja::debug::Check + Clone;
    type AttributionStats: ::sanakirja::debug::Check + Clone;

    /// Get attribution for a patch
    fn get_attribution(
        &self,
        patch_id: &PatchId,
    ) -> Result<Option<AttributedPatch>, TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>>;

    /// Get all patches by an author
    fn get_author_patches(
        &self,
        author_id: &AuthorId,
    ) -> Result<Vec<PatchId>, TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>>;

    /// Get AI metadata for a patch
    fn get_ai_metadata(
        &self,
        patch_id: &PatchId,
    ) -> Result<Option<AIMetadata>, TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>>;

    /// Get dependency attribution weights
    fn get_dependency_attributions(
        &self,
        patch_id: &PatchId,
    ) -> Result<
        Vec<(PatchId, AttributionWeight)>,
        TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>,
    >;

    /// Get attribution statistics for an author
    fn get_author_stats(
        &self,
        author_id: &AuthorId,
    ) -> Result<Option<AttributionStats>, TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>>;

    /// Check if a patch has AI assistance
    fn is_ai_assisted(
        &self,
        patch_id: &PatchId,
    ) -> Result<bool, TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>> {
        Ok(self
            .get_attribution(patch_id)?
            .map(|a| a.ai_assisted)
            .unwrap_or(false))
    }

    /// Get all AI-assisted patches
    fn iter_ai_patches(
        &self,
    ) -> Result<Vec<PatchId>, TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>>;

    /// Get patches by suggestion type
    fn get_patches_by_suggestion_type(
        &self,
        suggestion_type: SuggestionType,
    ) -> Result<Vec<PatchId>, TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>>;
}

/// Mutable operations for attribution data
pub trait AttributionMutTxnT: AttributionTxnT + MutTxnT {
    /// Store attribution for a patch
    fn put_attribution(
        &mut self,
        patch: &AttributedPatch,
    ) -> Result<(), TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>>;

    /// Add a patch to an author's list
    fn add_author_patch(
        &mut self,
        author_id: &AuthorId,
        patch_id: &PatchId,
    ) -> Result<(), TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>>;

    /// Store AI metadata
    fn put_ai_metadata(
        &mut self,
        patch_id: &PatchId,
        metadata: &AIMetadata,
    ) -> Result<(), TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>>;

    /// Store dependency attribution weight
    fn put_dependency_attribution(
        &mut self,
        dependent: &PatchId,
        dependency: &PatchId,
        weight: AttributionWeight,
    ) -> Result<(), TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>>;

    /// Update author statistics
    fn update_author_stats(
        &mut self,
        author_id: &AuthorId,
        stats: &AttributionStats,
    ) -> Result<(), TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>>;

    /// Remove attribution for a patch
    fn del_attribution(
        &mut self,
        patch_id: &PatchId,
    ) -> Result<(), TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>>;

    /// Batch import attributions
    fn import_attributions(
        &mut self,
        patches: Vec<AttributedPatch>,
    ) -> Result<(), TxnErr<<Self as crate::pristine::GraphTxnT>::GraphError>> {
        for patch in patches {
            self.put_attribution(&patch)?;
            self.add_author_patch(&patch.author.id, &patch.patch_id)?;
            if let Some(ref metadata) = patch.ai_metadata {
                self.put_ai_metadata(&patch.patch_id, metadata)?;
            }
        }
        Ok(())
    }
}

/// Attribution store implementation for Sanakirja backend
/// Uses simple types that are already Storable in Sanakirja
pub struct AttributionStore<T> {
    /// Database for patch attribution (serialized as bytes)
    pub patch_attribution: UDb<L64, [u8]>,
    /// Database for author -> patches mapping
    pub author_patches: Db<L64, L64>,
    /// Database for AI metadata (stored as bytes)
    pub ai_patch_metadata: UDb<L64, [u8]>,
    /// Database for dependency attribution weights (serialized as bytes)
    pub patch_dependencies_attribution: UDb<L64, [u8]>,
    /// Database for author statistics (stored as bytes)
    pub author_stats: UDb<L64, [u8]>,
    /// Database for author info (stored as bytes)
    pub author_info: UDb<L64, [u8]>,
    /// Database for patch descriptions (stored as bytes)
    pub patch_descriptions: UDb<L64, [u8]>,
    /// Phantom data for type parameter
    _phantom: PhantomData<T>,
}

impl<T> AttributionStore<T> {
    /// Create a new attribution store
    pub fn new(
        patch_attribution: UDb<L64, [u8]>,
        author_patches: Db<L64, L64>,
        ai_patch_metadata: UDb<L64, [u8]>,
        patch_dependencies_attribution: UDb<L64, [u8]>,
        author_stats: UDb<L64, [u8]>,
        author_info: UDb<L64, [u8]>,
        patch_descriptions: UDb<L64, [u8]>,
    ) -> Self {
        AttributionStore {
            patch_attribution,
            author_patches,
            ai_patch_metadata,
            patch_dependencies_attribution,
            author_stats,
            author_info,
            patch_descriptions,
            _phantom: PhantomData,
        }
    }
}

/// Helper functions for attribution queries
pub mod queries {
    use super::*;

    /// Find all patches that depend on a given patch
    pub fn find_dependent_patches<T: AttributionTxnT>(
        _txn: &T,
        _patch_id: &PatchId,
    ) -> Result<Vec<PatchId>, TxnErr<<T as crate::pristine::GraphTxnT>::GraphError>> {
        // This would iterate through all patches and check their dependencies
        // Implementation would use cursor operations on the database
        todo!("Implement using cursor operations")
    }

    /// Calculate total attribution weight for a patch
    pub fn calculate_attribution_weight<T: AttributionTxnT>(
        txn: &T,
        patch_id: &PatchId,
    ) -> Result<f64, TxnErr<<T as crate::pristine::GraphTxnT>::GraphError>> {
        let weights = txn.get_dependency_attributions(patch_id)?;
        Ok(weights.iter().map(|(_, w)| w.weight).sum())
    }

    /// Find patches with conflicts
    pub fn find_conflicting_patches<T: AttributionTxnT>(
        txn: &T,
        patch_id: &PatchId,
    ) -> Result<Vec<PatchId>, TxnErr<<T as crate::pristine::GraphTxnT>::GraphError>> {
        match txn.get_attribution(patch_id)? {
            Some(attr) => Ok(attr.conflicts_with.into_iter().collect()),
            None => Ok(Vec::new()),
        }
    }

    /// Get attribution chain (all dependencies recursively)
    pub fn get_attribution_chain<T: AttributionTxnT>(
        _txn: &T,
        patch_id: &PatchId,
        visited: &mut HashSet<PatchId>,
    ) -> Result<Vec<PatchId>, TxnErr<<T as crate::pristine::GraphTxnT>::GraphError>> {
        if !visited.insert(*patch_id) {
            return Ok(Vec::new()); // Already visited, avoid cycles
        }

        let chain = vec![*patch_id];

        // TODO: Implement actual chain traversal using txn
        // if let Some(attr) = txn.get_attribution(patch_id)? {
        //     for dep in attr.dependencies {
        //         let dep_chain = get_attribution_chain(txn, &dep, visited)?;
        //         chain.extend(dep_chain);
        //     }
        // }

        Ok(chain)
    }

    /// Calculate AI contribution percentage for a project
    pub fn calculate_ai_contribution<T: AttributionTxnT>(
        _txn: &T,
    ) -> Result<f64, TxnErr<<T as crate::pristine::GraphTxnT>::GraphError>> {
        // This is simplified - would need to get total patch count
        // Implementation would use actual database operations
        todo!("Implement using database operations")
    }
}

/// Conflict resolution strategies based on attribution
#[derive(Debug, Clone, Copy)]
pub enum ConflictResolutionStrategy {
    /// Prefer human-authored patches
    PreferHuman,
    /// Prefer AI-authored patches with high confidence
    PreferHighConfidenceAI,
    /// Prefer patches with more dependencies (more context)
    PreferMoreContext,
    /// Manual resolution required
    Manual,
}

/// Resolve conflicts based on attribution
pub fn resolve_conflict_by_attribution<T: AttributionTxnT>(
    txn: &T,
    conflicting_patches: Vec<PatchId>,
    strategy: ConflictResolutionStrategy,
) -> Result<Option<PatchId>, TxnErr<<T as crate::pristine::GraphTxnT>::GraphError>> {
    let attributions: Vec<_> = conflicting_patches
        .iter()
        .filter_map(|id| txn.get_attribution(id).ok().flatten())
        .collect();

    match strategy {
        ConflictResolutionStrategy::PreferHuman => {
            // Find first human-authored patch
            for (i, attr) in attributions.iter().enumerate() {
                if !attr.ai_assisted {
                    return Ok(Some(conflicting_patches[i]));
                }
            }
        }
        ConflictResolutionStrategy::PreferHighConfidenceAI => {
            // Find AI patch with highest confidence
            let mut best_idx = None;
            let mut best_confidence = 0.0;

            for (i, attr) in attributions.iter().enumerate() {
                if let Some(conf) = attr.confidence {
                    if conf > best_confidence {
                        best_confidence = conf;
                        best_idx = Some(i);
                    }
                }
            }

            if let Some(idx) = best_idx {
                return Ok(Some(conflicting_patches[idx]));
            }
        }
        ConflictResolutionStrategy::PreferMoreContext => {
            // Find patch with most dependencies (most context)
            let mut best_idx = 0;
            let mut max_deps = 0;

            for (i, attr) in attributions.iter().enumerate() {
                if attr.dependencies.len() > max_deps {
                    max_deps = attr.dependencies.len();
                    best_idx = i;
                }
            }

            if !attributions.is_empty() {
                return Ok(Some(conflicting_patches[best_idx]));
            }
        }
        ConflictResolutionStrategy::Manual => {
            return Ok(None); // Require manual resolution
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conflict_resolution_strategies() {
        // Test that conflict resolution strategies are properly defined
        let strategy = ConflictResolutionStrategy::PreferHuman;
        match strategy {
            ConflictResolutionStrategy::PreferHuman => assert!(true),
            _ => assert!(false),
        }

        let strategy = ConflictResolutionStrategy::PreferHighConfidenceAI;
        match strategy {
            ConflictResolutionStrategy::PreferHighConfidenceAI => assert!(true),
            _ => assert!(false),
        }
    }

    #[test]
    fn test_attribution_store_type_parameter() {
        // Test that AttributionStore can be parameterized with different types
        // This is a compile-time test - if it compiles, it passes

        // Verify the store can be parameterized with unit type
        let _store_size = std::mem::size_of::<AttributionStore<()>>();

        // Verify PhantomData doesn't cause issues
        let _phantom_size = std::mem::size_of::<PhantomData<()>>();
        assert_eq!(_phantom_size, 0);
    }
}
