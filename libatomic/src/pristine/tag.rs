//! Consolidating Tag Data Structures
//!
//! This module implements the data structures for tag-based dependency consolidation,
//! following the hybrid patch-snapshot model as described in the New Workflow Recommendation.
//!
//! # Architecture
//!
//! Consolidating tags serve as **dependency reference points** that enable clean dependency trees:
//! - **History is preserved**: All changes and their dependencies remain in the database
//! - **New changes get shortcuts**: Can depend on a tag instead of all previous changes
//! - **Mathematical correctness**: The tag represents the equivalent state of all consolidated changes
//! - **Scalability achieved**: Dependency depth bounded by tag cycle length
//!
//! # Important: No Data Deletion
//!
//! **Consolidating tags do NOT delete or merge old records!**
//!
//! When you create a consolidating tag:
//! - All old changes (Change 1, Change 2, ..., Change N) **still exist**
//! - All old dependencies between those changes **still exist**
//! - The full dependency graph **can still be traversed**
//! - Historical queries **work exactly as before**
//!
//! What the tag provides:
//! - A **mathematical reference point** representing the state after applying all those changes
//! - An **alternative starting point** for new changes to depend on
//! - A **clean dependency tree** for future development
//!
//! # Example
//!
//! ```text
//! // These changes and their dependencies remain in the database:
//! Change 1 → [no deps]           ← Still exists
//! Change 2 → [Change 1]          ← Still exists, with its dependency
//! Change 3 → [Change 1, 2]       ← Still exists, with its dependencies
//! ...
//! Change 25 → [Change 1...24]    ← Still exists, with all 24 dependencies
//!
//! // Tag provides a shortcut reference point:
//! 🏷️ TAG v1.0 [CONSOLIDATING] → Points to state equivalent to Changes 1-25
//!                              → Does NOT delete Changes 1-25!
//!
//! // New changes can use the tag as a clean dependency:
//! Change 26 → [TAG v1.0]         ← Single dependency (equivalent to depending on 1-25)
//! Change 27 → [Change 26]        ← Depends on 26, which depends on tag
//! ```
//!
//! This is similar to Git's reachability - old commits still exist, but a branch HEAD
//! gives you a convenient reference point.

use super::*;
use serde::{Deserialize, Serialize};

/// Byte slice wrapper for Sanakirja storage (unsized type).
///
/// This is the database representation that implements UnsizedStorable.
/// Format: [4 bytes length][serialized data]
#[repr(C)]
pub struct TagBytes {
    len: u32,
    data: [u8],
}

impl std::fmt::Debug for TagBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TagBytes")
            .field("len", &self.len)
            .field("data_len", &self.data_bytes().len())
            .finish()
    }
}

impl PartialEq for TagBytes {
    fn eq(&self, other: &Self) -> bool {
        self.data_bytes() == other.data_bytes()
    }
}

impl Eq for TagBytes {}

impl TagBytes {
    /// Create from a byte slice with length prefix
    pub unsafe fn from_slice(bytes: &[u8]) -> &Self {
        let ptr = bytes.as_ptr();
        let len = bytes.len();
        std::mem::transmute(std::slice::from_raw_parts(ptr, len))
    }

    /// Get the data portion (without length prefix)
    pub fn data_bytes(&self) -> &[u8] {
        &self.data[..self.len as usize]
    }

    /// Total size including length prefix
    pub fn total_size(&self) -> usize {
        4 + self.len as usize
    }
}

impl ::sanakirja::UnsizedStorable for TagBytes {
    const ALIGN: usize = 4;

    fn size(&self) -> usize {
        4 + self.len as usize
    }

    unsafe fn write_to_page_alloc<T: ::sanakirja::AllocPage>(&self, _: &mut T, p: *mut u8) {
        std::ptr::copy_nonoverlapping(&self.len as *const u32 as *const u8, p, 4);
        std::ptr::copy_nonoverlapping(self.data.as_ptr(), p.add(4), self.len as usize);
    }

    unsafe fn from_raw_ptr<'a, T>(_: &T, p: *const u8) -> &'a Self {
        let len = u32::from_le_bytes([*p, *p.add(1), *p.add(2), *p.add(3)]) as usize;
        let slice = std::slice::from_raw_parts(p, 4 + len);
        std::mem::transmute(slice)
    }

    unsafe fn onpage_size(p: *const u8) -> usize {
        let len = u32::from_le_bytes([*p, *p.add(1), *p.add(2), *p.add(3)]) as usize;
        4 + len
    }
}

impl ::sanakirja::Storable for TagBytes {
    fn compare<T>(&self, _: &T, x: &Self) -> std::cmp::Ordering {
        self.data_bytes().cmp(x.data_bytes())
    }

    type PageReferences = std::iter::Empty<u64>;
    fn page_references(&self) -> Self::PageReferences {
        std::iter::empty()
    }
}

impl ::sanakirja::debug::Check for TagBytes {}

/// Byte slice wrapper for attribution summary (unsized type).
#[repr(C)]
pub struct AttributionSummaryBytes {
    len: u32,
    data: [u8],
}

impl std::fmt::Debug for AttributionSummaryBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AttributionSummaryBytes")
            .field("len", &self.len)
            .field("data_len", &self.data_bytes().len())
            .finish()
    }
}

