//! Apply a change.
use crate::change::{Atom, Change, EdgeMap, NewVertex};
use crate::changestore::ChangeStore;
use crate::missing_context::*;
use crate::pristine::*;
use crate::record::InodeUpdate;
use crate::{HashMap, HashSet};
use std::collections::BTreeSet;
use thiserror::Error;
pub(crate) mod edge;
pub(crate) use edge::*;
mod vertex;
pub(crate) use vertex::*;

pub enum ApplyError<ChangestoreError: std::error::Error, T: GraphTxnT + TreeTxnT> {
    Changestore(ChangestoreError),
    LocalChange(LocalApplyError<T>),
    MakeChange(crate::change::MakeChangeError<T>),
}

impl<C: std::error::Error, T: GraphTxnT + TreeTxnT> std::fmt::Debug for ApplyError<C, T> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ApplyError::Changestore(e) => std::fmt::Debug::fmt(e, fmt),
            ApplyError::LocalChange(e) => std::fmt::Debug::fmt(e, fmt),
            ApplyError::MakeChange(e) => std::fmt::Debug::fmt(e, fmt),
        }
    }
}

impl<C: std::error::Error, T: GraphTxnT + TreeTxnT> std::fmt::Display for ApplyError<C, T> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ApplyError::Changestore(e) => std::fmt::Display::fmt(e, fmt),
            ApplyError::LocalChange(e) => std::fmt::Display::fmt(e, fmt),
            ApplyError::MakeChange(e) => std::fmt::Display::fmt(e, fmt),
        }
    }
}

impl<C: std::error::Error, T: GraphTxnT + TreeTxnT> std::error::Error for ApplyError<C, T> {}

#[derive(Error)]
pub enum LocalApplyError<T: GraphTxnT + TreeTxnT> {
    DependencyMissing {
        hash: crate::pristine::Hash,
    },
    ChangeAlreadyOnChannel {
        hash: crate::pristine::Hash,
    },
    TagAlreadyOnChannel {
        hash: crate::pristine::Hash,
    },
    TagStateMismatch {
        tag_hash: crate::pristine::Hash,
        expected_state: Merkle,
        actual_state: Merkle,
    },
    TagNotRegistered {
        hash: crate::pristine::Hash,
    },
    Txn(#[from] TxnErr<T::GraphError>),
    Tree(#[from] TreeErr<T::TreeError>),
    Block {
        block: Position<NodeId>,
    },
    InvalidChange,
    Corruption,
    MakeChange(#[from] crate::change::MakeChangeError<T>),
}

impl<T: GraphTxnT + TreeTxnT> std::fmt::Debug for LocalApplyError<T> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LocalApplyError::DependencyMissing { hash } => {
                write!(fmt, "Dependency missing: {:?}", hash)
            }
            LocalApplyError::ChangeAlreadyOnChannel { hash } => {
                write!(fmt, "Change already on channel: {:?}", hash)
            }
            LocalApplyError::TagAlreadyOnChannel { hash } => {
                write!(fmt, "Tag already on channel: {:?}", hash)
            }
            LocalApplyError::TagStateMismatch {
                tag_hash,
                expected_state,
                actual_state,
            } => {
                write!(
                    fmt,
                    "Tag {} state mismatch: expected {:?}, got {:?}",
                    tag_hash.to_base32(),
                    expected_state.to_base32(),
                    actual_state.to_base32()
                )
            }
            LocalApplyError::TagNotRegistered { hash } => {
                write!(fmt, "Tag not registered: {:?}", hash)
            }
            LocalApplyError::Txn(e) => std::fmt::Debug::fmt(e, fmt),
            LocalApplyError::Tree(e) => std::fmt::Debug::fmt(e, fmt),
            LocalApplyError::Block { block } => write!(fmt, "Block error: {:?}", block),
            LocalApplyError::InvalidChange => write!(fmt, "Invalid change"),
            LocalApplyError::Corruption => write!(fmt, "Corruption"),
            LocalApplyError::MakeChange(e) => std::fmt::Debug::fmt(e, fmt),
        }
    }
}

impl<T: GraphTxnT + TreeTxnT> std::fmt::Display for LocalApplyError<T> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LocalApplyError::DependencyMissing { hash } => {
                write!(fmt, "Dependency missing: {:?}", hash)
            }
            LocalApplyError::ChangeAlreadyOnChannel { hash } => {
                write!(fmt, "Change already on channel: {:?}", hash)
            }
            LocalApplyError::TagAlreadyOnChannel { hash } => {
                write!(fmt, "Tag already on channel: {:?}", hash)
            }
            LocalApplyError::TagStateMismatch {
                tag_hash,
                expected_state,
                actual_state,
            } => {
                write!(
                    fmt,
                    "Tag {} state mismatch: expected {}, got {}",
                    tag_hash.to_base32(),
                    expected_state.to_base32(),
                    actual_state.to_base32()
                )
            }
            LocalApplyError::TagNotRegistered { hash } => {
                write!(fmt, "Tag not registered: {:?}", hash)
            }
            LocalApplyError::Txn(e) => std::fmt::Display::fmt(e, fmt),
            LocalApplyError::Tree(e) => std::fmt::Display::fmt(e, fmt),
            LocalApplyError::Block { block } => write!(fmt, "Block error: {:?}", block),
            LocalApplyError::InvalidChange => write!(fmt, "Invalid change"),
            LocalApplyError::Corruption => write!(fmt, "Corruption"),
            LocalApplyError::MakeChange(e) => std::fmt::Display::fmt(e, fmt),
        }
    }
}

impl<C: std::error::Error, T: GraphTxnT + TreeTxnT> From<crate::pristine::TxnErr<T::GraphError>>
    for ApplyError<C, T>
{
    fn from(err: crate::pristine::TxnErr<T::GraphError>) -> Self {
        ApplyError::LocalChange(LocalApplyError::Txn(err))
    }
}

impl<C: std::error::Error, T: GraphTxnT + TreeTxnT> From<crate::change::MakeChangeError<T>>
    for ApplyError<C, T>
{
    fn from(err: crate::change::MakeChangeError<T>) -> Self {
        ApplyError::MakeChange(err)
    }
}

impl<C: std::error::Error, T: GraphTxnT + TreeTxnT> From<crate::pristine::TreeErr<T::TreeError>>
    for ApplyError<C, T>
{
    fn from(err: crate::pristine::TreeErr<T::TreeError>) -> Self {
        ApplyError::LocalChange(LocalApplyError::Tree(err))
    }
}

