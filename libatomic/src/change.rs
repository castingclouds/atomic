use crate::HashSet;
use std::collections::BTreeSet;

use crate::pristine::*;
use crate::text_encoding::Encoding;
use chrono::{DateTime, Utc};

#[cfg(feature = "zstd")]
use std::io::Write;

#[cfg(feature = "text-changes")]
mod parse;
#[cfg(feature = "text-changes")]
mod printable;
#[cfg(feature = "text-changes")]
mod text_changes;
#[cfg(feature = "text-changes")]
pub use parse::*; // for testing
#[cfg(feature = "text-changes")]
pub use printable::*; // for testing
#[cfg(feature = "text-changes")]
pub use text_changes::{TextDeError, TextSerError, WriteChangeLine};

#[cfg(feature = "zstd")]
mod change_file;

#[cfg(feature = "zstd")]
pub use change_file::*;

pub mod noenc;

#[derive(Debug, Error)]
pub enum ChangeError {
    #[error("Version mismatch: got {}", got)]
    VersionMismatch { got: u64 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("while retrieving {:?}: {}", hash, err)]
    IoHash {
        err: std::io::Error,
        hash: crate::pristine::Hash,
    },
    #[error(transparent)]
    Bincode(#[from] bincode::Error),

    #[cfg(feature = "zstd")]
    #[error(transparent)]
    Zstd(#[from] zstd_seekable::Error),

    #[error(transparent)]
    TomlDe(#[from] toml::de::Error),
    #[error(transparent)]
    TomlSer(#[from] toml::ser::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("Missing contents for change {:?}", hash)]
    MissingContents { hash: crate::pristine::Hash },
    #[error("Change hash mismatch, claimed {:?}, computed {:?}", claimed, computed)]
    ChangeHashMismatch {
        claimed: crate::pristine::Hash,
        computed: crate::pristine::Hash,
    },
    #[error(
        "Change contents hash mismatch, claimed {:?}, computed {:?}",
        claimed,
        computed
    )]
    ContentsHashMismatch {
        claimed: crate::pristine::Hash,
        computed: crate::pristine::Hash,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum Atom<Change> {
    NewVertex(NewVertex<Change>),
    EdgeMap(EdgeMap<Change>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewVertex<Change> {
    pub up_context: Vec<Position<Change>>,
    pub down_context: Vec<Position<Change>>,
    pub flag: EdgeFlags,
    pub start: ChangePosition,
    pub end: ChangePosition,
    pub inode: Position<Change>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EdgeMap<Change> {
    pub edges: Vec<NewEdge<Change>>,
    pub inode: Position<Change>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewEdge<Change> {
    pub previous: EdgeFlags,
    pub flag: EdgeFlags,
    /// The origin of the edge, i.e. if a vertex split is needed, the
    /// left-hand side of the split will include `from.pos`. This
    /// means that splitting vertex `[a, b[` to apply this edge
    /// modification will yield vertices `[a, from.pos+1[` and
    /// `[from.pos+1, b[`.
    pub from: Position<Change>,
    /// The destination of the edge, i.e. the last byte affected by
    /// this change.
    pub to: Vertex<Change>,
    /// The change that introduced the previous version of the edge
    /// (the one being replaced by this `NewEdge`).
    pub introduced_by: Change,
}

impl<T: Clone> NewEdge<T> {
    pub(crate) fn reverse(&self, introduced_by: T) -> Self {
        NewEdge {
            previous: self.flag,
            flag: self.previous,
            from: self.from.clone(),
            to: self.to.clone(),
            introduced_by,
        }
    }
}

/// The header of a change contains all the metadata about a change
/// (but not the actual contents of a change).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ChangeHeader_<Author> {
    pub message: String,
    pub description: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub authors: Vec<Author>,
}

/// The header of a change contains all the metadata about a change
/// (but not the actual contents of a change).
pub type ChangeHeader = ChangeHeader_<Author>;

impl Default for ChangeHeader {
    fn default() -> Self {
        ChangeHeader {
            message: String::new(),
            description: None,
            timestamp: Utc::now(),
            authors: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocalChange<Hunk, Author> {
    pub offsets: Offsets,
    pub hashed: Hashed<Hunk, Author>,
    /// unhashed TOML extra contents.
    pub unhashed: Option<serde_json::Value>,
    /// The contents.
    pub contents: Vec<u8>,
}

impl std::ops::Deref for LocalChange<Hunk<Option<Hash>, Local>, Author> {
    type Target = Hashed<Hunk<Option<Hash>, Local>, Author>;
    fn deref(&self) -> &Self::Target {
        &self.hashed
    }
}

impl std::ops::DerefMut for LocalChange<Hunk<Option<Hash>, Local>, Author> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.hashed
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Author(pub std::collections::BTreeMap<String, String>);

// Beware of changes in the version, tags also use that.
// VERSION 6: Without tag field
// VERSION 7: Current version - Added tag field for tag change serialization
pub const VERSION: u64 = 7;
pub const VERSION_NOENC: u64 = 4;

/// Lightweight metadata about a consolidating tag for serialization in change files.
/// This is a subset of the full `ConsolidatingTag` structure optimized for file storage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TagMetadata {
    /// Semantic version for this tag (e.g., "1.0.0", "2.1.0-beta.1")
    pub version: Option<String>,

    /// Channel this tag belongs to
    pub channel: String,

    /// Number of changes this tag consolidates
    pub consolidated_change_count: u64,

    /// Number of direct dependencies before consolidation
    pub dependency_count_before: u64,

    /// Explicit list of changes consolidated by this tag
    pub consolidated_changes: Vec<Hash>,

    /// Previous consolidating tag (if any)
    pub previous_consolidation: Option<Hash>,

    /// Whether this tag consolidates from a specific previous tag
    pub consolidates_since: Option<Hash>,

    /// User/system that created this tag
    pub created_by: Option<String>,

    /// Additional custom metadata as key-value pairs
    pub metadata: std::collections::HashMap<String, String>,
}

impl TagMetadata {
    /// Creates metadata from a full ConsolidatingTag
    pub fn from_tag(tag: &crate::pristine::Tag) -> Self {
        Self {
            version: tag.version.clone(),
            channel: tag.channel.clone(),
            consolidated_change_count: tag.consolidated_change_count,
            dependency_count_before: tag.dependency_count_before,
            consolidated_changes: tag.consolidated_changes.clone(),
            previous_consolidation: tag.previous_consolidation,
            consolidates_since: tag.consolidates_since,
            created_by: tag.created_by.clone(),
            metadata: tag.metadata.clone(),
        }
    }

    /// Converts metadata back to a full ConsolidatingTag
    ///
    /// This is used when applying a change with consolidating tag metadata
    /// to populate the database with the tag information.
    pub fn to_tag(&self, tag_hash: crate::pristine::Hash) -> crate::pristine::Tag {
        crate::pristine::Tag {
            tag_hash,
            change_file_hash: Some(tag_hash), // The change hash is the tag's change file hash
            state: crate::pristine::Merkle::zero(), // State will be set when tag is created
            channel: self.channel.clone(),
            consolidation_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            previous_consolidation: self.previous_consolidation,
            dependency_count_before: self.dependency_count_before,
            consolidated_change_count: self.consolidated_change_count,
            consolidates_since: self.consolidates_since,
            consolidated_changes: self.consolidated_changes.clone(),
            version: self.version.clone(),
            message: None, // Message is in the change header, not duplicated here
            created_by: self.created_by.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

/// VERSION 6 format of Hashed struct (without tag field)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct HashedV6<Hunk, Author> {
    pub version: u64,
    pub header: ChangeHeader_<Author>,
    pub dependencies: Vec<Hash>,
    pub extra_known: Vec<Hash>,
    pub metadata: Vec<u8>,
    pub changes: Vec<Hunk>,
    pub contents_hash: Hash,
}

impl<Hunk, Author> From<HashedV6<Hunk, Author>> for Hashed<Hunk, Author> {
    fn from(v6: HashedV6<Hunk, Author>) -> Self {
        Self {
            version: v6.version,
            header: v6.header,
            dependencies: v6.dependencies,
            extra_known: v6.extra_known,
            metadata: v6.metadata,
            changes: v6.changes,
            contents_hash: v6.contents_hash,
            tag: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hashed<Hunk, Author> {
    /// Version, again (in order to hash it).
    pub version: u64,
    /// Header part, containing the metadata.
    pub header: ChangeHeader_<Author>,
    /// The dependencies of this change.
    pub dependencies: Vec<Hash>,
    /// Extra known "context" changes to recover from deleted contexts.
    pub extra_known: Vec<Hash>,
    /// Some space to write application-specific data.
    pub metadata: Vec<u8>,
    /// The changes, without the contents.
    pub changes: Vec<Hunk>,
    /// Hash of the contents, so that the "contents" field is
    /// verifiable independently from the actions in this change.
    pub contents_hash: Hash, // TODO: in future format changes, put this field at the beginning.
    /// Consolidating tag metadata (only present for tag changes)
    pub tag: Option<TagMetadata>,
}

pub type Change = LocalChange<Hunk<Option<Hash>, Local>, Author>;

pub fn dependencies<
    'a,
    Local: 'a,
    I: Iterator<Item = &'a Hunk<Option<Hash>, Local>>,
    T: ChannelTxnT
        + DepsTxnT<DepsError = <T as GraphTxnT>::GraphError>
        + TagMetadataTxnT<TagError = <T as GraphTxnT>::GraphError>,
>(
    txn: &T,
    channel: &T::Channel,
    changes: I,
    add_channel_tip: bool,
) -> Result<(Vec<Hash>, Vec<Hash>), MakeChangeError<T>> {
    let mut deps = BTreeSet::new();
    let mut zombie_deps = BTreeSet::new();
    for ch in changes.flat_map(|r| r.iter()) {
        match *ch {
            Atom::NewVertex(NewVertex {
                ref up_context,
                ref down_context,
                ..
            }) => {
                for up in up_context.iter().chain(down_context.iter()) {
                    match up.change {
                        None => {}
                        Some(h) if h.is_none() => {}
                        Some(ref dep) => {
                            deps.insert(*dep);
                        }
                    }
                }
            }
            Atom::EdgeMap(EdgeMap { ref edges, .. }) => {
                for e in edges {
                    assert!(!e.flag.contains(EdgeFlags::PARENT));
                    assert!(e.introduced_by != Some(Hash::NONE));
                    if let Some(p) = e.from.change {
                        deps.insert(p);
                    }
                    if let Some(p) = e.introduced_by {
                        deps.insert(p);
                    }
                    if let Some(p) = e.to.change {
                        deps.insert(p);
                    }
                    add_zombie_deps_from(txn, txn.graph(channel), &mut zombie_deps, e.from)?;
                    add_zombie_deps_to(txn, txn.graph(channel), &mut zombie_deps, e.to)?
                }
            }
        }
    }

    // Include the channel tip as a dependency to capture tag changes when recording new changes
    // Tags don't modify files, so they won't be picked up by hunk analysis
    // Only do this when explicitly requested (i.e., when creating new changes, not reading existing ones)
    if add_channel_tip {
        if let Some(tip_entry) = crate::pristine::changeid_rev_log(txn, channel, None)?.next() {
            let (_, pair) = tip_entry?;
            let changeid = pair.a;
            if let Some(serialized_hash) = txn.get_external(&changeid)? {
                let tip_hash: Hash = serialized_hash.into();
                deps.insert(tip_hash);
                debug!(
                    "dependencies: added channel tip {} as dependency",
                    tip_hash.to_base32()
                );
            }
        }
    }

    let deps = minimize_deps(txn, &channel, &deps)?;
    let (deps, consolidated) = replace_deps_with_tags(txn, &channel, deps)?;
    // Add consolidated changes to zombie_deps (extra_known) since hunks may reference them
    for c in consolidated.iter() {
        zombie_deps.insert(*c);
    }
    for d in deps.iter() {
        zombie_deps.remove(d);
    }
    let mut deps: Vec<Hash> = deps.into_iter().collect();
    deps.sort_by(|a, b| {
        let a_int = match txn.get_internal(&a.into()) {
            Ok(Some(id)) => id,
            _ => return std::cmp::Ordering::Equal,
        };
        let b_int = match txn.get_internal(&b.into()) {
            Ok(Some(id)) => id,
            _ => return std::cmp::Ordering::Equal,
        };
        let a_pos = txn
            .get_changeset(txn.changes(&channel), &a_int)
            .ok()
            .flatten();
        let b_pos = txn
            .get_changeset(txn.changes(&channel), &b_int)
            .ok()
            .flatten();
        a_pos.cmp(&b_pos)
    });
    let mut zombie_deps: Vec<Hash> = zombie_deps.into_iter().collect();
    zombie_deps.sort_by(|a, b| {
        let a_int = match txn.get_internal(&a.into()) {
            Ok(Some(id)) => id,
            _ => return std::cmp::Ordering::Equal,
        };
        let b_int = match txn.get_internal(&b.into()) {
            Ok(Some(id)) => id,
            _ => return std::cmp::Ordering::Equal,
        };
        let a_pos = txn
            .get_changeset(txn.changes(&channel), &a_int)
            .ok()
            .flatten();
        let b_pos = txn
            .get_changeset(txn.changes(&channel), &b_int)
            .ok()
            .flatten();
        a_pos.cmp(&b_pos)
    });
    Ok((deps, zombie_deps))
}

pub fn full_dependencies<T: ChannelTxnT + DepsTxnT<DepsError = <T as GraphTxnT>::GraphError>>(
    txn: &T,
    channel: &ChannelRef<T>,
) -> Result<(Vec<Hash>, Vec<Hash>), TxnErr<T::DepsError>> {
    let mut deps = BTreeSet::new();
    let channel = channel.read();
    for x in changeid_log(txn, &channel, L64(0))? {
        let (_, p) = x?;
        let h = txn.get_external(&p.a)?.unwrap();
        deps.insert(h.into());
    }
    let deps = minimize_deps(txn, &channel, &deps)?;
    Ok((deps, Vec::new()))
}

fn add_zombie_deps_from<T: GraphTxnT>(
    txn: &T,
    channel: &T::Graph,
    zombie_deps: &mut BTreeSet<Hash>,
    e_from: Position<Option<Hash>>,
) -> Result<(), MakeChangeError<T>> {
    let e_from = if let Some(p) = e_from.change {
        Position {
            change: *txn.get_internal(&p.into())?.unwrap(),
            pos: e_from.pos,
        }
    } else {
        return Ok(());
    };
    let from = if let Ok(from) = txn.find_block_end(channel, e_from) {
        from
    } else {
        return Err(MakeChangeError::InvalidChange);
    };
    for edge in iter_adj_all(txn, channel, *from)? {
        let edge = edge?;
        if let Some(ext) = txn.get_external(&edge.introduced_by())? {
            let ext: Hash = ext.into();
            if ext.is_none() {
            } else {
                zombie_deps.insert(ext);
            }
        }
        if let Some(ext) = txn.get_external(&edge.dest().change)? {
            let ext: Hash = ext.into();
            if ext.is_none() {
            } else {
                zombie_deps.insert(ext);
            }
        }
    }
    Ok(())
}

fn add_zombie_deps_to<T: GraphTxnT>(
    txn: &T,
    channel: &T::Graph,
    zombie_deps: &mut BTreeSet<Hash>,
    e_to: Vertex<Option<Hash>>,
) -> Result<(), MakeChangeError<T>> {
    let to_pos = if let Some(p) = e_to.change {
        Position {
            change: *txn.get_internal(&p.into())?.unwrap(),
            pos: e_to.start,
        }
    } else {
        return Ok(());
    };
    let mut to = if let Ok(to) = txn.find_block(channel, to_pos) {
        to
    } else {
        return Err(MakeChangeError::InvalidChange);
    };
    loop {
        for edge in iter_adj_all(txn, channel, *to)? {
            let edge = edge?;
            if let Some(ext) = txn.get_external(&edge.introduced_by())? {
                let ext: Hash = ext.into();
                if ext.is_none() {
                } else {
                    zombie_deps.insert(ext);
                }
            }
            if let Some(ext) = txn.get_external(&edge.dest().change)? {
                let ext: Hash = ext.into();
                if ext.is_none() {
                } else {
                    zombie_deps.insert(ext);
                }
            }
        }
        if to.end >= e_to.end {
            break;
        }
        to = txn.find_block(channel, to.end_pos()).unwrap();
    }
    Ok(())
}

/// Replace dependencies with consolidating tags when applicable.
/// Replaces dependencies with tags where applicable.
///
/// Checks if any dependencies are tag changes with tag metadata.
/// If so, replaces dependencies that are consolidated by those tags with the tag itself.
///
/// # Arguments
///
/// * `txn` - The transaction
/// * `channel` - The channel
/// * `deps` - The dependencies to potentially replace
///
/// # Returns
/// Dependencies with consolidated ones replaced by tag references
fn replace_deps_with_tags<
    T: ChannelTxnT + GraphTxnT + TagMetadataTxnT<TagError = T::GraphError>,
>(
    txn: &T,
    channel: &T::Channel,
    deps: Vec<Hash>,
) -> Result<(Vec<Hash>, Vec<Hash>), TxnErr<T::GraphError>> {
    debug!(
        "replace_deps_with_tags: checking {} dependencies",
        deps.len()
    );
    for dep in deps.iter() {
        debug!("  - dependency: {}", dep.to_base32());
    }

    // If no dependencies, nothing to replace
    if deps.is_empty() {
        debug!("replace_deps_with_tags: no dependencies to replace");
        return Ok((deps, Vec::new()));
    }

    // NEW APPROACH: Query the channel's tags table to find all tags
    // and check if any of them cover our dependencies
    let tags_table = txn.tags(channel);
    let mut tags_with_metadata: Vec<(u64, Hash, crate::pristine::Tag)> = Vec::new();

    debug!("replace_deps_with_tags: querying channel tags table");

    // Iterate through all tags in the channel
    if let Ok(mut iter) = txn.iter_tags(tags_table, 0) {
        debug!("replace_deps_with_tags: successfully opened tags iterator");
        loop {
            match txn.cursor_tags_next(&mut iter.cursor) {
                Ok(Some((timestamp, tag_bytes))) => {
                    debug!(
                        "replace_deps_with_tags: found tag entry at timestamp {}",
                        timestamp
                    );
                    // Deserialize the tag directly from TagBytes
                    let serialized_tag =
                        crate::pristine::SerializedTag::from_bytes_wrapper(tag_bytes);
                    match serialized_tag.to_tag() {
                        Ok(tag_data) => {
                            // Use the change_file_hash if available (this is what should be used as a dependency)
                            // Fall back to tag_hash if change_file_hash is not set
                            let dep_hash = tag_data.change_file_hash.unwrap_or(tag_data.tag_hash);
                            debug!(
                                "replace_deps_with_tags: found tag at timestamp {} with hash {} (change_file: {:?}) consolidating {} changes",
                                timestamp,
                                tag_data.tag_hash.to_base32(),
                                tag_data.change_file_hash.map(|h| h.to_base32()),
                                tag_data.consolidated_changes.len()
                            );
                            debug!("  Tag consolidates the following changes:");
                            for consolidated in tag_data.consolidated_changes.iter() {
                                debug!("    - {}", consolidated.to_base32());
                            }
                            tags_with_metadata.push(((*timestamp).into(), dep_hash, tag_data));
                        }
                        Err(e) => {
                            debug!("replace_deps_with_tags: failed to deserialize tag: {}", e);
                        }
                    }
                }
                Ok(None) => {
                    debug!("replace_deps_with_tags: no more tags in iterator");
                    break;
                }
                Err(e) => {
                    error!("replace_deps_with_tags: error iterating tags: {:?}", e);
                    break;
                }
            }
        }
    }

    // Sort tags by timestamp (newest first) to prefer the most recent tag
    tags_with_metadata.sort_by(|a, b| b.0.cmp(&a.0));

    debug!(
        "replace_deps_with_tags: found {} tags with metadata",
        tags_with_metadata.len()
    );

    // Check each tag to see if it covers any of our dependencies
    for (timestamp, tag_dep_hash, tag_data) in tags_with_metadata {
        let mut covered_deps = Vec::new();

        // Check which of our dependencies are covered by this tag
        debug!(
            "replace_deps_with_tags: checking if tag {} (with {} consolidated changes) covers any of our {} dependencies",
            tag_dep_hash.to_base32(),
            tag_data.consolidated_changes.len(),
            deps.len()
        );
        for dep in deps.iter() {
            let is_covered = tag_data.consolidated_changes.contains(dep);
            debug!(
                "  Checking dependency {}: covered = {}",
                dep.to_base32(),
                is_covered
            );
            if is_covered {
                debug!(
                    "replace_deps_with_tags: tag {} covers dependency {}",
                    tag_dep_hash.to_base32(),
                    dep.to_base32()
                );
                covered_deps.push(*dep);
            }
        }

        // If this tag covers any dependencies, use it
        if !covered_deps.is_empty() {
            debug!(
                "replace_deps_with_tags: SUCCESS - using tag {} (timestamp {}) which covers {} dependencies",
                tag_dep_hash.to_base32(),
                timestamp,
                covered_deps.len()
            );
            debug!("  Covered dependencies:");
            for dep in covered_deps.iter() {
                debug!("    - {}", dep.to_base32());
            }

            let mut new_deps = Vec::new();
            let mut consolidated_deps = Vec::new();

            // Add the tag's change_file_hash as a dependency (this is the change that was applied)
            debug!(
                "replace_deps_with_tags: adding tag change_file_hash {} as dependency",
                tag_dep_hash.to_base32()
            );
            new_deps.push(tag_dep_hash);

            // Keep dependencies NOT covered by this tag
            for dep in deps {
                if covered_deps.contains(&dep) {
                    consolidated_deps.push(dep);
                } else {
                    // Check if this dep is the tag itself (don't duplicate)
                    if dep != tag_dep_hash {
                        new_deps.push(dep);
                    }
                }
            }

            // Sort by time in channel
            new_deps.sort_by(|a, b| {
                let a_int = match txn.get_internal(&(*a).into()) {
                    Ok(Some(id)) => id,
                    _ => return std::cmp::Ordering::Equal,
                };
                let b_int = match txn.get_internal(&(*b).into()) {
                    Ok(Some(id)) => id,
                    _ => return std::cmp::Ordering::Equal,
                };
                let a_pos = txn
                    .get_changeset(txn.changes(channel), &a_int)
                    .ok()
                    .flatten();
                let b_pos = txn
                    .get_changeset(txn.changes(channel), &b_int)
                    .ok()
                    .flatten();
                a_pos.cmp(&b_pos)
            });

            debug!(
                "replace_deps_with_tags: returning {} dependencies (consolidated {} with tag)",
                new_deps.len(),
                covered_deps.len()
            );
            return Ok((new_deps, consolidated_deps));
        }
    }

    // No tags found or no coverage
    debug!(
        "replace_deps_with_tags: no tags cover any dependencies, returning original {} dependencies",
        deps.len()
    );
    Ok((deps, Vec::new()))
}

fn minimize_deps<T: ChannelTxnT + DepsTxnT<DepsError = <T as GraphTxnT>::GraphError>>(
    txn: &T,
    channel: &T::Channel,
    deps: &BTreeSet<Hash>,
) -> Result<Vec<Hash>, TxnErr<T::DepsError>> {
    let mut min_time = std::u64::MAX;
    let mut internal_deps = Vec::new();
    let mut internal_deps_ = HashSet::default();
    for h in deps.iter() {
        if h.is_none() {
            continue;
        }
        debug!("h = {:?}", h);
        let id = txn.get_internal(&h.into())?.unwrap();
        debug!("id = {:?}", id);
        let time = txn.get_changeset(txn.changes(&channel), id)?.unwrap();
        let time = u64::from_le(time.0);
        debug!("time = {:?}", time);
        min_time = min_time.min(time);
        internal_deps.push((id, true));
        internal_deps_.insert(id);
    }
    internal_deps.sort_by(|a, b| a.1.cmp(&b.1));
    let mut visited = HashSet::default();
    while let Some((id, is_root)) = internal_deps.pop() {
        if is_root {
            if !internal_deps_.contains(&id) {
                continue;
            }
        } else if internal_deps_.remove(&id) {
            debug!("removing dep {:?}", id);
        }
        if !visited.insert(id) {
            continue;
        }
        let mut cursor = txn.iter_dep(id)?;
        while let Some(x) = txn.cursor_dep_next(&mut cursor.cursor)? {
            let (id0, dep) = x;
            trace!("minimize loop = {:?} {:?}", id0, dep);
            if id0 < id {
                continue;
            } else if id0 > id {
                break;
            }
            let time = if let Some(time) = txn.get_changeset(txn.changes(&channel), dep)? {
                time
            } else {
                panic!(
                    "not found in channel {:?}: id = {:?} depends on {:?}",
                    txn.name(channel),
                    id,
                    dep
                );
            };
            let time = u64::from_le(time.0);
            trace!("time = {:?}", time);
            if time >= min_time {
                internal_deps.push((dep, false))
            }
        }
    }
    Ok(internal_deps_
        .into_iter()
        .map(|id| txn.get_external(id).unwrap().unwrap().into())
        .collect())
}

impl Change {
    pub fn knows(&self, hash: &Hash) -> bool {
        self.extra_known.contains(hash) || self.dependencies.contains(&hash)
    }

    pub fn has_edge(
        &self,
        hash: Hash,
        from: Position<Option<Hash>>,
        to: Position<Option<Hash>>,
        flags: crate::pristine::EdgeFlags,
    ) -> bool {
        debug!("has_edge: {:?} {:?} {:?} {:?}", hash, from, to, flags);
        for change_ in self.changes.iter() {
            for change_ in change_.iter() {
                match change_ {
                    Atom::NewVertex(n) => {
                        debug!("has_edge: {:?}", n);
                        if from.change == Some(hash) && from.pos >= n.start && from.pos <= n.end {
                            if to.change == Some(hash) {
                                // internal
                                return flags | EdgeFlags::FOLDER
                                    == EdgeFlags::BLOCK | EdgeFlags::FOLDER;
                            } else {
                                // down context
                                if n.down_context.iter().any(|d| *d == to) {
                                    return flags.is_empty();
                                } else {
                                    return false;
                                }
                            }
                        } else if to.change == Some(hash) && to.pos >= n.start && to.pos <= n.end {
                            // up context
                            if n.up_context.iter().any(|d| *d == from) {
                                return flags | EdgeFlags::FOLDER
                                    == EdgeFlags::BLOCK | EdgeFlags::FOLDER;
                            } else {
                                return false;
                            }
                        }
                    }
                    Atom::EdgeMap(e) => {
                        debug!("has_edge: {:?}", e);
                        if e.edges
                            .iter()
                            .any(|e| e.from == from && e.to.start_pos() == to && e.flag == flags)
                        {
                            return true;
                        }
                    }
                }
            }
        }
        debug!("not found");
        false
    }
}

impl<A> Atom<A> {
    pub fn as_newvertex(&self) -> &NewVertex<A> {
        if let Atom::NewVertex(n) = self {
            n
        } else {
            panic!("Not a NewVertex")
        }
    }
}

impl Atom<Option<Hash>> {
    pub fn inode(&self) -> Position<Option<Hash>> {
        match self {
            Atom::NewVertex(ref n) => n.inode,
            Atom::EdgeMap(ref n) => n.inode,
        }
    }

    pub fn inverse(&self, hash: &Hash) -> Self {
        match *self {
            Atom::NewVertex(NewVertex {
                ref up_context,
                flag,
                start,
                end,
                ref inode,
                ..
            }) => {
                let mut edges = Vec::new();
                for up in up_context {
                    edges.push(NewEdge {
                        previous: flag,
                        flag: flag | EdgeFlags::DELETED,
                        from: Position {
                            change: Some(if let Some(ref h) = up.change {
                                *h
                            } else {
                                *hash
                            }),
                            pos: up.pos,
                        },
                        to: Vertex {
                            change: Some(*hash),
                            start,
                            end,
                        },
                        introduced_by: Some(*hash),
                    })
                }
                Atom::EdgeMap(EdgeMap {
                    edges,
                    inode: Position {
                        change: Some(if let Some(p) = inode.change { p } else { *hash }),
                        pos: inode.pos,
                    },
                })
            }
            Atom::EdgeMap(EdgeMap {
                ref edges,
                ref inode,
            }) => Atom::EdgeMap(EdgeMap {
                inode: Position {
                    change: Some(if let Some(p) = inode.change { p } else { *hash }),
                    pos: inode.pos,
                },
                edges: edges
                    .iter()
                    .map(|e| {
                        let mut e = e.clone();
                        e.introduced_by = Some(*hash);
                        std::mem::swap(&mut e.flag, &mut e.previous);
                        e
                    })
                    .collect(),
            }),
        }
    }
}

impl EdgeMap<Option<Hash>> {
    fn concat(mut self, e: EdgeMap<Option<Hash>>) -> Self {
        assert_eq!(self.inode, e.inode);
        self.edges.extend(e.edges.into_iter());
        EdgeMap {
            inode: self.inode,
            edges: self.edges,
        }
    }
}

impl<L: Clone> Hunk<Option<Hash>, L> {
    pub fn inverse(&self, hash: &Hash) -> Self {
        match self {
            Hunk::AddRoot { name, inode } => Hunk::DelRoot {
                name: name.inverse(hash),
                inode: inode.inverse(hash),
            },
            Hunk::DelRoot { name, inode } => Hunk::AddRoot {
                name: name.inverse(hash),
                inode: inode.inverse(hash),
            },
            Hunk::FileMove { del, add, path } => Hunk::FileMove {
                del: add.inverse(hash),
                add: del.inverse(hash),
                path: path.clone(),
            },
            Hunk::FileDel {
                del,
                contents,
                path,
                encoding,
            } => Hunk::FileUndel {
                undel: del.inverse(hash),
                contents: contents.as_ref().map(|c| c.inverse(hash)),
                path: path.clone(),
                encoding: encoding.clone(),
            },
            Hunk::FileUndel {
                undel,
                contents,
                path,
                encoding,
            } => Hunk::FileDel {
                del: undel.inverse(hash),
                contents: contents.as_ref().map(|c| c.inverse(hash)),
                path: path.clone(),
                encoding: encoding.clone(),
            },
            Hunk::FileAdd {
                add_name,
                add_inode,
                contents,
                path,
                encoding,
            } => {
                let del = match (add_name.inverse(hash), add_inode.inverse(hash)) {
                    (Atom::EdgeMap(e0), Atom::EdgeMap(e1)) => Atom::EdgeMap(e0.concat(e1)),
                    _ => unreachable!(),
                };
                Hunk::FileDel {
                    del,
                    contents: contents.as_ref().map(|c| c.inverse(hash)),
                    path: path.clone(),
                    encoding: encoding.clone(),
                }
            }
            Hunk::SolveNameConflict { name, path } => Hunk::UnsolveNameConflict {
                name: name.inverse(hash),
                path: path.clone(),
            },
            Hunk::UnsolveNameConflict { name, path } => Hunk::SolveNameConflict {
                name: name.inverse(hash),
                path: path.clone(),
            },
            Hunk::Edit {
                change,
                local,
                encoding,
            } => Hunk::Edit {
                change: change.inverse(hash),
                local: local.clone(),
                encoding: encoding.clone(),
            },
            Hunk::Replacement {
                change,
                replacement,
                local,
                encoding,
            } => Hunk::Replacement {
                change: replacement.inverse(hash),
                replacement: change.inverse(hash),
                local: local.clone(),
                encoding: encoding.clone(),
            },
            Hunk::SolveOrderConflict { change, local } => Hunk::UnsolveOrderConflict {
                change: change.inverse(hash),
                local: local.clone(),
            },
            Hunk::UnsolveOrderConflict { change, local } => Hunk::SolveOrderConflict {
                change: change.inverse(hash),
                local: local.clone(),
            },
            Hunk::ResurrectZombies {
                change,
                local,
                encoding,
            } => Hunk::Edit {
                change: change.inverse(hash),
                local: local.clone(),
                encoding: encoding.clone(),
            },
        }
    }
}

impl Change {
    pub fn inverse(&self, hash: &Hash, header: ChangeHeader, metadata: Vec<u8>) -> Self {
        let dependencies = vec![*hash];
        let contents_hash = Hasher::default().finish();
        Change {
            offsets: Offsets::default(),
            hashed: Hashed {
                version: VERSION,
                header,
                dependencies,
                extra_known: self.extra_known.clone(),
                metadata,
                changes: self.changes.iter().map(|r| r.inverse(hash)).collect(),
                contents_hash,
                tag: None,
            },
            contents: Vec::new(),
            unhashed: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalByte {
    pub path: String,
    pub line: usize,
    pub inode: Inode,
    pub byte: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Local {
    pub path: String,
    pub line: usize,
}

pub type Hunk<Hash, Local> = BaseHunk<Atom<Hash>, Local>;

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub enum BaseHunk<Atom, Local> {
    FileMove {
        del: Atom,
        add: Atom,
        path: String,
    },
    FileDel {
        del: Atom,
        contents: Option<Atom>,
        path: String,
        encoding: Option<Encoding>,
    },
    FileUndel {
        undel: Atom,
        contents: Option<Atom>,
        path: String,
        encoding: Option<Encoding>,
    },
    FileAdd {
        add_name: Atom,
        add_inode: Atom,
        contents: Option<Atom>,
        path: String,
        encoding: Option<Encoding>,
    },
    SolveNameConflict {
        name: Atom,
        path: String,
    },
    UnsolveNameConflict {
        name: Atom,
        path: String,
    },
    Edit {
        change: Atom,
        local: Local,
        encoding: Option<Encoding>,
    },
    Replacement {
        change: Atom,
        replacement: Atom,
        local: Local,
        encoding: Option<Encoding>,
    },
    SolveOrderConflict {
        change: Atom,
        local: Local,
    },
    UnsolveOrderConflict {
        change: Atom,
        local: Local,
    },
    ResurrectZombies {
        change: Atom,
        local: Local,
        encoding: Option<Encoding>,
    },
    AddRoot {
        name: Atom,
        inode: Atom,
    },
    DelRoot {
        name: Atom,
        inode: Atom,
    },
}

#[doc(hidden)]
pub struct HunkIter<R, C> {
    rec: Option<R>,
    extra: Option<C>,
    extra2: Option<C>,
}

impl<Context, Local> IntoIterator for Hunk<Context, Local> {
    type IntoIter = HunkIter<Hunk<Context, Local>, Atom<Context>>;
    type Item = Atom<Context>;
    fn into_iter(self) -> Self::IntoIter {
        HunkIter {
            rec: Some(self),
            extra: None,
            extra2: None,
        }
    }
}

impl<Context, Local> Hunk<Context, Local> {
    pub fn iter(&self) -> HunkIter<&Hunk<Context, Local>, &Atom<Context>> {
        HunkIter {
            rec: Some(self),
            extra: None,
            extra2: None,
        }
    }
    pub fn rev_iter(&self) -> RevHunkIter<&Hunk<Context, Local>, &Atom<Context>> {
        RevHunkIter {
            rec: Some(self),
            extra: None,
            extra2: None,
        }
    }
}

impl<Context, Local> Iterator for HunkIter<Hunk<Context, Local>, Atom<Context>> {
    type Item = Atom<Context>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(extra) = self.extra.take() {
            Some(extra)
        } else if let Some(extra) = self.extra2.take() {
            Some(extra)
        } else if let Some(rec) = self.rec.take() {
            match rec {
                Hunk::FileMove { del, add, .. } => {
                    self.extra = Some(add);
                    Some(del)
                }
                Hunk::FileDel { del, contents, .. } => {
                    self.extra = contents;
                    Some(del)
                }
                Hunk::FileUndel {
                    undel, contents, ..
                } => {
                    self.extra = contents;
                    Some(undel)
                }
                Hunk::FileAdd {
                    add_name,
                    add_inode,
                    contents,
                    ..
                } => {
                    self.extra = Some(add_inode);
                    self.extra2 = contents;
                    Some(add_name)
                }
                Hunk::SolveNameConflict { name, .. } => Some(name),
                Hunk::UnsolveNameConflict { name, .. } => Some(name),
                Hunk::Edit { change, .. } => Some(change),
                Hunk::Replacement {
                    change,
                    replacement,
                    ..
                } => {
                    self.extra = Some(replacement);
                    Some(change)
                }
                Hunk::SolveOrderConflict { change, .. } => Some(change),
                Hunk::UnsolveOrderConflict { change, .. } => Some(change),
                Hunk::ResurrectZombies { change, .. } => Some(change),
                Hunk::AddRoot { inode, name } | Hunk::DelRoot { inode, name } => {
                    self.extra = Some(inode);
                    Some(name)
                }
            }
        } else {
            None
        }
    }
}

impl<'a, Context, Local> Iterator for HunkIter<&'a Hunk<Context, Local>, &'a Atom<Context>> {
    type Item = &'a Atom<Context>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(extra) = self.extra.take() {
            Some(extra)
        } else if let Some(extra) = self.extra2.take() {
            Some(extra)
        } else if let Some(rec) = self.rec.take() {
            match *rec {
                Hunk::FileMove {
                    ref del, ref add, ..
                } => {
                    self.extra = Some(add);
                    Some(del)
                }
                Hunk::FileDel {
                    ref del,
                    ref contents,
                    ..
                } => {
                    self.extra = contents.as_ref();
                    Some(del)
                }
                Hunk::FileUndel {
                    ref undel,
                    ref contents,
                    ..
                } => {
                    self.extra = contents.as_ref();
                    Some(undel)
                }
                Hunk::FileAdd {
                    ref add_name,
                    ref add_inode,
                    ref contents,
                    ..
                } => {
                    self.extra = Some(add_inode);
                    self.extra2 = contents.as_ref();
                    Some(&add_name)
                }
                Hunk::SolveNameConflict { ref name, .. } => Some(&name),
                Hunk::UnsolveNameConflict { ref name, .. } => Some(&name),
                Hunk::Edit { change: ref c, .. } => Some(c),
                Hunk::Replacement {
                    replacement: ref r,
                    change: ref c,
                    ..
                } => {
                    self.extra = Some(r);
                    Some(c)
                }
                Hunk::SolveOrderConflict { ref change, .. } => Some(change),
                Hunk::UnsolveOrderConflict { ref change, .. } => Some(change),
                Hunk::ResurrectZombies { ref change, .. } => Some(change),
                Hunk::AddRoot {
                    ref inode,
                    ref name,
                }
                | Hunk::DelRoot {
                    ref inode,
                    ref name,
                } => {
                    self.extra = Some(inode);
                    Some(name)
                }
            }
        } else {
            None
        }
    }
}

pub struct RevHunkIter<R, C> {
    rec: Option<R>,
    extra: Option<C>,
    extra2: Option<C>,
}

impl<'a, Context, Local> Iterator for RevHunkIter<&'a Hunk<Context, Local>, &'a Atom<Context>> {
    type Item = &'a Atom<Context>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(extra) = self.extra.take() {
            Some(extra)
        } else if let Some(extra) = self.extra2.take() {
            Some(extra)
        } else if let Some(rec) = self.rec.take() {
            match *rec {
                Hunk::FileMove {
                    ref del, ref add, ..
                } => {
                    self.extra = Some(del);
                    Some(add)
                }
                Hunk::FileDel {
                    ref del,
                    ref contents,
                    ..
                } => {
                    if let Some(ref c) = contents {
                        self.extra = Some(del);
                        Some(c)
                    } else {
                        Some(del)
                    }
                }
                Hunk::FileUndel {
                    ref undel,
                    ref contents,
                    ..
                } => {
                    if let Some(ref c) = contents {
                        self.extra = Some(undel);
                        Some(c)
                    } else {
                        Some(undel)
                    }
                }
                Hunk::FileAdd {
                    ref add_name,
                    ref add_inode,
                    ref contents,
                    ..
                } => {
                    if let Some(ref c) = contents {
                        self.extra = Some(add_inode);
                        self.extra2 = Some(add_name);
                        Some(c)
                    } else {
                        self.extra = Some(add_name);
                        Some(add_inode)
                    }
                }
                Hunk::SolveNameConflict { ref name, .. } => Some(&name),
                Hunk::UnsolveNameConflict { ref name, .. } => Some(&name),
                Hunk::Edit { change: ref c, .. } => Some(c),
                Hunk::Replacement {
                    replacement: ref r,
                    change: ref c,
                    ..
                } => {
                    self.extra = Some(c);
                    Some(r)
                }
                Hunk::SolveOrderConflict { ref change, .. } => Some(change),
                Hunk::UnsolveOrderConflict { ref change, .. } => Some(change),
                Hunk::ResurrectZombies { ref change, .. } => Some(change),
                Hunk::AddRoot {
                    ref name,
                    ref inode,
                }
                | Hunk::DelRoot {
                    ref name,
                    ref inode,
                } => {
                    self.extra = Some(inode);
                    Some(name)
                }
            }
        } else {
            None
        }
    }
}

impl Atom<Option<NodeId>> {
    fn globalize<T: GraphTxnT>(&self, txn: &T) -> Result<Atom<Option<Hash>>, T::GraphError> {
        match self {
            Atom::NewVertex(NewVertex {
                up_context,
                down_context,
                start,
                end,
                flag,
                inode,
            }) => Ok(Atom::NewVertex(NewVertex {
                up_context: up_context
                    .iter()
                    .map(|&up| Position {
                        change: up
                            .change
                            .as_ref()
                            .and_then(|a| txn.get_external(a).unwrap().map(Into::into)),
                        pos: up.pos,
                    })
                    .collect(),
                down_context: down_context
                    .iter()
                    .map(|&down| Position {
                        change: down
                            .change
                            .as_ref()
                            .and_then(|a| txn.get_external(a).unwrap().map(Into::into)),
                        pos: down.pos,
                    })
                    .collect(),
                start: *start,
                end: *end,
                flag: *flag,
                inode: Position {
                    change: inode
                        .change
                        .as_ref()
                        .and_then(|a| txn.get_external(a).unwrap().map(Into::into)),
                    pos: inode.pos,
                },
            })),
            Atom::EdgeMap(EdgeMap { edges, inode }) => Ok(Atom::EdgeMap(EdgeMap {
                edges: edges
                    .iter()
                    .map(|edge| NewEdge {
                        previous: edge.previous,
                        flag: edge.flag,
                        from: Position {
                            change: edge
                                .from
                                .change
                                .as_ref()
                                .and_then(|a| txn.get_external(a).unwrap().map(Into::into)),
                            pos: edge.from.pos,
                        },
                        to: Vertex {
                            change: edge
                                .to
                                .change
                                .as_ref()
                                .and_then(|a| txn.get_external(a).unwrap().map(Into::into)),
                            start: edge.to.start,
                            end: edge.to.end,
                        },
                        introduced_by: edge.introduced_by.as_ref().map(|a| {
                            if let Some(a) = txn.get_external(a).unwrap() {
                                a.into()
                            } else {
                                panic!("introduced by {:?}", a);
                            }
                        }),
                    })
                    .collect(),
                inode: Position {
                    change: inode
                        .change
                        .as_ref()
                        .and_then(|a| txn.get_external(a).unwrap().map(Into::into)),
                    pos: inode.pos,
                },
            })),
        }
    }
}

impl<H> Hunk<H, Local> {
    pub fn local(&self) -> Option<&Local> {
        match self {
            Hunk::Edit { ref local, .. }
            | Hunk::Replacement { ref local, .. }
            | Hunk::SolveOrderConflict { ref local, .. }
            | Hunk::UnsolveOrderConflict { ref local, .. }
            | Hunk::ResurrectZombies { ref local, .. } => Some(local),
            _ => None,
        }
    }

    pub fn path(&self) -> &str {
        match self {
            Hunk::FileMove { ref path, .. }
            | Hunk::FileDel { ref path, .. }
            | Hunk::FileUndel { ref path, .. }
            | Hunk::SolveNameConflict { ref path, .. }
            | Hunk::UnsolveNameConflict { ref path, .. }
            | Hunk::FileAdd { ref path, .. } => path,
            Hunk::Edit { ref local, .. }
            | Hunk::Replacement { ref local, .. }
            | Hunk::SolveOrderConflict { ref local, .. }
            | Hunk::UnsolveOrderConflict { ref local, .. }
            | Hunk::ResurrectZombies { ref local, .. } => &local.path,
            Hunk::AddRoot { .. } | Hunk::DelRoot { .. } => "/",
        }
    }

    pub fn line(&self) -> Option<usize> {
        self.local().map(|x| x.line)
    }
}

impl<A, Local> BaseHunk<A, Local> {
    pub fn atom_map<B, E, Loc, F: FnMut(A) -> Result<B, E>, L: FnMut(Local) -> Loc>(
        self,
        mut f: F,
        mut l: L,
    ) -> Result<BaseHunk<B, Loc>, E> {
        Ok(match self {
            BaseHunk::FileMove { del, add, path } => BaseHunk::FileMove {
                del: f(del)?,
                add: f(add)?,
                path,
            },
            BaseHunk::FileDel {
                del,
                contents,
                path,
                encoding,
            } => BaseHunk::FileDel {
                del: f(del)?,
                contents: if let Some(c) = contents {
                    Some(f(c)?)
                } else {
                    None
                },
                path,
                encoding,
            },
            BaseHunk::FileUndel {
                undel,
                contents,
                path,
                encoding,
            } => BaseHunk::FileUndel {
                undel: f(undel)?,
                contents: if let Some(c) = contents {
                    Some(f(c)?)
                } else {
                    None
                },
                path,
                encoding,
            },
            BaseHunk::SolveNameConflict { name, path } => BaseHunk::SolveNameConflict {
                name: f(name)?,
                path,
            },
            BaseHunk::UnsolveNameConflict { name, path } => BaseHunk::UnsolveNameConflict {
                name: f(name)?,
                path,
            },
            BaseHunk::FileAdd {
                add_inode,
                add_name,
                contents,
                path,
                encoding,
            } => BaseHunk::FileAdd {
                add_name: f(add_name)?,
                add_inode: f(add_inode)?,
                contents: if let Some(c) = contents {
                    Some(f(c)?)
                } else {
                    None
                },
                path,
                encoding,
            },
            BaseHunk::Edit {
                change,
                local,
                encoding,
            } => BaseHunk::Edit {
                change: f(change)?,
                local: l(local),
                encoding,
            },
            BaseHunk::Replacement {
                change,
                replacement,
                local,
                encoding,
            } => BaseHunk::Replacement {
                change: f(change)?,
                replacement: f(replacement)?,
                local: l(local),
                encoding,
            },
            BaseHunk::SolveOrderConflict { change, local } => BaseHunk::SolveOrderConflict {
                change: f(change)?,
                local: l(local),
            },
            BaseHunk::UnsolveOrderConflict { change, local } => BaseHunk::UnsolveOrderConflict {
                change: f(change)?,
                local: l(local),
            },
            BaseHunk::ResurrectZombies {
                change,
                local,
                encoding,
            } => BaseHunk::ResurrectZombies {
                change: f(change)?,
                local: l(local),
                encoding,
            },
            BaseHunk::AddRoot { name, inode } => BaseHunk::AddRoot {
                name: f(name)?,
                inode: f(inode)?,
            },
            BaseHunk::DelRoot { name, inode } => BaseHunk::DelRoot {
                name: f(name)?,
                inode: f(inode)?,
            },
        })
    }
}

impl Hunk<Option<NodeId>, LocalByte> {
    pub fn globalize<T: GraphTxnT>(
        self,
        txn: &T,
    ) -> Result<Hunk<Option<Hash>, Local>, T::GraphError> {
        self.atom_map(
            |x| x.globalize(txn),
            |l| Local {
                path: l.path,
                line: l.line,
            },
        )
    }
}

/// A table of contents of a change, indicating where each section is,
/// to allow seeking inside a change file.
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct Offsets {
    pub version: u64,
    pub hashed_len: u64, // length of the hashed contents
    pub unhashed_off: u64,
    pub unhashed_len: u64, // length of the unhashed contents
    pub contents_off: u64,
    pub contents_len: u64,
    pub total: u64,
}

#[derive(Error)]
pub enum MakeChangeError<T: GraphTxnT> {
    #[error(transparent)]
    Graph(#[from] TxnErr<<T as GraphTxnT>::GraphError>),
    #[error("Invalid change")]
    InvalidChange,
}

impl<T: GraphTxnT> std::fmt::Debug for MakeChangeError<T> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            MakeChangeError::Graph(e) => std::fmt::Debug::fmt(e, fmt),
            MakeChangeError::InvalidChange => std::fmt::Debug::fmt("InvalidChange", fmt),
        }
    }
}

impl LocalChange<Hunk<Option<Hash>, Local>, Author> {
    pub const OFFSETS_SIZE: u64 = 56;

    pub fn make_change<
        T: ChannelTxnT
            + DepsTxnT<DepsError = <T as GraphTxnT>::GraphError>
            + TagMetadataTxnT<TagError = <T as GraphTxnT>::GraphError>,
    >(
        txn: &T,
        channel: &ChannelRef<T>,
        changes: Vec<Hunk<Option<Hash>, Local>>,
        contents: Vec<u8>,
        header: ChangeHeader,
        metadata: Vec<u8>,
    ) -> Result<Self, MakeChangeError<T>> {
        let (dependencies, extra_known) = dependencies(txn, &channel.read(), changes.iter(), true)?;
        trace!("make_change, contents = {:?}", contents);
        let contents_hash = {
            let mut hasher = Hasher::default();
            hasher.update(&contents);
            hasher.finish()
        };
        debug!("make_change, contents_hash = {:?}", contents_hash);
        Ok(LocalChange {
            offsets: Offsets::default(),
            hashed: Hashed {
                version: VERSION,
                header,
                changes,
                contents_hash,
                metadata,
                dependencies,
                extra_known,
                tag: None,
            },
            contents,
            unhashed: None,
        })
    }

    pub fn new() -> Self {
        LocalChange {
            offsets: Offsets::default(),
            hashed: Hashed {
                version: VERSION,
                header: ChangeHeader::default(),
                changes: Vec::new(),
                contents_hash: Hasher::default().finish(),
                metadata: Vec::new(),
                dependencies: Vec::new(),
                extra_known: Vec::new(),
                tag: None,
            },
            unhashed: None,
            contents: Vec::new(),
        }
    }
    pub fn write_all_deps<F: FnMut(Hash) -> Result<(), ChangeError>>(
        &self,
        f: F,
    ) -> Result<(), ChangeError> {
        self.hashed.write_all_deps(f)
    }
}

impl Hashed<Hunk<Option<Hash>, Local>, Author> {
    pub fn write_all_deps<F: FnMut(Hash) -> Result<(), ChangeError>>(
        &self,
        mut f: F,
    ) -> Result<(), ChangeError> {
        for c in self.changes.iter() {
            for c in c.iter() {
                match *c {
                    Atom::NewVertex(ref n) => {
                        for change in n
                            .up_context
                            .iter()
                            .chain(n.down_context.iter())
                            .map(|c| c.change)
                            .chain(std::iter::once(n.inode.change))
                        {
                            if let Some(change) = change {
                                if change.is_none() {
                                    continue;
                                }
                                f(change)?
                            }
                        }
                    }
                    Atom::EdgeMap(ref e) => {
                        for edge in e.edges.iter() {
                            for change in &[
                                edge.from.change,
                                edge.to.change,
                                edge.introduced_by,
                                e.inode.change,
                            ] {
                                if let Some(change) = *change {
                                    if change.is_none() {
                                        continue;
                                    }
                                    f(change)?
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(feature = "zstd")]
const LEVEL: usize = 3;
#[cfg(feature = "zstd")]
const FRAME_SIZE: usize = 4096;
#[cfg(feature = "zstd")]
fn compress(input: &[u8], w: &mut Vec<u8>) -> Result<(), ChangeError> {
    info!(
        "compressing with ZStd {}",
        zstd_seekable::version().to_str().unwrap()
    );
    let mut level = LEVEL;
    if let Ok(l) = std::env::var("ZSTD_LEVEL") {
        if let Ok(l) = l.parse() {
            level = l
        }
    }
    let mut cstream = zstd_seekable::SeekableCStream::new(level, FRAME_SIZE).unwrap();
    let mut output = [0; 4096];
    let mut input_pos = 0;
    while input_pos < input.len() {
        let (out_pos, inp_pos) = cstream.compress(&mut output, &input[input_pos..])?;
        w.write_all(&output[..out_pos])?;
        input_pos += inp_pos;
    }
    while let Ok(n) = cstream.end_stream(&mut output) {
        if n == 0 {
            break;
        }
        w.write_all(&output[..n])?;
    }
    Ok(())
}

impl Change {
    pub fn size_no_contents<R: std::io::Read + std::io::Seek>(
        r: &mut R,
    ) -> Result<u64, ChangeError> {
        let pos = r.seek(std::io::SeekFrom::Current(0))?;
        let mut off = [0u8; Self::OFFSETS_SIZE as usize];
        r.read_exact(&mut off)?;
        let off: Offsets = bincode::deserialize(&off)?;
        if off.version != VERSION && off.version != VERSION_NOENC {
            return Err(ChangeError::VersionMismatch { got: off.version });
        }
        r.seek(std::io::SeekFrom::Start(pos))?;
        Ok(off.contents_off)
    }

    /// Serialise the change as a file named "<hash>.change" in
    /// directory `dir`, where "<hash>" is the actual hash of the
    /// change.
    #[cfg(feature = "zstd")]
    pub fn serialize<
        W: Write,
        E: From<ChangeError>,
        F: FnOnce(&mut Self, &Hash) -> Result<(), E>,
    >(
        &mut self,
        mut w: W,
        f: F,
    ) -> Result<Hash, E> {
        // Hashed part.
        let mut hashed = Vec::new();
        bincode::serialize_into(&mut hashed, &self.hashed).map_err(From::from)?;
        trace!("hashed = {:?}", hashed);
        let mut hasher = Hasher::default();
        hasher.update(&hashed);
        let hash = hasher.finish();
        debug!("{:?}", hash);

        f(self, &hash)?;

        // Unhashed part.
        let unhashed = if let Some(ref un) = self.unhashed {
            let s = serde_json::to_string(un).unwrap();
            s.into()
        } else {
            Vec::new()
        };

        // Compress the change.
        let mut hashed_comp = Vec::new();
        let now = std::time::Instant::now();
        compress(&hashed, &mut hashed_comp)?;
        debug!("compressed hashed in {:?}", now.elapsed());
        let now = std::time::Instant::now();
        let unhashed_off = Self::OFFSETS_SIZE + hashed_comp.len() as u64;
        let mut unhashed_comp = Vec::new();
        compress(&unhashed, &mut unhashed_comp)?;
        debug!("compressed unhashed in {:?}", now.elapsed());
        let contents_off = unhashed_off + unhashed_comp.len() as u64;
        let mut contents_comp = Vec::new();
        let now = std::time::Instant::now();
        compress(&self.contents, &mut contents_comp)?;
        debug!(
            "compressed {:?} bytes of contents in {:?}",
            self.contents.len(),
            now.elapsed()
        );

        let offsets = Offsets {
            version: VERSION,
            hashed_len: hashed.len() as u64,
            unhashed_off,
            unhashed_len: unhashed.len() as u64,
            contents_off,
            contents_len: self.contents.len() as u64,
            total: contents_off + contents_comp.len() as u64,
        };

        bincode::serialize_into(&mut w, &offsets).map_err(From::from)?;
        w.write_all(&hashed_comp).map_err(From::from)?;
        w.write_all(&unhashed_comp).map_err(From::from)?;
        w.write_all(&contents_comp).map_err(From::from)?;
        debug!("change serialized");

        Ok(hash)
    }

    /// Deserialise a change from the file given as input `file`.
    #[cfg(feature = "zstd")]
    pub fn check_from_buffer(buf: &[u8], hash: &Hash) -> Result<(), ChangeError> {
        let offsets: Offsets = bincode::deserialize_from(&buf[..Self::OFFSETS_SIZE as usize])?;
        if offsets.version != VERSION && offsets.version != VERSION_NOENC {
            return Err(ChangeError::VersionMismatch {
                got: offsets.version,
            });
        }

        debug!("check_from_buffer, offsets = {:?}", offsets);
        let mut s = zstd_seekable::Seekable::init_buf(
            &buf[Self::OFFSETS_SIZE as usize..offsets.unhashed_off as usize],
        )?;
        let mut buf_ = Vec::new();
        buf_.resize(offsets.hashed_len as usize, 0);
        s.decompress(&mut buf_[..], 0)?;
        trace!("check_from_buffer, buf_ = {:?}", buf_);
        let mut hasher = Hasher::default();
        hasher.update(&buf_);
        let computed_hash = hasher.finish();
        debug!("{:?} {:?}", computed_hash, hash);
        if &computed_hash != hash {
            return Err((ChangeError::ChangeHashMismatch {
                claimed: *hash,
                computed: computed_hash,
            })
            .into());
        }

        let hashed: Hashed<Hunk<Option<Hash>, Local>, Author> = if offsets.version == VERSION {
            bincode::deserialize(&buf_)?
        } else {
            let h: Hashed<noenc::Hunk<Option<Hash>, Local>, noenc::Author> =
                bincode::deserialize(&buf_)?;
            h.into()
        };
        buf_.clear();
        buf_.resize(offsets.contents_len as usize, 0);
        let mut s = zstd_seekable::Seekable::init_buf(&buf[offsets.contents_off as usize..])?;
        buf_.resize(offsets.contents_len as usize, 0);
        s.decompress(&mut buf_[..], 0)?;
        let mut hasher = Hasher::default();
        trace!("contents = {:?}", buf_);
        hasher.update(&buf_);
        let computed_hash = hasher.finish();
        debug!(
            "contents hash: {:?}, computed: {:?}",
            hashed.contents_hash, computed_hash
        );
        if computed_hash != hashed.contents_hash {
            return Err(ChangeError::ContentsHashMismatch {
                claimed: hashed.contents_hash,
                computed: computed_hash,
            });
        }
        Ok(())
    }

    /// Deserialise a change from the file given as input `file`.
    #[cfg(feature = "zstd")]
    pub fn deserialize(file: &str, hash: Option<&Hash>) -> Result<Self, ChangeError> {
        use std::io::Read;
        let mut r = std::fs::File::open(file).map_err(|err| {
            if let Some(h) = hash {
                ChangeError::IoHash { err, hash: *h }
            } else {
                ChangeError::Io(err)
            }
        })?;
        let mut buf = vec![0u8; Self::OFFSETS_SIZE as usize];
        r.read_exact(&mut buf)?;
        let offsets: Offsets = bincode::deserialize(&buf)?;
        if offsets.version == VERSION_NOENC {
            return Self::deserialize_noenc(offsets, r, hash);
        } else if offsets.version != VERSION {
            return Err(ChangeError::VersionMismatch {
                got: offsets.version,
            });
        }
        debug!("offsets = {:?}", offsets);
        buf.clear();
        buf.resize((offsets.unhashed_off - Self::OFFSETS_SIZE) as usize, 0);
        r.read_exact(&mut buf)?;

        let hashed: Hashed<Hunk<Option<Hash>, Local>, Author> = {
            let mut s = zstd_seekable::Seekable::init_buf(&buf[..])?;
            let mut out = vec![0u8; offsets.hashed_len as usize];
            s.decompress(&mut out[..], 0)?;
            let mut hasher = Hasher::default();
            hasher.update(&out);
            let computed_hash = hasher.finish();
            if let Some(hash) = hash {
                if &computed_hash != hash {
                    return Err(ChangeError::ChangeHashMismatch {
                        claimed: *hash,
                        computed: computed_hash,
                    });
                }
            }
            bincode::deserialize_from(&out[..])?
        };
        buf.clear();
        buf.resize((offsets.contents_off - offsets.unhashed_off) as usize, 0);
        let unhashed = if buf.is_empty() {
            None
        } else {
            r.read_exact(&mut buf)?;
            let mut s = zstd_seekable::Seekable::init_buf(&buf[..])?;
            let mut out = vec![0u8; offsets.unhashed_len as usize];
            s.decompress(&mut out[..], 0)?;
            debug!("parsing unhashed: {:?}", std::str::from_utf8(&out));
            serde_json::from_slice(&out).ok()
        };
        debug!("unhashed = {:?}", unhashed);

        buf.clear();
        buf.resize((offsets.total - offsets.contents_off) as usize, 0);
        let contents = if r.read_exact(&mut buf).is_ok() {
            let mut s = zstd_seekable::Seekable::init_buf(&buf[..])?;
            let mut contents = vec![0u8; offsets.contents_len as usize];
            s.decompress(&mut contents[..], 0)?;
            contents
        } else {
            Vec::new()
        };
        debug!("contents = {:?}", contents);

        Ok(LocalChange {
            offsets,
            hashed,
            unhashed,
            contents,
        })
    }

    /// Compute the hash of this change. If the `zstd` feature is
    /// enabled, it is probably more efficient to serialise the change
    /// (using the `serialize` method) at the same time, which also
    /// returns the hash.
    pub fn hash(&self) -> Result<Hash, bincode::Error> {
        let input = bincode::serialize(&self.hashed)?;
        let mut hasher = Hasher::default();
        hasher.update(&input);
        Ok(hasher.finish())
    }
}