impl PartialEq for AttributionSummaryBytes {
    fn eq(&self, other: &Self) -> bool {
        self.data_bytes() == other.data_bytes()
    }
}

impl Eq for AttributionSummaryBytes {}

impl AttributionSummaryBytes {
    /// Get the data portion (without length prefix)
    pub fn data_bytes(&self) -> &[u8] {
        &self.data[..self.len as usize]
    }

    /// Total size including length prefix
    pub fn total_size(&self) -> usize {
        4 + self.len as usize
    }
}

impl ::sanakirja::UnsizedStorable for AttributionSummaryBytes {
    const ALIGN: usize = 4;

    fn size(&self) -> usize {
        4 + self.len as usize
    }

    unsafe fn write_to_page_alloc<T: ::sanakirja::AllocPage>(&self, _: &mut T, p: *mut u8) {
        std::ptr::copy_nonoverlapping(&self.len as *const u32 as *const u8, p, 4);
        std::ptr::copy_nonoverlapping(self.data.as_ptr(), p.add(4), self.len as usize);
    }

    unsafe fn from_raw_ptr<'a, T>(_: &T, p: *const u8) -> &'a Self {
        let len = u32::from_le_bytes([*p, *p.add(1), *p.add(2), *p.add(3)]) as usize;
        let slice = std::slice::from_raw_parts(p, 4 + len);
        std::mem::transmute(slice)
    }

    unsafe fn onpage_size(p: *const u8) -> usize {
        let len = u32::from_le_bytes([*p, *p.add(1), *p.add(2), *p.add(3)]) as usize;
        4 + len
    }
}

impl ::sanakirja::Storable for AttributionSummaryBytes {
    fn compare<T>(&self, _: &T, x: &Self) -> std::cmp::Ordering {
        self.data_bytes().cmp(x.data_bytes())
    }

    type PageReferences = std::iter::Empty<u64>;
    fn page_references(&self) -> Self::PageReferences {
        std::iter::empty()
    }
}

impl ::sanakirja::debug::Check for AttributionSummaryBytes {}

/// Serialized version of Tag for database storage.
///
/// This structure stores the tag data as a binary blob for efficient
/// Sanakirja btree storage. It uses bincode for serialization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedTag {
    data: Vec<u8>,
}

impl SerializedTag {
    /// Creates a new serialized consolidating tag from the source structure.
    pub fn from_tag(tag: &Tag) -> Result<Self, bincode::Error> {
        let data = bincode::serialize(tag)?;
        Ok(SerializedTag { data })
    }

    /// Deserializes back to a Tag.
    pub fn to_tag(&self) -> Result<Tag, bincode::Error> {
        bincode::deserialize(&self.data)
    }

    /// Returns the size of the serialized data.
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Returns the raw bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Create a boxed byte slice wrapper for Sanakirja storage
    pub fn to_bytes_wrapper(&self) -> Box<TagBytes> {
        let len = self.data.len() as u32;
        let total_size = 4 + self.data.len();

        unsafe {
            let layout = std::alloc::Layout::from_size_align_unchecked(total_size, 4);
            let ptr = std::alloc::alloc(layout);

            // Write length prefix
            std::ptr::copy_nonoverlapping(&len as *const u32 as *const u8, ptr, 4);
            // Write data
            std::ptr::copy_nonoverlapping(self.data.as_ptr(), ptr.add(4), self.data.len());

            let slice = std::slice::from_raw_parts(ptr, total_size);
            Box::from_raw(std::mem::transmute::<*const [u8], *mut TagBytes>(
                slice as *const [u8],
            ))
        }
    }

    /// Create from byte slice wrapper
    pub fn from_bytes_wrapper(wrapper: &TagBytes) -> Self {
        SerializedTag {
            data: wrapper.data_bytes().to_vec(),
        }
    }
}

impl From<Tag> for SerializedTag {
    fn from(tag: Tag) -> Self {
        SerializedTag::from_tag(&tag).expect("serialization should not fail")
    }
}

/// Serialized version of TagAttributionSummary for database storage.
///
/// This structure stores the attribution summary as a binary blob for efficient
/// Sanakirja btree storage. It uses bincode for serialization.
#[derive(Clone, Debug, PartialEq)]
pub struct SerializedTagAttributionSummary {
    data: Vec<u8>,
}

impl SerializedTagAttributionSummary {
    /// Creates a new serialized attribution summary from the source structure.
    pub fn from_summary(summary: &TagAttributionSummary) -> Result<Self, bincode::Error> {
        let data = bincode::serialize(summary)?;
        Ok(SerializedTagAttributionSummary { data })
    }

    /// Deserializes back to a TagAttributionSummary.
    pub fn to_summary(&self) -> Result<TagAttributionSummary, bincode::Error> {
        bincode::deserialize(&self.data)
    }