impl<T: GraphTxnT + TreeTxnT> LocalApplyError<T> {
    fn from_missing(err: MissingError<T::GraphError>) -> Self {
        match err {
            MissingError::Txn(e) => LocalApplyError::Txn(TxnErr(e)),
            MissingError::Block(e) => e.into(),
            MissingError::Inconsistent(_) => LocalApplyError::InvalidChange,
        }
    }
}

impl<T: GraphTxnT + TreeTxnT> From<crate::pristine::InconsistentChange<T::GraphError>>
    for LocalApplyError<T>
{
    fn from(err: crate::pristine::InconsistentChange<T::GraphError>) -> Self {
        match err {
            InconsistentChange::Txn(e) => LocalApplyError::Txn(TxnErr(e)),
            _ => LocalApplyError::InvalidChange,
        }
    }
}

impl<T: GraphTxnT + TreeTxnT> From<crate::pristine::BlockError<T::GraphError>>
    for LocalApplyError<T>
{
    fn from(err: crate::pristine::BlockError<T::GraphError>) -> Self {
        match err {
            BlockError::Txn(e) => LocalApplyError::Txn(TxnErr(e)),
            BlockError::Block { block } => LocalApplyError::Block { block },
        }
    }
}

impl<C: std::error::Error, T: GraphTxnT + TreeTxnT> From<crate::pristine::BlockError<T::GraphError>>
    for ApplyError<C, T>
{
    fn from(err: crate::pristine::BlockError<T::GraphError>) -> Self {
        ApplyError::LocalChange(LocalApplyError::from(err))
    }
}

/// Get a change from the changestore, falling back to tag lookup if not found.
///
/// This function implements tag-aware dependency resolution:
/// 1. First tries to load the change as a regular .change file
/// 2. If not found, checks if the hash refers to a tag
/// 3. If it's a tag, creates a virtual change from the tag metadata
fn get_change_or_tag<
    T: GraphTxnT + TreeTxnT + crate::pristine::TagMetadataTxnT<TagError = T::GraphError>,
    P: ChangeStore,
>(
    changes: &P,
    txn: &T,
    hash: &Hash,
) -> Result<Change, ApplyError<P::Error, T>> {
    // Try to load as a regular change file
    match changes.get_change(hash) {
        Ok(change) => {
            debug!(
                "get_change_or_tag: loaded change {} from changestore",
                hash.to_base32()
            );
            Ok(change)
        }
        Err(changestore_err) => {
            debug!(
                "get_change_or_tag: change {} not found in changestore, checking tags table",
                hash.to_base32()
            );

            // Check if this hash refers to a tag
            match txn.get_tag(hash) {
                Ok(Some(serialized_tag)) => {
                    debug!(
                        "get_change_or_tag: found tag metadata for {}",
                        hash.to_base32()
                    );

                    // Convert tag to a virtual change
                    match serialized_tag.to_tag() {
                        Ok(tag) => {
                            debug!("get_change_or_tag: creating virtual change from tag (consolidates {} changes)",
                                   tag.consolidated_changes.len());

                            // Create a virtual change that represents the tag
                            // Tags don't modify files, so empty hunks
                            // Dependencies are the consolidated changes
                            let virtual_change = Change {
                                offsets: crate::change::Offsets::default(),
                                hashed: crate::change::Hashed {
                                    version: crate::change::VERSION,
                                    header: crate::change::ChangeHeader {
                                        message: format!(
                                            "Tag (consolidates {} changes)",
                                            tag.consolidated_changes.len()
                                        ),
                                        authors: vec![],
                                        description: None,
                                        timestamp: chrono::DateTime::from_timestamp(
                                            tag.consolidation_timestamp as i64,
                                            0,
                                        )
                                        .unwrap_or_else(chrono::Utc::now),
                                    },
                                    changes: vec![], // Tags don't modify files
                                    contents_hash: {
                                        let mut hasher = crate::pristine::Hasher::default();
                                        hasher.update(&[]);
                                        hasher.finish()
                                    },
                                    metadata: vec![],
                                    dependencies: tag.consolidated_changes.clone(),
                                    extra_known: vec![], // All dependencies are explicit
                                    tag: None,           // Virtual change doesn't have tag metadata
                                },
                                unhashed: None,
                                contents: vec![],
                            };

                            debug!(
                                "get_change_or_tag: virtual change has {} dependencies",
                                virtual_change.hashed.dependencies.len()
                            );
                            Ok(virtual_change)
                        }
                        Err(e) => {
                            error!("get_change_or_tag: failed to deserialize tag: {}", e);
                            Err(ApplyError::Changestore(changestore_err))
                        }
                    }
                }
                Ok(None) => {
                    debug!(
                        "get_change_or_tag: {} is neither a change nor a tag",
                        hash.to_base32()
                    );
                    Err(ApplyError::Changestore(changestore_err))
                }
                Err(txn_err) => {
                    error!(
                        "get_change_or_tag: error querying tags table: {:?}",
                        txn_err
                    );
                    Err(ApplyError::Changestore(changestore_err))
                }
            }
        }
    }
}

/// Apply a node (change or tag) to a channel.
///
/// This is the unified function that handles both changes and tags uniformly.
/// Tags are registered in the graph but don't modify the working copy.
/// Changes are registered and applied to the channel.
pub fn apply_node_ws<
    T: MutTxnT + crate::pristine::TagMetadataMutTxnT<TagError = T::GraphError>,
    P: ChangeStore,
>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    node_type: crate::pristine::NodeType,
    workspace: &mut Workspace,
) -> Result<(u64, Merkle), ApplyError<P::Error, T>> {
    debug!("apply_node {:?} (type: {:?})", hash.to_base32(), node_type);

    match node_type {
        crate::pristine::NodeType::Change => {
            apply_change_ws_impl(changes, txn, channel, hash, workspace)
        }
        crate::pristine::NodeType::Tag => {
            debug!("Applying tag to channel");

            // 1. Verify tag is registered in the graph
            let shash: SerializedHash = hash.into();
            let internal = if let Some(&i) = txn.get_internal(&shash)? {
                i
            } else {
                return Err(ApplyError::LocalChange(LocalApplyError::TagNotRegistered {
                    hash: *hash,
                }));
            };

            // 2. Check if tag is already on this channel
            if let Some(_) = txn.get_changeset(txn.changes(channel), &internal)? {
                debug!("Tag {} already on channel, skipping", hash.to_base32());
                return Err(ApplyError::LocalChange(
                    LocalApplyError::TagAlreadyOnChannel { hash: *hash },
                ));
            }

            // 3. Get current channel state
            let current_state = crate::pristine::current_state(txn, channel)?;
            debug!(
                "Current channel state: {}, applying tag for hash: {}",
                current_state.to_base32(),
                hash.to_base32()
            );

            // 4. Tags mark the current state - they don't change it
            // The tag's state should match the current channel state
            // (This is a validation - tags are created at specific states)

            // 5. Get the apply counter (position in channel log)
            let position = txn.apply_counter(channel);
            debug!("Tag position in channel: {}", position);

            // 6. Add tag to channel's tags table
            // This associates the position with the current state
            if let Some(state_position) =
                txn.channel_has_state(txn.states(channel), &current_state.into())?
            {
                let tags = txn.tags_mut(channel);
                txn.put_tags(tags, state_position.into(), &current_state)?;
                debug!(
                    "Tag {} added to channel at position {} for state {}",
                    hash.to_base32(),
                    state_position,
                    current_state.to_base32()
                );
            }

            // 7. Increment apply counter for next operation
            txn.touch_channel(channel, None);

            // 8. Return position and state (state unchanged by tag)
            Ok((position, current_state))
        }
    }
}

