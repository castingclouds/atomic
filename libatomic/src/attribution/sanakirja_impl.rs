//! Simple attribution storage implementation for Sanakirja
//!
//! This module provides a basic implementation of attribution storage
//! that works alongside the existing Sanakirja database without modifying
//! the core transaction types.

use super::{AIMetadata, AttributedPatch, AttributionStats, AuthorId, PatchId, SuggestionType};
use crate::pristine::{
    sanakirja::{Pristine, Root, SanakirjaError, UDb, UP},
    MutTxnT, L64,
};
use ::sanakirja::{btree, RootDb};

/// Simple attribution store that can be used alongside existing transactions
pub struct AttributionStore {
    pristine: Pristine,
}

impl AttributionStore {
    /// Create a new attribution store
    pub fn new(pristine: Pristine) -> Self {
        Self { pristine }
    }

    /// Get attribution for a patch
    pub fn get_attribution(
        &self,
        patch_id: &PatchId,
    ) -> Result<Option<AttributedPatch>, SanakirjaError> {
        let txn = self.pristine.txn_begin()?;

        if let Some(db) = txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::PatchAttribution as usize)
        {
            let key = patch_id.0 .0;
            if let Some((_, data)) = btree::get(&txn.txn, &db, &key, None)? {
                let patch: AttributedPatch = bincode::deserialize(data).map_err(|e| {
                    SanakirjaError::Sanakirja(::sanakirja::Error::IO(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )))
                })?;
                return Ok(Some(patch));
            }
        }

        Ok(None)
    }

    /// Store attribution for a patch
    pub fn put_attribution(&self, patch: &AttributedPatch) -> Result<(), SanakirjaError> {
        let mut txn = self.pristine.mut_txn_begin()?;

        // Get or create patch attribution table
        let mut db = if let Some(existing_db) = txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::PatchAttribution as usize)
        {
            existing_db
        } else {
            unsafe { btree::create_db_(&mut txn.txn)? }
        };

        let key = patch.patch_id.0 .0;
        let data = bincode::serialize(patch).map_err(|e| {
            SanakirjaError::Sanakirja(::sanakirja::Error::IO(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )))
        })?;

        btree::put(&mut txn.txn, &mut db, &key, &data[..])?;

        // Update root
        txn.txn
            .set_root(Root::PatchAttribution as usize, db.db.into());

        // Also update author patches table - store list of patches per author
        let mut author_db = if let Some(existing_db) = txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::AuthorPatches as usize)
        {
            existing_db
        } else {
            unsafe { btree::create_db_(&mut txn.txn)? }
        };

        let author_key = patch.author.id.0;

        // Get existing patch list for this author
        let mut patch_list =
            if let Some((_, data)) = btree::get(&txn.txn, &author_db, &author_key, None)? {
                bincode::deserialize::<Vec<PatchId>>(data).unwrap_or_else(|_| Vec::new())
            } else {
                Vec::new()
            };

        // Add this patch if it's not already in the list
        if !patch_list.contains(&patch.patch_id) {
            patch_list.push(patch.patch_id);
        }

        // Serialize and store the updated list
        let patch_list_data = bincode::serialize(&patch_list).map_err(|e| {
            SanakirjaError::Sanakirja(::sanakirja::Error::IO(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )))
        })?;

        btree::put(
            &mut txn.txn,
            &mut author_db,
            &author_key,
            &patch_list_data[..],
        )?;
        txn.txn
            .set_root(Root::AuthorPatches as usize, author_db.db.into());

        // Store AI metadata if present
        if let Some(ref metadata) = patch.ai_metadata {
            let mut ai_db = if let Some(existing_db) = txn
                .txn
                .root_db::<L64, [u8], UP<L64, [u8]>>(Root::AIPatchMetadata as usize)
            {
                existing_db
            } else {
                unsafe { btree::create_db_(&mut txn.txn)? }
            };

            let ai_data = bincode::serialize(metadata).map_err(|e| {
                SanakirjaError::Sanakirja(::sanakirja::Error::IO(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )))
            })?;

            btree::put(&mut txn.txn, &mut ai_db, &key, &ai_data[..])?;
            txn.txn
                .set_root(Root::AIPatchMetadata as usize, ai_db.db.into());
        }

        txn.commit()?;
        Ok(())
    }

    /// Get all patches by an author
    pub fn get_author_patches(&self, author_id: &AuthorId) -> Result<Vec<PatchId>, SanakirjaError> {
        let txn = self.pristine.txn_begin()?;
        let mut patches = Vec::new();

        if let Some(db) = txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::AuthorPatches as usize)
        {
            let author_key = author_id.0;

            // Get the patch list for this author
            if let Some((_, data)) = btree::get(&txn.txn, &db, &author_key, None)? {
                if let Ok(patch_list) = bincode::deserialize::<Vec<PatchId>>(data) {
                    patches = patch_list;
                }
            }
        }

        Ok(patches)
    }

    /// Get AI metadata for a patch
    pub fn get_ai_metadata(
        &self,
        patch_id: &PatchId,
    ) -> Result<Option<AIMetadata>, SanakirjaError> {
        let txn = self.pristine.txn_begin()?;

        if let Some(db) = txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::AIPatchMetadata as usize)
        {
            let key = patch_id.0 .0;
            if let Some((_, data)) = btree::get(&txn.txn, &db, &key, None)? {
                let metadata: AIMetadata = bincode::deserialize(data).map_err(|e| {
                    SanakirjaError::Sanakirja(::sanakirja::Error::IO(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )))
                })?;
                return Ok(Some(metadata));
            }
        }

        Ok(None)
    }

    /// Get all AI-assisted patches
    pub fn get_ai_patches(&self) -> Result<Vec<PatchId>, SanakirjaError> {
        let txn = self.pristine.txn_begin()?;
        let mut ai_patches = Vec::new();

        if let Some(db) = txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::PatchAttribution as usize)
        {
            for result in btree::iter(&txn.txn, &db, None)? {
                let (patch_id, data) = result?;
                if let Ok(patch) = bincode::deserialize::<AttributedPatch>(data) {
                    if patch.ai_assisted {
                        ai_patches.push(PatchId::new(crate::pristine::NodeId(*patch_id)));
                    }
                }
            }
        }

        Ok(ai_patches)
    }

    /// Get patches by suggestion type
    pub fn get_patches_by_suggestion_type(
        &self,
        suggestion_type: SuggestionType,
    ) -> Result<Vec<PatchId>, SanakirjaError> {
        let txn = self.pristine.txn_begin()?;
        let mut matching_patches = Vec::new();

        if let Some(db) = txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::AIPatchMetadata as usize)
        {
            for result in btree::iter(&txn.txn, &db, None)? {
                let (patch_id, data) = result?;
                let metadata: AIMetadata = bincode::deserialize(data).map_err(|e| {
                    SanakirjaError::Sanakirja(::sanakirja::Error::IO(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )))
                })?;

                if metadata.suggestion_type == suggestion_type {
                    matching_patches.push(PatchId::new(crate::pristine::NodeId(*patch_id)));
                }
            }
        }

        Ok(matching_patches)
    }

    /// Update author statistics
    pub fn update_author_stats(
        &self,
        author_id: &AuthorId,
        stats: &AttributionStats,
    ) -> Result<(), SanakirjaError> {
        let mut txn = self.pristine.mut_txn_begin()?;

        let mut db = if let Some(existing_db) = txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::AuthorStats as usize)
        {
            existing_db
        } else {
            unsafe { btree::create_db_(&mut txn.txn)? }
        };

        let key = author_id.0;
        let data = bincode::serialize(stats).map_err(|e| {
            SanakirjaError::Sanakirja(::sanakirja::Error::IO(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )))
        })?;

        btree::put(&mut txn.txn, &mut db, &key, &data[..])?;
        txn.txn.set_root(Root::AuthorStats as usize, db.db.into());

        txn.commit()?;
        Ok(())
    }

    /// Get author statistics
    pub fn get_author_stats(
        &self,
        author_id: &AuthorId,
    ) -> Result<Option<AttributionStats>, SanakirjaError> {
        let txn = self.pristine.txn_begin()?;

        if let Some(db) = txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::AuthorStats as usize)
        {
            let key = author_id.0;
            if let Some((_, data)) = btree::get(&txn.txn, &db, &key, None)? {
                let stats: AttributionStats = bincode::deserialize(data).map_err(|e| {
                    SanakirjaError::Sanakirja(::sanakirja::Error::IO(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )))
                })?;
                return Ok(Some(stats));
            }
        }

        Ok(None)
    }

    /// Remove attribution for a patch
    pub fn delete_attribution(&self, patch_id: &PatchId) -> Result<(), SanakirjaError> {
        let mut txn = self.pristine.mut_txn_begin()?;
        let key = patch_id.0 .0;

        // Get attribution to find author for cleanup
        let author_id = if let Some(db) = txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::PatchAttribution as usize)
        {
            if let Some((_, data)) = btree::get(&txn.txn, &db, &key, None)? {
                let patch: AttributedPatch = bincode::deserialize(data).map_err(|e| {
                    SanakirjaError::Sanakirja(::sanakirja::Error::IO(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )))
                })?;
                Some(patch.author.id)
            } else {
                None
            }
        } else {
            None
        };

        // Remove from patch attribution
        if let Some(mut db) = txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::PatchAttribution as usize)
        {
            btree::del(&mut txn.txn, &mut db, &key, None)?;
            txn.txn
                .set_root(Root::PatchAttribution as usize, db.db.into());
        }

        // Remove from author patches
        if let (Some(author), Some(mut db)) = (
            author_id,
            txn.txn
                .root_db::<L64, [u8], UP<L64, [u8]>>(Root::AuthorPatches as usize),
        ) {
            let author_key = author.0;

            // Get existing patch list for this author
            if let Some((_, data)) = btree::get(&txn.txn, &db, &author_key, None)? {
                if let Ok(mut patch_list) = bincode::deserialize::<Vec<PatchId>>(data) {
                    // Remove this patch from the list
                    patch_list.retain(|&p| p.0 .0 != key);

                    if patch_list.is_empty() {
                        // Remove the entry entirely if no patches left
                        btree::del(&mut txn.txn, &mut db, &author_key, None)?;
                    } else {
                        // Update with the new list
                        let updated_data = bincode::serialize(&patch_list).map_err(|e| {
                            SanakirjaError::Sanakirja(::sanakirja::Error::IO(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                e.to_string(),
                            )))
                        })?;
                        btree::put(&mut txn.txn, &mut db, &author_key, &updated_data[..])?;
                    }
                }
            }
            txn.txn.set_root(Root::AuthorPatches as usize, db.db.into());
        }

        // Remove AI metadata
        if let Some(mut db) = txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::AIPatchMetadata as usize)
        {
            btree::del(&mut txn.txn, &mut db, &key, None)?;
            txn.txn
                .set_root(Root::AIPatchMetadata as usize, db.db.into());
        }

        txn.commit()?;
        Ok(())
    }

    /// Initialize attribution tables if they don't exist
    pub fn initialize_tables(&self) -> Result<(), SanakirjaError> {
        let mut txn = self.pristine.mut_txn_begin()?;

        // Create patch attribution table if it doesn't exist
        if txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::PatchAttribution as usize)
            .is_none()
        {
            let db: UDb<L64, [u8]> = unsafe { btree::create_db_(&mut txn.txn)? };
            txn.txn
                .set_root(Root::PatchAttribution as usize, db.db.into());
        }

        // Create author patches table if it doesn't exist
        if txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::AuthorPatches as usize)
            .is_none()
        {
            let db: UDb<L64, [u8]> = unsafe { btree::create_db_(&mut txn.txn)? };
            txn.txn.set_root(Root::AuthorPatches as usize, db.db.into());
        }

        // Create AI metadata table if it doesn't exist
        if txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::AIPatchMetadata as usize)
            .is_none()
        {
            let db: UDb<L64, [u8]> = unsafe { btree::create_db_(&mut txn.txn)? };
            txn.txn
                .set_root(Root::AIPatchMetadata as usize, db.db.into());
        }

        // Create author stats table if it doesn't exist
        if txn
            .txn
            .root_db::<L64, [u8], UP<L64, [u8]>>(Root::AuthorStats as usize)
            .is_none()
        {
            let db: UDb<L64, [u8]> = unsafe { btree::create_db_(&mut txn.txn)? };
            txn.txn.set_root(Root::AuthorStats as usize, db.db.into());
        }

        txn.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribution::AuthorInfo;
    use std::collections::HashSet;

    #[test]
    fn test_attribution_store_creation() {
        // This is a basic test to ensure the types compile correctly
        // In a real test environment, we'd need to set up a test database
        let author = AuthorInfo {
            id: AuthorId::new(1),
            name: "Test Author".to_string(),
            email: "test@example.com".to_string(),
            is_ai: false,
        };

        let patch = AttributedPatch {
            patch_id: PatchId::new(crate::pristine::NodeId::ROOT),
            author,
            timestamp: chrono::Utc::now(),
            ai_assisted: false,
            ai_metadata: None,
            description: "Test patch".to_string(),
            dependencies: HashSet::new(),
            conflicts_with: HashSet::new(),
            confidence: None,
        };

        // Just verify the types work
        assert_eq!(
            patch.patch_id,
            PatchId::new(crate::pristine::NodeId::ROOT)
        );
        assert!(!patch.ai_assisted);
    }
}