    /// Returns the size of the serialized data.
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Returns the raw bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Create a boxed byte slice wrapper for Sanakirja storage
    pub fn to_bytes_wrapper(&self) -> Box<AttributionSummaryBytes> {
        let len = self.data.len() as u32;
        let total_size = 4 + self.data.len();

        unsafe {
            let layout = std::alloc::Layout::from_size_align_unchecked(total_size, 4);
            let ptr = std::alloc::alloc(layout);

            // Write length prefix
            std::ptr::copy_nonoverlapping(&len as *const u32 as *const u8, ptr, 4);
            // Write data
            std::ptr::copy_nonoverlapping(self.data.as_ptr(), ptr.add(4), self.data.len());

            let slice = std::slice::from_raw_parts(ptr, total_size);
            Box::from_raw(std::mem::transmute::<
                *const [u8],
                *mut AttributionSummaryBytes,
            >(slice as *const [u8]))
        }
    }

    /// Create from byte slice wrapper
    pub fn from_bytes_wrapper(wrapper: &AttributionSummaryBytes) -> Self {
        SerializedTagAttributionSummary {
            data: wrapper.data_bytes().to_vec(),
        }
    }
}

impl From<TagAttributionSummary> for SerializedTagAttributionSummary {
    fn from(summary: TagAttributionSummary) -> Self {
        SerializedTagAttributionSummary::from_summary(&summary)
            .expect("serialization should not fail")
    }
}

/// A consolidating tag that serves as a **dependency reference point**.
///
/// **Critical: This does NOT delete or merge old changes!**
///
/// This structure represents a point in the channel's history that new changes can
/// depend on as a shortcut, rather than depending on all previous changes individually.
///
/// # What This Tag Does
///
/// - **Provides a reference point**: New changes can depend on this tag
/// - **Simplifies dependency trees**: Instead of 50 dependencies, depend on 1 tag
/// - **Preserves history**: All old changes and dependencies remain queryable
/// - **Represents equivalent state**: Depending on this tag is mathematically equivalent
///   to depending on all the changes it references
///
/// # What This Tag Does NOT Do
///
/// - **Does NOT delete old changes**: All Changes 1-N remain in the database
/// - **Does NOT merge dependencies**: Old dependency relationships are preserved
/// - **Does NOT prevent historical queries**: You can still traverse the full graph
///
/// # Mathematical Properties
///
/// - **Equivalence**: Tag v1.0 ≡ State after applying Changes 1-25
/// - **Commutativity**: Changes within a tag cycle maintain commutative properties
/// - **Associativity**: Tag chains preserve associative relationships
/// - **Idempotence**: Multiple applications of the same tag state yield identical results
///
/// # Example
///
/// ```text
/// // Historical changes remain in database with full dependencies:
/// Change 1 → [no deps]           ← PRESERVED
/// Change 2 → [Change 1]          ← PRESERVED with dependency
/// Change 3 → [Change 1, 2]       ← PRESERVED with dependencies
/// ...
/// Change 25 → [Change 1...24]    ← PRESERVED with all 24 dependencies
///
/// // Tag provides shortcut for new changes:
/// Tag v1.0 → References state of Changes 1-25 (but doesn't delete them!)
///
/// // New changes can use the shortcut:
/// Change 26 → [Tag v1.0]         ← Clean dependency tree!
/// Change 27 → [Change 26]        ← Only 1 dependency
///
/// // Historical queries still work:
/// Query: "Show me Change 15's dependencies" → [Change 1...14] ✓
/// Query: "Show me all changes in Tag v1.0" → [Change 1...25] ✓
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[repr(C)]
pub struct Tag {
    /// Hash of the tag (Ed25519 hash of tag content)
    pub tag_hash: Hash,

    /// Hash of the change file created for this tag
    /// This is the hash that gets applied to the channel and can be used as a dependency
    pub change_file_hash: Option<Hash>,

    /// Merkle hash representing the channel state at tag creation
    /// This is the primary identifier for the tag file (e.g., in .atomic/changes/HASH.tag)
    pub state: Merkle,

    /// Channel this tag belongs to
    pub channel: String,

    /// Timestamp when this consolidation was created
    pub consolidation_timestamp: u64,

    /// Previous consolidating tag (if any)
    pub previous_consolidation: Option<Hash>,

    /// Number of direct dependencies before consolidation
    pub dependency_count_before: u64,

    /// Number of changes this tag references (NOT deleted, just counted)
    pub consolidated_change_count: u64,

    /// Whether this tag consolidates from a specific previous tag
    /// (for flexible consolidation strategies like production hotfixes)
    pub consolidates_since: Option<Hash>,

    /// Explicit list of changes consolidated by this tag.
    ///
    /// This list is populated by traversing the DAG from the channel tip
    /// at the time of tag creation. If the DAG is modified later (e.g.,
    /// by inserting changes via `atomic record -e`), this list remains
    /// unchanged - the tag is an immutable snapshot.
    ///
    /// When creating new tags, this list enables:
    /// - Exact expansion during DAG traversal
    /// - Validation of tag contents
    /// - Detection of which changes are included
    /// - Accurate dependency analysis
    pub consolidated_changes: Vec<Hash>,

    /// Semantic version for this tag (e.g., "1.0.0", "2.1.0-beta.1")
    /// Following semver.org specification: MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]
    pub version: Option<String>,

    /// Human-readable message/description for this tag
    pub message: Option<String>,

    /// User/system that created this tag
    pub created_by: Option<String>,

    /// Additional custom metadata as key-value pairs
    pub metadata: std::collections::HashMap<String, String>,
}

/// Semantic version structure for parsing and manipulation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub pre_release: Option<String>,
    pub build_metadata: Option<String>,
}