/// Apply a change to a channel. This function does not update the
/// inodes/tree tables, i.e. the correspondence between the pristine
/// and the working copy. Therefore, this function must be used only
/// on remote changes, or locally with the
/// [`libatomic::working_copy::filesystem::FileSystem`].
pub fn apply_change_ws<
    T: MutTxnT + crate::pristine::TagMetadataMutTxnT<TagError = T::GraphError>,
    P: ChangeStore,
>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    workspace: &mut Workspace,
) -> Result<(u64, Merkle), ApplyError<P::Error, T>> {
    apply_change_ws_impl(changes, txn, channel, hash, workspace)
}

fn apply_change_ws_impl<
    T: MutTxnT + crate::pristine::TagMetadataMutTxnT<TagError = T::GraphError>,
    P: ChangeStore,
>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    workspace: &mut Workspace,
) -> Result<(u64, Merkle), ApplyError<P::Error, T>> {
    debug!("apply_change {:?}", hash.to_base32());
    workspace.clear();
    let change = changes.get_change(hash).map_err(ApplyError::Changestore)?;

    let shash: SerializedHash = hash.into();
    let internal = if let Some(&p) = txn.get_internal(&shash)? {
        p
    } else {
        let internal: NodeId = make_changeid(txn, &hash)?;
        register_change(txn, &internal, hash, &change)?;
        internal
    };
    debug!("internal = {:?}", internal);
    let result = apply_change_to_channel(
        txn,
        channel,
        &mut |h| changes.knows(h, hash).unwrap(),
        internal,
        hash,
        &change,
        workspace,
    )
    .map_err(ApplyError::LocalChange)?;

    Ok(result)
}

pub fn apply_change_rec_ws<
    T: TxnT + MutTxnT + crate::pristine::TagMetadataMutTxnT<TagError = T::GraphError>,
    P: ChangeStore,
>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    workspace: &mut Workspace,
    deps_only: bool,
) -> Result<(), ApplyError<P::Error, T>> {
    debug!("apply_change {:?}", hash.to_base32());
    workspace.clear();
    let mut dep_stack = vec![(*hash, true, !deps_only)];
    let mut visited = HashSet::default();
    while let Some((hash, first, actually_apply)) = dep_stack.pop() {
        let change = get_change_or_tag(changes, txn, &hash)?;
        let shash: SerializedHash = (&hash).into();

        if first {
            if !visited.insert(hash) {
                continue;
            }
            if let Some(change_id) = txn.get_internal(&shash)? {
                if txn
                    .get_changeset(txn.changes(&channel), change_id)?
                    .is_some()
                {
                    continue;
                }
            }

            dep_stack.push((hash, false, actually_apply));
            for &hash in change.dependencies.iter() {
                if hash.is_none() {
                    continue;
                }
                dep_stack.push((hash, true, true))
            }
        } else if actually_apply {
            let applied = if let Some(int) = txn.get_internal(&shash)? {
                txn.get_changeset(txn.changes(&channel), int)?.is_some()
            } else {
                false
            };
            if !applied {
                let internal = if let Some(&p) = txn.get_internal(&shash)? {
                    p
                } else {
                    let internal: NodeId = make_changeid(txn, &hash)?;
                    register_change(txn, &internal, &hash, &change)?;
                    internal
                };
                debug!("internal = {:?}", internal);
                workspace.clear();
                apply_change_to_channel(
                    txn,
                    channel,
                    &mut |h| changes.knows(h, &hash).unwrap(),
                    internal,
                    &hash,
                    &change,
                    workspace,
                )
                .map_err(ApplyError::LocalChange)?;
            }
        }
    }

    Ok(())
}

/// Apply a node (change or tag) to a channel, allocating its own workspace.
/// Apply a node recursively with its dependencies, using provided workspace.
///
/// This is the unified recursive application function that works for both changes and tags.
/// It automatically resolves and applies all dependencies before applying the node itself.
///
/// # Arguments
/// * `changes` - Change store for loading change/tag data
/// * `txn` - Transaction for database operations
/// * `channel` - Channel to apply the node to
/// * `hash` - Hash of the node to apply
/// * `node_type` - Type of node (Change or Tag)
/// * `workspace` - Workspace for apply operations
/// * `deps_only` - If true, only apply dependencies, not the node itself
pub fn apply_node_rec_ws<
    T: TxnT + MutTxnT + crate::pristine::TagMetadataMutTxnT<TagError = T::GraphError>,
    P: ChangeStore,
>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    node_type: crate::pristine::NodeType,
    workspace: &mut Workspace,
    deps_only: bool,
) -> Result<(), ApplyError<P::Error, T>> {
    debug!(
        "apply_node_rec: {:?} (type: {:?}, deps_only: {})",
        hash.to_base32(),
        node_type,
        deps_only
    );
    workspace.clear();

    // Stack of (hash, node_type, first_visit, actually_apply)
    let mut dep_stack = vec![(*hash, node_type, true, !deps_only)];
    let mut visited = HashSet::default();

    while let Some((hash, node_type, first, actually_apply)) = dep_stack.pop() {
        let shash: SerializedHash = (&hash).into();

        if first {
            // First visit to this node - check if already applied
            if !visited.insert(hash) {
                continue; // Already visited in this traversal
            }

            // Check if node is already on channel
            if let Some(change_id) = txn.get_internal(&shash)? {
                if txn
                    .get_changeset(txn.changes(&channel), change_id)?
                    .is_some()
                {
                    debug!("Node {} already on channel, skipping", hash.to_base32());
                    continue;
                }
            }

            // Get the node's data (works for both changes and tags)
            let node_data = get_change_or_tag(changes, txn, &hash)?;

            // Push this node back for second visit (actual application)
            dep_stack.push((hash, node_type, false, actually_apply));

            // Push all dependencies onto stack (will be processed first)
            for &dep_hash in node_data.dependencies.iter() {
                if dep_hash.is_none() {
                    continue;
                }

                // Determine dependency type
                let dep_type = if let Some(&dep_internal) = txn.get_internal(&(&dep_hash).into())? {
                    // Node is registered, get its type
                    if let Some(dt) = txn.get_node_type(&dep_internal)? {
                        dt
                    } else {
                        // Default to Change if type not found
                        debug!(
                            "Dependency {} has no node type, assuming Change",
                            dep_hash.to_base32()
                        );
                        crate::pristine::NodeType::Change
                    }
                } else {
                    // Not registered yet, assume Change
                    debug!(
                        "Dependency {} not registered, assuming Change",
                        dep_hash.to_base32()
                    );
                    crate::pristine::NodeType::Change
                };

                debug!(
                    "Adding dependency {} (type: {:?}) to stack",
                    dep_hash.to_base32(),
                    dep_type
                );
                dep_stack.push((dep_hash, dep_type, true, true));
            }
        } else if actually_apply {
            // Second visit - apply the node if not already applied
            let applied = if let Some(int) = txn.get_internal(&shash)? {
                txn.get_changeset(txn.changes(&channel), int)?.is_some()
            } else {
                false
            };

            if !applied {
                debug!("Applying node {} (type: {:?})", hash.to_base32(), node_type);
                workspace.clear();
                apply_node_ws(changes, txn, channel, &hash, node_type, workspace)?;
            } else {
                debug!(
                    "Node {} already applied during this operation",
                    hash.to_base32()
                );
            }
        }
    }

    Ok(())
}

/// Same as [apply_change_ws], but allocates its own workspace.
pub fn apply_node<
    T: MutTxnT + crate::pristine::TagMetadataMutTxnT<TagError = T::GraphError>,
    P: ChangeStore,
>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    node_type: crate::pristine::NodeType,
) -> Result<(u64, Merkle), ApplyError<P::Error, T>> {
    apply_node_ws(
        changes,
        txn,
        channel,
        hash,
        node_type,
        &mut Workspace::new(),
    )
}

/// Apply a node recursively with its dependencies, allocating its own workspace.
pub fn apply_node_rec<
    T: TxnT + MutTxnT + crate::pristine::TagMetadataMutTxnT<TagError = T::GraphError>,
    P: ChangeStore,
>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    node_type: crate::pristine::NodeType,
) -> Result<(), ApplyError<P::Error, T>> {
    apply_node_rec_ws(
        changes,
        txn,
        channel,
        hash,
        node_type,
        &mut Workspace::new(),
        false,
    )
}

/// Same as [apply_change_ws], but allocates its own workspace.
pub fn apply_change<
    T: MutTxnT + crate::pristine::TagMetadataMutTxnT<TagError = T::GraphError>,
    P: ChangeStore,
>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
) -> Result<(u64, Merkle), ApplyError<P::Error, T>> {
    apply_change_ws(changes, txn, channel, hash, &mut Workspace::new())
}

/// Same as [apply_change], but with a wrapped `txn` and `channel`.
pub fn apply_change_arc<
    T: MutTxnT + crate::pristine::TagMetadataMutTxnT<TagError = T::GraphError>,
    P: ChangeStore,
>(
    changes: &P,
    txn: &ArcTxn<T>,
    channel: &ChannelRef<T>,
    hash: &Hash,
) -> Result<(u64, Merkle), ApplyError<P::Error, T>> {
    apply_change_ws(
        changes,
        &mut *txn.write(),
        &mut *channel.write(),
        hash,
        &mut Workspace::new(),
    )
}

/// Same as [apply_change_ws], but allocates its own workspace.
pub fn apply_change_rec<
    T: TxnT + MutTxnT + crate::pristine::TagMetadataMutTxnT<TagError = T::GraphError>,
    P: ChangeStore,
>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    deps_only: bool,
) -> Result<(), ApplyError<P::Error, T>> {
    apply_change_rec_ws(
        changes,
        txn,
        channel,
        hash,
        &mut Workspace::new(),
        deps_only,
    )
}

fn apply_change_to_channel<T: ChannelMutTxnT + TreeTxnT, F: FnMut(&Hash) -> bool>(
    txn: &mut T,
    channel: &mut T::Channel,
    changes: &mut F,
    change_id: NodeId,
    hash: &Hash,
    change: &Change,
    ws: &mut Workspace,
) -> Result<(u64, Merkle), LocalApplyError<T>> {
    ws.assert_empty();
    let n = txn.apply_counter(channel);
    debug!("apply_change_to_channel {:?} {:?}", change_id, hash);
    let merkle =
        if let Some(m) = txn.put_changes(channel, change_id, txn.apply_counter(channel), hash)? {
            m
        } else {
            return Err(LocalApplyError::ChangeAlreadyOnChannel { hash: *hash });
        };
    debug!("apply change to channel");
    let now = std::time::Instant::now();
    for (n, change_) in change.changes.iter().enumerate() {
        debug!("Applying {} {:?} (1)", n, change_);
        for change_ in change_.iter() {
            match *change_ {
                Atom::NewVertex(ref n) => put_newvertex(
                    txn,
                    T::graph_mut(channel),
                    changes,
                    change,
                    ws,
                    change_id,
                    n,
                )?,
                Atom::EdgeMap(ref n) => {
                    for edge in n.edges.iter() {
                        if !edge.flag.contains(EdgeFlags::DELETED) {
                            put_newedge(
                                txn,
                                T::graph_mut(channel),
                                ws,
                                change_id,
                                n.inode,
                                edge,
                                |_, _| true,
                                |h| change.knows(h),
                            )?;
                        }
                    }
                }
            }
        }
    }
    for change_ in change.changes.iter() {
        debug!("Applying {:?} (2)", change_);
        for change_ in change_.iter() {
            if let Atom::EdgeMap(ref n) = *change_ {
                for edge in n.edges.iter() {
                    if edge.flag.contains(EdgeFlags::DELETED) {
                        put_newedge(
                            txn,
                            T::graph_mut(channel),
                            ws,
                            change_id,
                            n.inode,
                            edge,
                            |_, _| true,
                            |h| change.knows(h),
                        )?;
                    }
                }
            }
        }
    }
    crate::TIMERS.lock().unwrap().apply += now.elapsed();

    let mut inodes = clean_obsolete_pseudo_edges(txn, T::graph_mut(channel), ws, change_id)?;
    collect_missing_contexts(txn, txn.graph(channel), ws, &change, change_id, &mut inodes)?;
    for i in inodes {
        repair_zombies(txn, T::graph_mut(channel), i)?;
    }

    detect_folder_conflict_resolutions(
        txn,
        T::graph_mut(channel),
        &mut ws.missing_context,
        change_id,
        change,
    )
    .map_err(LocalApplyError::from_missing)?;

    repair_cyclic_paths(txn, T::graph_mut(channel), ws)?;
    info!("done applying change");
    Ok((n, merkle))
}