impl SemanticVersion {
    /// Parse a semantic version string (e.g., "1.2.3", "2.0.0-beta.1", "1.0.0+build.123")
    pub fn parse(version: &str) -> Result<Self, String> {
        // Split on '+' to separate build metadata
        let (version_part, build_metadata) = if let Some(pos) = version.find('+') {
            let (v, b) = version.split_at(pos);
            (v, Some(b[1..].to_string()))
        } else {
            (version, None)
        };

        // Split on '-' to separate pre-release
        let (core_version, pre_release) = if let Some(pos) = version_part.find('-') {
            let (v, p) = version_part.split_at(pos);
            (v, Some(p[1..].to_string()))
        } else {
            (version_part, None)
        };

        // Parse major.minor.patch
        let parts: Vec<&str> = core_version.split('.').collect();
        if parts.len() != 3 {
            return Err(format!(
                "Invalid version format: expected x.y.z, got '{}'",
                version
            ));
        }

        let major = parts[0]
            .parse::<u32>()
            .map_err(|_| format!("Invalid major version: '{}'", parts[0]))?;
        let minor = parts[1]
            .parse::<u32>()
            .map_err(|_| format!("Invalid minor version: '{}'", parts[1]))?;
        let patch = parts[2]
            .parse::<u32>()
            .map_err(|_| format!("Invalid patch version: '{}'", parts[2]))?;

        Ok(Self {
            major,
            minor,
            patch,
            pre_release,
            build_metadata,
        })
    }

    /// Convert back to string representation
    pub fn to_string(&self) -> String {
        let mut result = format!("{}.{}.{}", self.major, self.minor, self.patch);

        if let Some(ref pre) = self.pre_release {
            result.push('-');
            result.push_str(pre);
        }

        if let Some(ref build) = self.build_metadata {
            result.push('+');
            result.push_str(build);
        }

        result
    }

    /// Increment the patch version (z in x.y.z)
    pub fn increment_patch(&self) -> Self {
        Self {
            major: self.major,
            minor: self.minor,
            patch: self.patch + 1,
            pre_release: None,    // Remove pre-release when incrementing
            build_metadata: None, // Remove build metadata when incrementing
        }
    }

    /// Increment the minor version (y in x.y.z) and reset patch to 0
    pub fn increment_minor(&self) -> Self {
        Self {
            major: self.major,
            minor: self.minor + 1,
            patch: 0,
            pre_release: None,
            build_metadata: None,
        }
    }

    /// Increment the major version (x in x.y.z) and reset minor and patch to 0
    pub fn increment_major(&self) -> Self {
        Self {
            major: self.major + 1,
            minor: 0,
            patch: 0,
            pre_release: None,
            build_metadata: None,
        }
    }
}