/// Apply a change created locally: serialize it, compute its hash, and
/// apply it. This function also registers changes in the filesystem
/// introduced by the change (file additions, deletions and moves), to
/// synchronise the pristine and the working copy after the
/// application.
pub fn apply_local_change_ws<
    T: ChannelMutTxnT
        + DepsMutTxnT<DepsError = <T as GraphTxnT>::GraphError>
        + TreeMutTxnT
        + crate::pristine::TagMetadataTxnT<TagError = T::GraphError>,
>(
    txn: &mut T,
    channel: &ChannelRef<T>,
    change: &Change,
    hash: &Hash,
    inode_updates: &HashMap<usize, InodeUpdate>,
    workspace: &mut Workspace,
) -> Result<(u64, Merkle), LocalApplyError<T>> {
    let mut channel = channel.write();
    let internal: NodeId = make_changeid(txn, hash)?;
    debug!("make_changeid {:?} {:?}", hash, internal);

    // Tag-aware dependency validation
    for dep_hash in change.dependencies.iter() {
        if dep_hash.is_none() {
            continue;
        }

        // Check if dependency is already in the channel
        if let Some(int) = txn.get_internal(&dep_hash.into())? {
            if txn.get_changeset(txn.changes(&channel), int)?.is_some() {
                continue;
            }
        }

        // Check if dependency is a tag
        // Tags are valid dependencies even if not yet applied as changes
        if let Ok(Some(_)) = txn.get_tag(dep_hash) {
            debug!(
                "apply_local_change_ws: dependency {} is a tag (valid)",
                dep_hash.to_base32()
            );
            continue;
        }

        return Err((LocalApplyError::DependencyMissing { hash: *dep_hash }).into());
    }

    register_change(txn, &internal, hash, &change)?;
    let n = apply_change_to_channel(
        txn,
        &mut channel,
        &mut |_| true,
        internal,
        &hash,
        &change,
        workspace,
    )?;
    for (_, update) in inode_updates.iter() {
        info!("updating {:?}", update);
        update_inode(txn, &channel, internal, update)?;
    }
    Ok(n)
}

/// Same as [apply_local_change_ws], but allocates its own workspace.
pub fn apply_local_change<
    T: ChannelMutTxnT
        + DepsMutTxnT<DepsError = <T as GraphTxnT>::GraphError>
        + TreeMutTxnT
        + crate::pristine::TagMetadataTxnT<TagError = T::GraphError>,
>(
    txn: &mut T,
    channel: &ChannelRef<T>,
    change: &Change,
    hash: &Hash,
    inode_updates: &HashMap<usize, InodeUpdate>,
) -> Result<(u64, Merkle), LocalApplyError<T>> {
    apply_local_change_ws(
        txn,
        channel,
        change,
        hash,
        inode_updates,
        &mut Workspace::new(),
    )
}

fn update_inode<T: ChannelTxnT + TreeMutTxnT>(
    txn: &mut T,
    channel: &T::Channel,
    internal: NodeId,
    update: &InodeUpdate,
) -> Result<(), LocalApplyError<T>> {
    debug!("update_inode {:?}", update);
    match *update {
        InodeUpdate::Add { inode, pos, .. } => {
            let vertex = Position {
                change: internal,
                pos,
            };
            if txn
                .get_graph(txn.graph(channel), &vertex.inode_vertex(), None)?
                .is_some()
            {
                debug!("Adding inodes: {:?} {:?}", inode, vertex);
                put_inodes_with_rev(txn, &inode, &vertex)?;
            } else {
                debug!("Not adding inodes: {:?} {:?}", inode, vertex);
            }
        }
        InodeUpdate::Deleted { inode } => {
            if let Some(parent) = txn.get_revtree(&inode, None)?.map(|x| x.to_owned()) {
                del_tree_with_rev(txn, &parent, &inode)?;
            }
            // Delete the directory, if it's there.
            txn.del_tree(&OwnedPathId::inode(inode), Some(&inode))?;
            if let Some(&vertex) = txn.get_inodes(&inode, None)? {
                del_inodes_with_rev(txn, &inode, &vertex)?;
            }
        }
    }
    Ok(())
}

#[derive(Default)]
pub struct Workspace {
    parents: HashSet<Vertex<NodeId>>,
    children: HashSet<Vertex<NodeId>>,
    pseudo: Vec<(Vertex<NodeId>, SerializedEdge, Position<Option<Hash>>)>,
    deleted_by: HashSet<NodeId>,
    up_context: Vec<Vertex<NodeId>>,
    down_context: Vec<Vertex<NodeId>>,
    pub(crate) missing_context: crate::missing_context::Workspace,
    rooted: HashMap<Vertex<NodeId>, bool>,
    adjbuf: Vec<SerializedEdge>,
    alive_folder: HashMap<Vertex<NodeId>, bool>,
    folder_stack: Vec<(Vertex<NodeId>, bool)>,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }
    fn clear(&mut self) {
        self.children.clear();
        self.parents.clear();
        self.pseudo.clear();
        self.deleted_by.clear();
        self.up_context.clear();
        self.down_context.clear();
        self.missing_context.clear();
        self.rooted.clear();
        self.adjbuf.clear();
        self.alive_folder.clear();
        self.folder_stack.clear();
    }
    fn assert_empty(&self) {
        assert!(self.children.is_empty());
        assert!(self.parents.is_empty());
        assert!(self.pseudo.is_empty());
        assert!(self.deleted_by.is_empty());
        assert!(self.up_context.is_empty());
        assert!(self.down_context.is_empty());
        self.missing_context.assert_empty();
        assert!(self.rooted.is_empty());
        assert!(self.adjbuf.is_empty());
        assert!(self.alive_folder.is_empty());
        assert!(self.folder_stack.is_empty());
    }
}

#[derive(Debug)]
struct StackElt {
    vertex: Vertex<NodeId>,
    last_alive: Vertex<NodeId>,
    is_on_path: bool,
}

impl StackElt {
    fn is_alive(&self) -> bool {
        self.vertex == self.last_alive
    }
}

pub(crate) fn repair_zombies<T: GraphMutTxnT + TreeTxnT>(
    txn: &mut T,
    channel: &mut T::Graph,
    root: Position<NodeId>,
) -> Result<(), LocalApplyError<T>> {
    let mut stack = vec![StackElt {
        vertex: root.inode_vertex(),
        last_alive: root.inode_vertex(),
        is_on_path: false,
    }];

    let mut visited = BTreeSet::new();
    let mut descendants = BTreeSet::new();

    while let Some(elt) = stack.pop() {
        debug!("elt {:?}", elt);
        if elt.is_on_path {
            continue;
        }

        // Has this vertex been visited already?
        debug!("visited {:?}", visited);
        if !visited.insert(elt.vertex) {
            debug!("already visited!");
            debug!("descendants {:#?}", descendants);
            for (_, r) in descendants.range((elt.vertex, Vertex::ROOT)..=(elt.vertex, Vertex::MAX))
            {
                put_graph_with_rev(
                    txn,
                    channel,
                    EdgeFlags::PSEUDO,
                    elt.last_alive,
                    *r,
                    NodeId::ROOT,
                )?;
            }

            continue;
        }

        // Else, visit its children.
        stack.push(StackElt {
            is_on_path: true,
            ..elt
        });

        let len = stack.len();
        // If this is the first visit, find the children, in flag
        // order (alive first), since we don't want to reconnect
        // vertices multiple times.
        for e in iter_adjacent(
            txn,
            channel,
            elt.vertex,
            EdgeFlags::empty(),
            EdgeFlags::all(),
        )? {
            let e = e?;

            if e.flag().contains(EdgeFlags::PARENT) {
                if e.flag() & (EdgeFlags::BLOCK | EdgeFlags::DELETED) == EdgeFlags::BLOCK {
                    // This vertex is alive!
                    stack[len - 1].last_alive = elt.vertex;
                }
                continue;
            } else if e.flag().contains(EdgeFlags::FOLDER) {
                // If we are here, at least one child of `root` is
                // FOLDER, hence all are.
                return Ok(());
            }

            let child = txn.find_block(channel, e.dest())?;
            stack.push(StackElt {
                vertex: *child,
                last_alive: elt.last_alive,
                is_on_path: false,
            });
        }

        if len >= 2 && stack[len - 1].is_alive() {
            // The visited vertex is alive. Change the last_alive of its children
            for x in &mut stack[len..] {
                x.last_alive = elt.vertex
            }

            for v in (&stack[..len - 1]).iter().rev() {
                if v.is_on_path {
                    // If the last vertex on the path to `current` is not
                    // alive, a reconnect is needed.
                    if v.is_alive() {
                        // We need to reconnect, and we can do it now
                        // since we won't have a chance to visit that
                        // edge (because non-PARENT edge we are
                        // inserting now starts from a vertex that is
                        // on the path, which means we've already
                        // pushed all its children onto the stack.).
                        put_graph_with_rev(
                            txn,
                            channel,
                            EdgeFlags::PSEUDO,
                            elt.last_alive,
                            stack[len - 1].vertex,
                            NodeId::ROOT,
                        )?;
                        break;
                    } else {
                        // Remember that those dead vertices have
                        // `stack[len-1].vertex` as a descendant.
                        descendants.insert((v.vertex, stack[len - 1].vertex));
                    }
                }
            }
        }

        // If no children, pop.
        if stack.len() == len {
            stack.pop();
        }
    }

    Ok(())
}

pub fn clean_obsolete_pseudo_edges<T: GraphMutTxnT + TreeTxnT>(
    txn: &mut T,
    channel: &mut T::Graph,
    ws: &mut Workspace,
    change_id: NodeId,
) -> Result<HashSet<Position<NodeId>>, LocalApplyError<T>> {
    info!(
        "clean_obsolete_pseudo_edges, ws.pseudo.len() = {}",
        ws.pseudo.len()
    );
    let mut alive_folder = std::mem::replace(&mut ws.alive_folder, HashMap::new());
    let mut folder_stack = std::mem::replace(&mut ws.folder_stack, Vec::new());

    let mut inodes = HashSet::new();

    for (next_vertex, p, inode) in ws.pseudo.drain(..) {
        debug!(
            "clean_obsolete_pseudo_edges {:?} {:?} {:?}",
            next_vertex, p, inode
        );

        {
            let still_here: Vec<_> = iter_adjacent(
                txn,
                channel,
                next_vertex,
                EdgeFlags::empty(),
                EdgeFlags::all(),
            )?
            .collect();
            debug!(
                "pseudo edge still here ? {:?} {:?}",
                next_vertex.change.0 .0, still_here
            )
        }

        let (a, b) = if p.flag().is_parent() {
            if let Ok(&dest) = txn.find_block_end(channel, p.dest()) {
                (dest, next_vertex)
            } else {
                continue;
            }
        } else if let Ok(&dest) = txn.find_block(channel, p.dest()) {
            (next_vertex, dest)
        } else {
            continue;
        };
        let a_is_alive = is_alive(txn, channel, &a)?;
        let b_is_alive = is_alive(txn, channel, &b)?;
        if a_is_alive && b_is_alive {
            continue;
        }

        // If we're deleting a FOLDER edge, repair_context_deleted
        // will not repair its potential descendants. Hence, we must
        // also count as "alive" a FOLDER node with alive descendants.
        if p.flag().is_folder() {
            if folder_has_alive_descendants(txn, channel, &mut alive_folder, &mut folder_stack, b)?
            {
                continue;
            }
        }

        if a.is_empty() && b_is_alive {
            // In this case, `a` can be an inode, in which case we
            // can't simply delete the edge, since b would become
            // unreachable.
            //
            // We test this here:
            let mut is_inode = false;
            for e in iter_adjacent(
                txn,
                channel,
                a,
                EdgeFlags::FOLDER | EdgeFlags::PARENT,
                EdgeFlags::all(),
            )? {
                let e = e?;
                if e.flag().contains(EdgeFlags::FOLDER | EdgeFlags::PARENT) {
                    is_inode = true;
                    break;
                }
            }
            if is_inode {
                continue;
            }
        }

        debug!(
            "deleting {:?} {:?} {:?} {:?} {:?} {:?}",
            a,
            b,
            p.introduced_by(),
            p.flag(),
            a_is_alive,
            b_is_alive,
        );
        del_graph_with_rev(
            txn,
            channel,
            p.flag() - EdgeFlags::PARENT,
            a,
            b,
            p.introduced_by(),
        )?;

        if a_is_alive || (b_is_alive && !p.flag().is_folder()) {
            // A context repair is needed.
            inodes.insert(internal_pos(txn, &inode, change_id)?);
        }
    }

    ws.alive_folder = alive_folder;
    ws.folder_stack = folder_stack;
    Ok(inodes)
}