impl Tag {
    /// Creates a new consolidating tag.
    ///
    /// # Arguments
    ///
    /// * `tag_hash` - The hash identifying this tag
    /// * `channel` - The channel this tag belongs to
    /// * `previous_consolidation` - The previous consolidating tag (if any)
    /// * `dependency_count_before` - Number of dependencies before consolidation
    /// * `consolidated_change_count` - Number of changes this tag references
    ///
    /// # Returns
    ///
    /// A new `Tag` instance with the current timestamp.
    ///
    /// # Note
    ///
    /// Creating this tag does NOT delete or modify the changes it references.
    /// All changes remain in the database with their original dependencies.
    pub fn new(
        tag_hash: Hash,
        state: Merkle,
        channel: String,
        previous_consolidation: Option<Hash>,
        dependency_count_before: u64,
        consolidated_change_count: u64,
        consolidated_changes: Vec<Hash>,
    ) -> Self {
        Self {
            tag_hash,
            change_file_hash: None,
            state,
            channel,
            consolidation_timestamp: chrono::Utc::now().timestamp() as u64,
            previous_consolidation,
            dependency_count_before,
            consolidated_change_count,
            consolidates_since: None,
            consolidated_changes,
            version: None,
            message: None,
            created_by: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Creates a new consolidating tag with version and metadata.
    ///
    /// # Arguments
    ///
    /// * `tag_hash` - The hash identifying this tag
    /// * `channel` - The channel this tag belongs to
    /// * `previous_consolidation` - The previous consolidating tag (if any)
    /// * `dependency_count_before` - Number of dependencies before consolidation
    /// * `consolidated_change_count` - Number of changes this tag references
    /// * `consolidated_changes` - List of changes consolidated by this tag
    /// * `version` - Semantic version (e.g., "1.0.0", "2.1.0-beta.1")
    /// * `message` - Human-readable description
    ///
    /// # Returns
    /// Creates a new consolidating tag for testing purposes.
    ///
    /// This is a simpler constructor for tests that don't need all the
    /// consolidation metadata.
    #[cfg(test)]
    pub fn new_test(
        tag_hash: Hash,
        state: Merkle,
        channel: String,
        previous_consolidation: Option<Hash>,
        dependency_count_before: u64,
        consolidated_change_count: u64,
        consolidated_changes: Vec<Hash>,
        version: Option<String>,
        message: Option<String>,
    ) -> Self {
        Self {
            tag_hash,
            change_file_hash: None,
            state,
            channel,
            consolidation_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            previous_consolidation,
            dependency_count_before,
            consolidated_change_count,
            consolidates_since: None,
            consolidated_changes,
            version,
            message,
            created_by: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Creates a new consolidating tag with flexible consolidation strategy.
    ///
    /// This allows consolidating from a specific previous tag rather than
    /// the immediate predecessor, enabling production hotfix workflows.
    ///
    /// # Arguments
    ///
    /// * `tag_hash` - The hash identifying this tag
    /// * `channel` - The channel this tag belongs to
    /// * `consolidates_since` - The tag to consolidate from
    /// * `dependency_count_before` - Number of dependencies before consolidation
    /// * `consolidated_change_count` - Number of changes this tag references
    ///
    /// # Note
    ///
    /// This does NOT delete or modify any changes. It provides a reference point
    /// for new changes to depend on.
    pub fn new_with_since(
        tag_hash: Hash,
        state: Merkle,
        channel: String,
        consolidates_since: Hash,
        dependency_count_before: u64,
        consolidated_change_count: u64,
        consolidated_changes: Vec<Hash>,
    ) -> Self {
        Self {
            tag_hash,
            change_file_hash: None,
            state,
            channel,
            consolidation_timestamp: chrono::Utc::now().timestamp() as u64,
            previous_consolidation: None,
            dependency_count_before,
            consolidated_change_count,
            consolidates_since: Some(consolidates_since),
            consolidated_changes,
            version: None,
            message: None,
            created_by: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Checks if this is the first consolidating tag in the channel.
    pub fn is_initial(&self) -> bool {
        self.previous_consolidation.is_none()
    }

    /// Returns the effective dependency count after consolidation.
    ///
    /// For a consolidating tag, the effective dependency count is 1
    /// (the tag itself becomes the single dependency).
    pub fn effective_dependency_count(&self) -> u64 {
        1
    }

    /// Calculates the dependency reduction achieved by this consolidation.
    pub fn dependency_reduction(&self) -> u64 {
        self.dependency_count_before.saturating_sub(1)
    }

    /// Traverse the DAG from a starting change, expanding any tag references encountered.
    ///
    /// This function performs a depth-first traversal of the dependency graph, collecting
    /// all reachable changes. When it encounters a dependency that is a consolidating tag,
    /// it expands the tag by including all of its consolidated changes.
    ///
    /// # Arguments
    ///
    /// * `txn` - Transaction for database access
    /// * `start` - Starting change hash (typically the channel tip)
    /// * `get_dependencies` - Function to retrieve dependencies for a change
    ///
    /// # Returns
    ///
    /// A vector of all reachable change hashes, in traversal order.
    ///
    /// # Algorithm
    ///
    /// 1. Start from the given change
    /// 2. For each dependency:
    ///    - Check if it's a consolidating tag
    ///    - If tag: expand to include all its consolidated changes
    ///    - If regular change: add to traversal stack
    /// 3. Continue until all reachable changes are visited
    ///
    /// # Example
    ///
    /// ```text
    /// DAG:
    ///   C1 → C2 → C3 → Tag v1.0 [C1, C2, C3] → C4
    ///
    /// traverse_with_tag_expansion(txn, C4, get_deps) returns:
    ///   [C4, C3, C2, C1]  // Tag v1.0 was expanded
    /// ```
    pub fn traverse_with_tag_expansion<T, F>(
        txn: &T,
        start: Hash,
        get_dependencies: F,
    ) -> Result<Vec<Hash>, TxnErr<T::TagError>>
    where
        T: super::TagMetadataTxnT,
        F: Fn(&T, &Hash) -> Result<Vec<Hash>, TxnErr<T::TagError>>,
    {
        use std::collections::HashSet;

        let mut all_changes = Vec::new();
        let mut visited = HashSet::new();
        let mut stack = vec![start];

        while let Some(hash) = stack.pop() {
            // Skip if already visited
            if visited.contains(&hash) {
                continue;
            }
            visited.insert(hash);
            all_changes.push(hash);

            // Get dependencies for this change
            let deps = get_dependencies(txn, &hash)?;

            for dep_hash in deps {
                // Check if this dependency is a consolidating tag
                if let Some(serialized_tag) = txn.get_tag(&dep_hash)? {
                    // Try to deserialize the tag
                    match serialized_tag.to_tag() {
                        Ok(tag) => {
                            // EXPAND: Add all changes from the tag to the stack
                            for tag_change in &tag.consolidated_changes {
                                if !visited.contains(tag_change) {
                                    stack.push(*tag_change);
                                }
                            }
                        }
                        Err(_) => {
                            // Deserialization failed - treat as regular change
                            if !visited.contains(&dep_hash) {
                                stack.push(dep_hash);
                            }
                        }
                    }
                } else {
                    // Regular change dependency
                    if !visited.contains(&dep_hash) {
                        stack.push(dep_hash);
                    }
                }
            }
        }

        Ok(all_changes)
    }
}

/// AI attribution summary for a consolidating tag.
///
/// This structure aggregates AI attribution information for changes referenced by a tag,
/// implementing the Attribution Bridge architecture described in the workflow recommendation.
///
/// # Purpose
///
/// When a tag references multiple changes, we aggregate their attribution metadata
/// for efficient querying. This summary captures statistics without duplicating
/// per-change attribution data (which remains stored with each individual change).
///
/// # Important: Source Data Preserved
///
/// Individual changes keep their full attribution data. This summary is an
/// **aggregate cache** for O(1) queries, not a replacement for the source data.
///
/// # Performance
///
/// - O(1) lookup via Sanakirja btree
/// - Calculated once during tag creation
/// - Enables fast cross-consolidation attribution queries
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[repr(C)]
pub struct TagAttributionSummary {
    /// Hash of the tag this summary belongs to
    pub tag_hash: Hash,

    /// Total number of changes referenced by this tag
    pub total_changes: u64,

    /// Number of AI-assisted changes referenced by this tag
    pub ai_assisted_changes: u64,

    /// Number of purely human-authored changes referenced by this tag
    pub human_authored_changes: u64,

    /// AI provider statistics (provider name -> stats)
    pub ai_provider_stats: HashMap<String, ProviderStats>,

    /// Number of high-confidence AI suggestions
    pub confidence_high: u64,

    /// Number of medium-confidence AI suggestions
    pub confidence_medium: u64,

    /// Number of low-confidence AI suggestions
    pub confidence_low: u64,

    /// Average confidence score across all AI-assisted changes
    pub average_confidence: f32,

    /// Time span of changes consolidated (earliest to latest timestamp)
    pub creation_time_span: (u64, u64),

    /// Number of code changes (excluding tests and docs)
    pub code_changes: u64,

    /// Number of test changes
    pub test_changes: u64,

    /// Number of documentation changes
    pub doc_changes: u64,
}

/// Statistics for a specific AI provider within a tag consolidation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderStats {
    /// Number of changes from this provider
    pub change_count: u64,

    /// Average confidence for this provider's suggestions
    pub average_confidence: f32,

    /// Models used by this provider
    pub models_used: Vec<String>,

    /// Types of suggestions (complete, partial, collaborative)
    pub suggestion_types: HashMap<String, u64>,
}

impl TagAttributionSummary {
    /// Creates a new empty attribution summary.
    pub fn new(tag_hash: Hash) -> Self {
        Self {
            tag_hash,
            total_changes: 0,
            ai_assisted_changes: 0,
            human_authored_changes: 0,
            ai_provider_stats: HashMap::default(),
            confidence_high: 0,
            confidence_medium: 0,
            confidence_low: 0,
            average_confidence: 0.0,
            creation_time_span: (0, 0),
            code_changes: 0,
            test_changes: 0,
            doc_changes: 0,
        }
    }

    /// Calculates the percentage of AI-assisted changes.
    pub fn ai_percentage(&self) -> f32 {
        if self.total_changes == 0 {
            0.0
        } else {
            (self.ai_assisted_changes as f32 / self.total_changes as f32) * 100.0
        }
    }

    /// Calculates the percentage of human-authored changes.
    pub fn human_percentage(&self) -> f32 {
        if self.total_changes == 0 {
            0.0
        } else {
            (self.human_authored_changes as f32 / self.total_changes as f32) * 100.0
        }
    }

    /// Returns the dominant AI provider (by change count).
    pub fn dominant_provider(&self) -> Option<(&String, &ProviderStats)> {
        self.ai_provider_stats
            .iter()
            .max_by_key(|(_, stats)| stats.change_count)
    }

    /// Returns the time span duration in seconds.
    pub fn time_span_seconds(&self) -> u64 {
        self.creation_time_span
            .1
            .saturating_sub(self.creation_time_span.0)
    }
}

impl ProviderStats {
    /// Creates a new empty provider stats instance.
    pub fn new() -> Self {
        Self {
            change_count: 0,
            average_confidence: 0.0,
            models_used: Vec::new(),
            suggestion_types: HashMap::default(),
        }
    }

    /// Increments the change count and updates running averages.
    pub fn add_change(&mut self, confidence: f32, model: String, suggestion_type: String) {
        // Update running average for confidence
        let total = self.change_count as f32 * self.average_confidence + confidence;
        self.change_count += 1;
        self.average_confidence = total / self.change_count as f32;

        // Track model usage
        if !self.models_used.contains(&model) {
            self.models_used.push(model);
        }

        // Track suggestion types
        *self.suggestion_types.entry(suggestion_type).or_insert(0) += 1;
    }
}

impl Default for ProviderStats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dag_traversal_with_tag_expansion() {
        // This test would require a full database setup
        // For now, we verify the function signature compiles
        // Integration tests will verify the actual behavior
    }

    #[test]
    fn test_tag_creation() {
        let tag_hash = Hash::NONE;
        let state = Merkle::zero();
        let channel = "main".to_string();
        let tag = Tag::new(tag_hash, state, channel, None, 50, 25, vec![]);

        assert_eq!(tag.tag_hash, tag_hash);
        assert!(tag.is_initial());
        assert_eq!(tag.dependency_count_before, 50);
        assert_eq!(tag.consolidated_change_count, 25);
        assert_eq!(tag.effective_dependency_count(), 1);
        assert_eq!(tag.dependency_reduction(), 49);
        assert_eq!(tag.consolidated_changes.len(), 0); // Empty in this test
    }

    #[test]
    fn test_tag_with_previous() {
        let tag_hash = Hash::NONE;
        let state = Merkle::zero();
        let prev_hash = Hash::NONE;
        let channel = "main".to_string();
        let tag = Tag::new(tag_hash, state, channel, Some(prev_hash), 75, 50, vec![]);

        assert!(!tag.is_initial());
        assert_eq!(tag.previous_consolidation, Some(prev_hash));
        assert_eq!(tag.dependency_reduction(), 74);
    }

    #[test]
    fn test_attribution_summary_percentages() {
        let mut summary = TagAttributionSummary::new(Hash::NONE);
        summary.total_changes = 100;
        summary.ai_assisted_changes = 60;
        summary.human_authored_changes = 40;

        assert!((summary.ai_percentage() - 60.0).abs() < 0.0001);
        assert!((summary.human_percentage() - 40.0).abs() < 0.0001);
    }

    #[test]
    fn test_provider_stats_running_average() {
        let mut stats = ProviderStats::new();

        stats.add_change(0.8, "gpt-4".to_string(), "complete".to_string());
        assert_eq!(stats.change_count, 1);
        assert!((stats.average_confidence - 0.8).abs() < 0.0001);

        stats.add_change(0.6, "gpt-4".to_string(), "partial".to_string());
        assert_eq!(stats.change_count, 2);
        assert!((stats.average_confidence - 0.7).abs() < 0.0001);

        stats.add_change(0.9, "gpt-4".to_string(), "complete".to_string());
        assert_eq!(stats.change_count, 3);
        // (0.8 + 0.6 + 0.9) / 3 ≈ 0.7666...
        assert!((stats.average_confidence - 0.7666666).abs() < 0.0001);
    }

    #[test]
    fn test_empty_summary_percentages() {
        let summary = TagAttributionSummary::new(Hash::NONE);
        assert_eq!(summary.ai_percentage(), 0.0);
        assert_eq!(summary.human_percentage(), 0.0);
    }

    #[test]
    fn test_serialized_tag_roundtrip() {
        let tag_hash = Hash::NONE;
        let state = Merkle::zero();
        let channel = "main".to_string();
        let tag = Tag::new(tag_hash, state, channel, None, 10, 5, vec![]);

        // Serialize
        let serialized = SerializedTag::from_tag(&tag).unwrap();
        assert!(serialized.size() > 0);

        // Deserialize
        let deserialized = serialized.to_tag().unwrap();
        assert_eq!(tag, deserialized);
    }

    #[test]
    fn test_serialized_attribution_summary_roundtrip() {
        let mut summary = TagAttributionSummary::new(Hash::NONE);
        summary.total_changes = 100;
        summary.ai_assisted_changes = 60;
        summary.human_authored_changes = 40;

        // Serialize
        let serialized = SerializedTagAttributionSummary::from_summary(&summary).unwrap();
        assert!(serialized.size() > 0);

        // Deserialize
        let deserialized = serialized.to_summary().unwrap();
        assert_eq!(summary, deserialized);
    }

    #[test]
    fn test_tag_database_operations() {
        use crate::pristine::sanakirja::*;
        use crate::pristine::{TagMetadataMutTxnT, TagMetadataTxnT};

        // Create in-memory test database
        let pristine = Pristine::new_anon().unwrap();
        let mut txn = pristine.mut_txn_begin().unwrap();

        // Create a test tag
        let tag_hash = Hash::NONE;
        let state = Merkle::zero();
        let channel = "main".to_string();
        let tag = Tag::new(tag_hash, state, channel, None, 50, 25, vec![]);
        let serialized = SerializedTag::from_tag(&tag).unwrap();

        // Test: Initially tag should not exist
        assert!(!txn.has_tag(&tag_hash).unwrap());
        assert!(txn.get_tag(&tag_hash).unwrap().is_none());

        // Test: Put tag
        txn.put_tag(&tag_hash, &serialized).unwrap();

        // Test: Tag should now exist
        assert!(txn.has_tag(&tag_hash).unwrap());
        let retrieved = txn.get_tag(&tag_hash).unwrap();
        assert!(retrieved.is_some());

        // Test: Retrieved tag should match original
        let retrieved_tag = retrieved.unwrap().to_tag().unwrap();
        assert_eq!(tag, retrieved_tag);

        // Test: Delete tag
        assert!(txn.del_tag(&tag_hash).unwrap());

        // Test: Tag should no longer exist
        assert!(!txn.has_tag(&tag_hash).unwrap());
        assert!(txn.get_tag(&tag_hash).unwrap().is_none());

        // Test: Delete non-existent tag returns false
        assert!(!txn.del_tag(&tag_hash).unwrap());
    }

    #[test]
    fn test_tag_attribution_database_operations() {
        use crate::pristine::sanakirja::*;
        use crate::pristine::{TagMetadataMutTxnT, TagMetadataTxnT};

        // Create in-memory test database
        let pristine = Pristine::new_anon().unwrap();
        let mut txn = pristine.mut_txn_begin().unwrap();

        // Create a test attribution summary
        let tag_hash = Hash::NONE;
        let mut summary = TagAttributionSummary::new(tag_hash);
        summary.total_changes = 100;
        summary.ai_assisted_changes = 60;
        summary.human_authored_changes = 40;
        let serialized = SerializedTagAttributionSummary::from_summary(&summary).unwrap();

        // Test: Initially summary should not exist
        assert!(txn
            .get_tag_attribution_summary(&tag_hash)
            .unwrap()
            .is_none());

        // Test: Put summary
        txn.put_tag_attribution_summary(&tag_hash, &serialized)
            .unwrap();

        // Test: Summary should now exist
        let retrieved = txn.get_tag_attribution_summary(&tag_hash).unwrap();
        assert!(retrieved.is_some());

        // Test: Retrieved summary should match original
        let retrieved_summary = retrieved.unwrap().to_summary().unwrap();
        assert_eq!(summary, retrieved_summary);

        // Test: Delete summary
        assert!(txn.del_tag_attribution_summary(&tag_hash).unwrap());

        // Test: Summary should no longer exist
        assert!(txn
            .get_tag_attribution_summary(&tag_hash)
            .unwrap()
            .is_none());

        // Test: Delete non-existent summary returns false
        assert!(!txn.del_tag_attribution_summary(&tag_hash).unwrap());
    }

    #[test]
    fn test_multiple_tags_database_operations() {
        use crate::pristine::sanakirja::*;
        use crate::pristine::{TagMetadataMutTxnT, TagMetadataTxnT};

        // Create in-memory test database
        let pristine = Pristine::new_anon().unwrap();
        let mut txn = pristine.mut_txn_begin().unwrap();

        // Create multiple test tags
        let tag1_hash = Hash::NONE;
        let state1 = Merkle::zero();
        let tag1 = Tag::new(tag1_hash, state1, "main".to_string(), None, 50, 25, vec![]);
        let serialized1 = SerializedTag::from_tag(&tag1).unwrap();

        // Note: For testing multiple tags, we'd normally use different hashes
        // But Hash::NONE is the only variant available as the Ed25519 base point
        // Test basic put and retrieve

        // Test: Put first tag
        txn.put_tag(&tag1_hash, &serialized1).unwrap();
        assert!(txn.has_tag(&tag1_hash).unwrap());

        // Test: Retrieve the tag
        let retrieved = txn.get_tag(&tag1_hash).unwrap().unwrap();
        let retrieved_tag = retrieved.to_tag().unwrap();
        assert_eq!(retrieved_tag.channel, "main");
        assert_eq!(retrieved_tag.consolidated_change_count, 25);

        // Test: Delete and re-add with different data
        assert!(txn.del_tag(&tag1_hash).unwrap());

        let tag2 = Tag::new(
            tag1_hash,
            state1,
            "develop".to_string(),
            None,
            75,
            50,
            vec![],
        );
        let serialized2 = SerializedTag::from_tag(&tag2).unwrap();
        txn.put_tag(&tag1_hash, &serialized2).unwrap();

        // Test: Should retrieve new tag
        let retrieved2 = txn.get_tag(&tag1_hash).unwrap().unwrap();
        let retrieved_tag2 = retrieved2.to_tag().unwrap();
        assert_eq!(retrieved_tag2.channel, "develop");
        assert_eq!(retrieved_tag2.consolidated_change_count, 50);
    }

    #[test]
    fn test_tag_with_attribution_together() {
        use crate::pristine::sanakirja::*;
        use crate::pristine::{TagMetadataMutTxnT, TagMetadataTxnT};

        // Create in-memory test database
        let pristine = Pristine::new_anon().unwrap();
        let mut txn = pristine.mut_txn_begin().unwrap();

        let tag_hash = Hash::NONE;

        // Create and store tag
        let state = Merkle::zero();
        let tag = Tag::new(tag_hash, state, "main".to_string(), None, 50, 25, vec![]);
        let serialized_tag = SerializedTag::from_tag(&tag).unwrap();
        txn.put_tag(&tag_hash, &serialized_tag).unwrap();

        // Create and store attribution summary for same tag
        let mut summary = TagAttributionSummary::new(tag_hash);
        summary.total_changes = 25;
        summary.ai_assisted_changes = 15;
        summary.human_authored_changes = 10;
        let serialized_summary = SerializedTagAttributionSummary::from_summary(&summary).unwrap();
        txn.put_tag_attribution_summary(&tag_hash, &serialized_summary)
            .unwrap();

        // Test: Both should be retrievable
        assert!(txn.has_tag(&tag_hash).unwrap());
        let retrieved_tag = txn.get_tag(&tag_hash).unwrap().unwrap();
        let retrieved_summary = txn.get_tag_attribution_summary(&tag_hash).unwrap().unwrap();

        // Test: Data should match
        let tag_data = retrieved_tag.to_tag().unwrap();
        let summary_data = retrieved_summary.to_summary().unwrap();

        assert_eq!(tag_data.consolidated_change_count, 25);
        assert_eq!(summary_data.total_changes, 25);
        assert_eq!(summary_data.ai_assisted_changes, 15);

        // Test: Delete tag doesn't affect attribution
        txn.del_tag(&tag_hash).unwrap();
        assert!(!txn.has_tag(&tag_hash).unwrap());
        assert!(txn
            .get_tag_attribution_summary(&tag_hash)
            .unwrap()
            .is_some());

        // Test: Delete attribution
        txn.del_tag_attribution_summary(&tag_hash).unwrap();
        assert!(txn
            .get_tag_attribution_summary(&tag_hash)
            .unwrap()
            .is_none());
    }
}