fn folder_has_alive_descendants<T: GraphMutTxnT + TreeTxnT>(
    txn: &mut T,
    channel: &mut T::Graph,
    alive: &mut HashMap<Vertex<NodeId>, bool>,
    stack: &mut Vec<(Vertex<NodeId>, bool)>,
    b: Vertex<NodeId>,
) -> Result<bool, LocalApplyError<T>> {
    if let Some(r) = alive.get(&b) {
        return Ok(*r);
    }
    debug!("alive descendants");
    stack.clear();
    stack.push((b, false));
    while let Some((b, visited)) = stack.pop() {
        debug!("visiting {:?} {:?}", b, visited);
        if visited {
            if !alive.contains_key(&b) {
                alive.insert(b, false);
            }
            continue;
        }
        stack.push((b, true));
        for e in iter_adjacent(
            txn,
            channel,
            b,
            EdgeFlags::empty(),
            EdgeFlags::all() - EdgeFlags::DELETED - EdgeFlags::PARENT,
        )? {
            let e = e?;
            debug!("e = {:?}", e);
            if e.flag().contains(EdgeFlags::FOLDER) {
                let c = txn.find_block(channel, e.dest())?;
                stack.push((*c, false));
            } else {
                // This is a non-deleted non-folder edge.
                let c = txn.find_block(channel, e.dest())?;
                if is_alive(txn, channel, &c)? {
                    // The entire path is alive.
                    for (x, on_path) in stack.iter() {
                        if *on_path {
                            alive.insert(*x, true);
                        }
                    }
                }
            }
        }
    }
    Ok(*alive.get(&b).unwrap_or(&false))
}

fn collect_missing_contexts<T: GraphMutTxnT + TreeTxnT>(
    txn: &T,
    channel: &T::Graph,
    ws: &mut Workspace,
    change: &Change,
    change_id: NodeId,
    inodes: &mut HashSet<Position<NodeId>>,
) -> Result<(), LocalApplyError<T>> {
    inodes.extend(
        ws.missing_context
            .unknown_parents
            .drain(..)
            .map(|x| internal_pos(txn, &x.2, change_id).unwrap()),
    );
    for atom in change.changes.iter().flat_map(|r| r.iter()) {
        match atom {
            Atom::NewVertex(ref n) if !n.flag.is_folder() => {
                let inode = internal_pos(txn, &n.inode, change_id)?;
                if !inodes.contains(&inode) {
                    for up in n.up_context.iter() {
                        let up =
                            *txn.find_block_end(channel, internal_pos(txn, &up, change_id)?)?;
                        if !is_alive(txn, channel, &up)? {
                            inodes.insert(inode);
                            break;
                        }
                    }
                    for down in n.down_context.iter() {
                        let down =
                            *txn.find_block(channel, internal_pos(txn, &down, change_id)?)?;
                        let mut down_has_other_parents = false;
                        for e in iter_adjacent(
                            txn,
                            channel,
                            down,
                            EdgeFlags::PARENT,
                            EdgeFlags::all() - EdgeFlags::DELETED,
                        )? {
                            let e = e?;
                            if e.introduced_by() != change_id {
                                down_has_other_parents = true;
                                break;
                            }
                        }
                        if !down_has_other_parents {
                            inodes.insert(inode);
                            break;
                        }
                    }
                }
            }
            Atom::NewVertex(_) => {}
            Atom::EdgeMap(ref n) => {
                has_missing_edge_context(txn, channel, change_id, change, n, inodes)?;
            }
        }
    }
    Ok(())
}

fn has_missing_edge_context<T: GraphMutTxnT + TreeTxnT>(
    txn: &T,
    channel: &T::Graph,
    change_id: NodeId,
    change: &Change,
    n: &EdgeMap<Option<Hash>>,
    inodes: &mut HashSet<Position<NodeId>>,
) -> Result<(), LocalApplyError<T>> {
    let inode = internal_pos(txn, &n.inode, change_id)?;
    if !inodes.contains(&inode) {
        for e in n.edges.iter() {
            assert!(!e.flag.contains(EdgeFlags::PARENT));
            if e.flag.contains(EdgeFlags::DELETED) {
                trace!("repairing context deleted {:?}", e);
                if has_missing_context_deleted(txn, channel, change_id, |h| change.knows(&h), e)
                    .map_err(LocalApplyError::from_missing)?
                {
                    inodes.insert(inode);
                    break;
                }
            } else {
                trace!("repairing context nondeleted {:?}", e);
                if has_missing_context_nondeleted(txn, channel, change_id, e)
                    .map_err(LocalApplyError::from_missing)?
                {
                    inodes.insert(inode);
                    break;
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn repair_cyclic_paths<T: GraphMutTxnT + TreeTxnT>(
    txn: &mut T,
    channel: &mut T::Graph,
    ws: &mut Workspace,
) -> Result<(), LocalApplyError<T>> {
    let now = std::time::Instant::now();
    let mut files = std::mem::replace(&mut ws.missing_context.files, HashSet::default());
    for file in files.drain() {
        if file.is_empty() {
            if !is_rooted(txn, channel, file, ws)? {
                repair_edge(txn, channel, file, ws)?
            }
        } else {
            let f0 = EdgeFlags::FOLDER;
            let f1 = EdgeFlags::FOLDER | EdgeFlags::BLOCK | EdgeFlags::PSEUDO;
            let mut iter = iter_adjacent(txn, channel, file, f0, f1)?;
            if let Some(ee) = iter.next() {
                let ee = ee?;
                let dest = ee.dest().inode_vertex();
                if !is_rooted(txn, channel, dest, ws)? {
                    repair_edge(txn, channel, dest, ws)?
                }
            }
        }
    }
    ws.missing_context.files = files;
    crate::TIMERS.lock().unwrap().check_cyclic_paths += now.elapsed();
    Ok(())
}

fn repair_edge<T: GraphMutTxnT + TreeTxnT>(
    txn: &mut T,
    channel: &mut T::Graph,
    to0: Vertex<NodeId>,
    ws: &mut Workspace,
) -> Result<(), LocalApplyError<T>> {
    debug!("repair_edge {:?}", to0);
    let mut stack = vec![(to0, true, true, true)];
    ws.parents.clear();
    while let Some((current, _, al, anc_al)) = stack.pop() {
        if !ws.parents.insert(current) {
            continue;
        }
        debug!("repair_cyclic {:?}", current);
        if current != to0 {
            stack.push((current, true, al, anc_al));
        }
        if current.is_root() {
            debug!("root");
            break;
        }
        if let Some(&true) = ws.rooted.get(&current) {
            debug!("rooted");
            break;
        }
        let f = EdgeFlags::PARENT | EdgeFlags::FOLDER;
        let len = stack.len();
        for parent in iter_adjacent(txn, channel, current, f, EdgeFlags::all())? {
            let parent = parent?;
            if parent.flag().is_parent() {
                let anc = txn.find_block_end(channel, parent.dest())?;
                debug!("is_rooted, parent = {:?}", parent);
                let al = if let Some(e) = iter_adjacent(
                    txn,
                    channel,
                    *anc,
                    f,
                    f | EdgeFlags::BLOCK | EdgeFlags::PSEUDO,
                )?
                .next()
                {
                    e?;
                    true
                } else {
                    false
                };
                debug!("al = {:?}, flag = {:?}", al, parent.flag());
                stack.push((*anc, false, parent.flag().is_deleted(), al));
            }
        }
        if stack.len() == len {
            stack.pop();
        } else {
            (&mut stack[len..]).sort_unstable_by(|a, b| a.3.cmp(&b.3))
        }
    }
    let mut current = to0;
    for (next, on_path, del, _) in stack {
        if on_path {
            if del {
                put_graph_with_rev(
                    txn,
                    channel,
                    EdgeFlags::FOLDER | EdgeFlags::PSEUDO,
                    next,
                    current,
                    NodeId::ROOT,
                )?;
            }
            current = next
        }
    }
    ws.parents.clear();
    Ok(())
}

fn is_rooted<T: GraphTxnT + TreeTxnT>(
    txn: &T,
    channel: &T::Graph,
    v: Vertex<NodeId>,
    ws: &mut Workspace,
) -> Result<bool, LocalApplyError<T>> {
    let mut alive = false;
    assert!(v.is_empty());
    for e in iter_adjacent(txn, channel, v, EdgeFlags::empty(), EdgeFlags::all())? {
        let e = e?;
        if e.flag().contains(EdgeFlags::PARENT) {
            if e.flag() & (EdgeFlags::FOLDER | EdgeFlags::DELETED) == EdgeFlags::FOLDER {
                alive = true;
                break;
            }
        } else if !e.flag().is_deleted() {
            alive = true;
            break;
        }
    }
    if !alive {
        debug!("is_rooted, not alive");
        return Ok(true);
    }
    // Recycling ws.up_context and ws.parents as a stack and a
    // "visited" hashset, respectively.
    let stack = &mut ws.up_context;
    stack.clear();
    stack.push(v);
    let visited = &mut ws.parents;
    visited.clear();

    while let Some(to) = stack.pop() {
        debug!("is_rooted, pop = {:?}", to);
        if to.is_root() {
            stack.clear();
            for v in visited.drain() {
                ws.rooted.insert(v, true);
            }
            return Ok(true);
        }
        if !visited.insert(to) {
            continue;
        }
        if let Some(&rooted) = ws.rooted.get(&to) {
            if rooted {
                for v in visited.drain() {
                    ws.rooted.insert(v, true);
                }
                return Ok(true);
            } else {
                continue;
            }
        }
        let f = EdgeFlags::PARENT | EdgeFlags::FOLDER;
        for parent in iter_adjacent(
            txn,
            channel,
            to,
            f,
            f | EdgeFlags::PSEUDO | EdgeFlags::BLOCK,
        )? {
            let parent = parent?;
            debug!("is_rooted, parent = {:?}", parent);
            stack.push(*txn.find_block_end(channel, parent.dest())?)
        }
    }
    for v in visited.drain() {
        ws.rooted.insert(v, false);
    }
    Ok(false)
}

pub fn apply_root_change<
    R: rand::Rng,
    T: MutTxnT + TagMetadataMutTxnT<TagError = <T as GraphTxnT>::GraphError>,
    P: ChangeStore,
>(
    txn: &mut T,
    channel: &ChannelRef<T>,
    store: &P,
    rng: R,
) -> Result<Option<(Hash, u64, Merkle)>, ApplyError<P::Error, T>> {
    let mut change = {
        // If the graph already has a root.
        {
            let channel = channel.read();
            let gr = txn.graph(&*channel);
            for v in iter_adjacent(
                &*txn,
                gr,
                Vertex::ROOT,
                EdgeFlags::FOLDER,
                EdgeFlags::FOLDER | EdgeFlags::BLOCK,
            )? {
                let v = txn.find_block(gr, v?.dest())?;
                if v.start == v.end {
                    // Already has a root
                    return Ok(None);
                } else {
                    // Non-empty channel without a root
                    break;
                }
            }
            // If we are here, either the channel is empty, or it
            // isn't and doesn't have a root.
        }
        let root = Position {
            change: Some(Hash::NONE),
            pos: ChangePosition(0u64.into()),
        };
        let contents = rng
            .sample_iter(rand::distributions::Standard)
            .take(32)
            .collect();
        debug!(
            "change position {:?} {:?}",
            ChangePosition(1u64.into()),
            ChangePosition(1u64.into()).0.as_u64()
        );
        crate::change::LocalChange::make_change(
            txn,
            channel,
            vec![crate::change::Hunk::AddRoot {
                name: Atom::NewVertex(NewVertex {
                    up_context: vec![root],
                    down_context: Vec::new(),
                    start: ChangePosition(0u64.into()),
                    end: ChangePosition(0u64.into()),
                    flag: EdgeFlags::FOLDER | EdgeFlags::BLOCK,
                    inode: root,
                }),
                inode: Atom::NewVertex(NewVertex {
                    up_context: vec![Position {
                        change: None,
                        pos: ChangePosition(0u64.into()),
                    }],
                    down_context: Vec::new(),
                    start: ChangePosition(1u64.into()),
                    end: ChangePosition(1u64.into()),
                    flag: EdgeFlags::FOLDER | EdgeFlags::BLOCK,
                    inode: root,
                }),
            }],
            contents,
            crate::change::ChangeHeader::default(),
            Vec::new(),
        )?
    };
    let h = store
        .save_change(&mut change, |_, _| Ok(()))
        .map_err(ApplyError::Changestore)?;
    let (n, merkle) = apply_change(store, txn, &mut channel.write(), &h)?;
    Ok(Some((h, n, merkle)))
}
