# Graph Node Unification Implementation Plan

## Overview

Unify changes and tags into a single graph node architecture with a type discriminator. This enables:
- Tags to have internal IDs
- Unified dependency graph (changes can depend on tags seamlessly)
- Extensibility for future node types (merges, rollbacks, etc.)
- Semantic correctness: tags ARE nodes in the version DAG

**Design Pattern**: Single Table Inheritance with type discriminator
**Breaking Change**: Yes, acceptable in MVP phase
**Migration Strategy**: None needed - fresh start

---

## Phase 1: Foundation - Type System

### Task 1.1: Create NodeType Enum

**File**: `libatomic/src/pristine/node_type.rs`

**Implementation**:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum NodeType {
    Change = 0,
    Tag = 1,
    // Future: Merge = 2, Rollback = 3, etc.
}

impl NodeType {
    pub fn is_change(&self) -> bool {
        matches!(self, NodeType::Change)
    }

    pub fn is_tag(&self) -> bool {
        matches!(self, NodeType::Tag)
    }
}
```

**Test**: `libatomic/src/pristine/node_type.rs`
```rust
#[test]
fn test_node_type_discriminator() {
    assert_eq!(NodeType::Change as u8, 0);
    assert_eq!(NodeType::Tag as u8, 1);
    assert!(NodeType::Change.is_change());
    assert!(NodeType::Tag.is_tag());
}

#[test]
fn test_node_type_serialization() {
    let change_type = NodeType::Change;
    let serialized = bincode::serialize(&change_type).unwrap();
    let deserialized: NodeType = bincode::deserialize(&serialized).unwrap();
    assert_eq!(change_type, deserialized);
}
```

**Acceptance Criteria**:
- [ ] Enum compiles with explicit u8 representation
- [ ] Helper methods work correctly
- [ ] Serialization/deserialization works
- [ ] All tests pass

---

### Task 1.2: Add NodeType to Database Schema

**File**: `libatomic/src/pristine/sanakirja.rs`

**Changes**:
```rust
// Add to Root enum
pub enum Root {
    // ... existing entries
    NodeTypes,  // New table: ChangeId -> NodeType
}

// Add to GenericTxn
pub struct GenericTxn<T> {
    // ... existing fields
    pub(crate) node_types: Db<L64, u8>,  // Maps ChangeId -> NodeType as u8
}

// Update txn_begin to load node_types table
fn begin(txn: ::sanakirja::Txn<Arc<::sanakirja::Env>>) -> Option<Txn> {
    // ... existing loads
    debug!("Loading root_db: NodeTypes");
    let node_types = txn.root_db(Root::NodeTypes as usize)?;

    Some(Txn {
        // ... existing fields
        node_types,
    })
}
```

**Test**: `libatomic/tests/node_types_table.rs`
```rust
#[test]
fn test_node_types_table_creation() {
    let repo = test_repo();
    let txn = repo.pristine.txn_begin().unwrap();
    // Verify node_types table exists and is accessible
    assert!(txn.node_types.db > 0);
}
```

**Acceptance Criteria**:
- [ ] New table added to Root enum
- [ ] GenericTxn has node_types field
- [ ] Table is initialized in txn_begin
- [ ] Test verifies table exists
- [ ] Database opens without errors

---

### Task 1.3: Add NodeType Trait Methods

**File**: `libatomic/src/pristine/mod.rs`

**Add to GraphTxnT trait**:
```rust
pub trait GraphTxnT: Sized {
    // ... existing methods

    /// Get the type of a graph node (Change, Tag, etc.)
    fn get_node_type(
        &self,
        id: &ChangeId,
    ) -> Result<Option<NodeType>, TxnErr<Self::GraphError>>;
}
```

**Add to GraphMutTxnT trait**:
```rust
pub trait GraphMutTxnT: GraphTxnT {
    // ... existing methods

    /// Set the type of a graph node
    fn put_node_type(
        &mut self,
        id: &ChangeId,
        node_type: NodeType,
    ) -> Result<(), TxnErr<Self::GraphError>>;
}
```

**Test**: `libatomic/tests/node_type_trait.rs`
```rust
#[test]
fn test_node_type_operations() {
    let repo = test_repo();
    let mut txn = repo.pristine.mut_txn_begin().unwrap();

    let change_id = ChangeId::from(42);

    // Should be None initially
    assert!(txn.get_node_type(&change_id).unwrap().is_none());

    // Put node type
    txn.put_node_type(&change_id, NodeType::Change).unwrap();

    // Should retrieve correctly
    assert_eq!(
        txn.get_node_type(&change_id).unwrap(),
        Some(NodeType::Change)
    );
}
```

**Acceptance Criteria**:
- [ ] Trait methods added to GraphTxnT and GraphMutTxnT
- [ ] Compiles without errors
- [ ] Test passes

---

### Task 1.4: Implement NodeType Methods for Sanakirja

**File**: `libatomic/src/pristine/sanakirja.rs`

**Implementation**:
```rust
impl<T> GraphTxnT for GenericTxn<T> {
    // ... existing methods

    fn get_node_type(
        &self,
        id: &ChangeId,
    ) -> Result<Option<NodeType>, TxnErr<Self::GraphError>> {
        let id_u64: u64 = (*id).into();
        if let Some(type_u8) = btree::get(&self.txn, &self.node_types, &id_u64.into(), None)? {
            let node_type = match *type_u8 {
                0 => NodeType::Change,
                1 => NodeType::Tag,
                _ => return Err(TxnErr(SanakirjaError::InvalidNodeType.into())),
            };
            Ok(Some(node_type))
        } else {
            Ok(None)
        }
    }
}

impl GraphMutTxnT for MutTxn<()> {
    // ... existing methods

    fn put_node_type(
        &mut self,
        id: &ChangeId,
        node_type: NodeType,
    ) -> Result<(), TxnErr<Self::GraphError>> {
        let id_u64: u64 = (*id).into();
        let type_u8 = node_type as u8;
        btree::put(&mut self.txn, &mut self.node_types, &id_u64.into(), &type_u8)?;
        Ok(())
    }
}
```

**Test**: `libatomic/tests/sanakirja_node_types.rs`
```rust
#[test]
fn test_sanakirja_node_type_storage() {
    let repo = test_repo();
    let change_id = ChangeId::from(100);

    // Write in one transaction
    {
        let mut txn = repo.pristine.mut_txn_begin().unwrap();
        txn.put_node_type(&change_id, NodeType::Tag).unwrap();
        txn.commit().unwrap();
    }

    // Read in another transaction
    {
        let txn = repo.pristine.txn_begin().unwrap();
        assert_eq!(
            txn.get_node_type(&change_id).unwrap(),
            Some(NodeType::Tag)
        );
    }
}

#[test]
fn test_node_type_persistence() {
    let temp_dir = tempdir().unwrap();
    let repo_path = temp_dir.path();

    // Create and store
    {
        let repo = init_repo(repo_path);
        let mut txn = repo.pristine.mut_txn_begin().unwrap();
        txn.put_node_type(&ChangeId::from(1), NodeType::Change).unwrap();
        txn.put_node_type(&ChangeId::from(2), NodeType::Tag).unwrap();
        txn.commit().unwrap();
    }

    // Reopen and verify
    {
        let repo = open_repo(repo_path);
        let txn = repo.pristine.txn_begin().unwrap();
        assert_eq!(txn.get_node_type(&ChangeId::from(1)).unwrap(), Some(NodeType::Change));
        assert_eq!(txn.get_node_type(&ChangeId::from(2)).unwrap(), Some(NodeType::Tag));
    }
}
```

**Acceptance Criteria**:
- [ ] get_node_type and put_node_type implemented
- [ ] Storage and retrieval works
- [ ] Persistence across transactions verified
- [ ] All tests pass

---

## Phase 2: Update Change Registration

### Task 2.1: Modify register_change to Set NodeType

**File**: `libatomic/src/pristine/mod.rs`

**Changes to `register_change` function**:
```rust
pub(crate) fn register_change<T>(
    txn: &mut T,
    internal: &ChangeId,
    hash: &Hash,
    change: &Change,
) -> Result<(), TxnErr<T::GraphError>> {
    debug!("registering change {:?}", hash);
    let shash = hash.into();
    txn.put_external(internal, &shash)?;
    txn.put_internal(&shash, internal)?;

    // NEW: Set node type
    txn.put_node_type(internal, NodeType::Change)?;

    // Rest of existing logic unchanged
    for dep in change.dependencies.iter() {
        debug!("dep = {:?}", dep);
        if let Some(dep_internal_ref) = txn.get_internal(&dep.into())? {
            let dep_internal = *dep_internal_ref;
            debug!("{:?} depends on {:?}", internal, dep_internal);
            txn.put_revdep(&dep_internal, internal)?;
            txn.put_dep(internal, &dep_internal)?;
        } else {
            debug!(
                "{:?} has dependency {:?} without internal ID (not yet registered)",
                internal, dep
            );
        }
    }
    // ... rest unchanged
}
```

**Test**: `libatomic/tests/register_change_node_type.rs`
```rust
#[test]
fn test_register_change_sets_node_type() {
    let repo = test_repo();
    let mut txn = repo.pristine.arc_txn_begin().unwrap();

    let change = create_test_change("test change");
    let hash = save_change(&repo.changes, &change).unwrap();
    let internal_id = ChangeId::from(1);

    register_change(
        &mut *txn.write(),
        &internal_id,
        &hash,
        &change,
    ).unwrap();

    // Verify node type was set
    assert_eq!(
        txn.read().get_node_type(&internal_id).unwrap(),
        Some(NodeType::Change)
    );

    // Verify internal/external mapping still works
    assert_eq!(
        txn.read().get_internal(&hash.into()).unwrap(),
        Some(&internal_id)
    );
}
```

**Acceptance Criteria**:
- [ ] register_change calls put_node_type
- [ ] Node type is set to NodeType::Change
- [ ] Existing functionality unchanged
- [ ] Test passes

---

### Task 2.2: Create register_tag Function

**File**: `libatomic/src/pristine/mod.rs`

**New function**:
```rust
pub(crate) fn register_tag<T>(
    txn: &mut T,
    internal: &ChangeId,
    merkle: &Merkle,
    tag: &Tag,
) -> Result<(), TxnErr<T::GraphError>>
where
    T: GraphMutTxnT + TagMetadataMutTxnT<TagError = <T as GraphTxnT>::GraphError>,
{
    debug!("registering tag {:?}", merkle);

    // Convert Merkle to Hash for internal/external mapping
    let hash: Hash = (*merkle).into();
    let shash = hash.into();

    // Register in internal/external tables (just like changes!)
    txn.put_external(internal, &shash)?;
    txn.put_internal(&shash, internal)?;

    // Mark as tag
    txn.put_node_type(internal, NodeType::Tag)?;

    // Store tag-specific metadata
    let serialized = tag.to_serialized();
    txn.put_tag(&hash, &serialized)?;

    debug!("Successfully registered tag with internal ID {:?}", internal);
    Ok(())
}
```

**Test**: `libatomic/tests/register_tag.rs`
```rust
#[test]
fn test_register_tag_basic() {
    let repo = test_repo();
    let mut txn = repo.pristine.arc_txn_begin().unwrap();

    let tag = create_test_tag("v1.0.0");
    let merkle = calculate_tag_merkle(&tag);
    let internal_id = ChangeId::from(100);

    register_tag(
        &mut *txn.write(),
        &internal_id,
        &merkle,
        &tag,
    ).unwrap();

    // Verify node type
    assert_eq!(
        txn.read().get_node_type(&internal_id).unwrap(),
        Some(NodeType::Tag)
    );

    // Verify internal/external mapping
    let hash: Hash = merkle.into();
    assert_eq!(
        txn.read().get_internal(&hash.into()).unwrap(),
        Some(&internal_id)
    );

    // Verify tag metadata stored
    assert!(txn.read().get_tag(&hash).unwrap().is_some());
}

#[test]
fn test_register_tag_with_dependencies() {
    let repo = test_repo();
    let mut txn = repo.pristine.arc_txn_begin().unwrap();

    // Register a change first
    let change = create_test_change("base change");
    let change_hash = save_change(&repo.changes, &change).unwrap();
    let change_id = ChangeId::from(1);
    register_change(&mut *txn.write(), &change_id, &change_hash, &change).unwrap();

    // Create tag that consolidates the change
    let tag = create_test_tag_with_changes(vec![change_hash]);
    let merkle = calculate_tag_merkle(&tag);
    let tag_id = ChangeId::from(2);

    register_tag(&mut *txn.write(), &tag_id, &merkle, &tag).unwrap();

    // Both should be in the graph
    assert_eq!(
        txn.read().get_node_type(&change_id).unwrap(),
        Some(NodeType::Change)
    );
    assert_eq!(
        txn.read().get_node_type(&tag_id).unwrap(),
        Some(NodeType::Tag)
    );
}
```

**Acceptance Criteria**:
- [ ] register_tag function created
- [ ] Tags get internal IDs
- [ ] Tags are marked with NodeType::Tag
- [ ] Tag metadata is stored
- [ ] Tests pass

---

## Phase 3: Update Dependency Resolution

### Task 3.1: Fix Dependency Registration for Tags

**File**: `libatomic/src/pristine/mod.rs`

**Update register_change to handle tag dependencies**:
```rust
pub(crate) fn register_change<T>(
    txn: &mut T,
    internal: &ChangeId,
    hash: &Hash,
    change: &Change,
) -> Result<(), TxnErr<T::GraphError>> {
    // ... existing setup code

    // Process dependencies - now works for BOTH changes and tags!
    for dep in change.dependencies.iter() {
        debug!("Processing dependency: {:?}", dep);

        if let Some(dep_internal_ref) = txn.get_internal(&dep.into())? {
            let dep_internal = *dep_internal_ref;

            // Check what type of node this dependency is
            if let Some(dep_type) = txn.get_node_type(&dep_internal)? {
                debug!(
                    "{:?} depends on {:?} (type: {:?})",
                    internal, dep_internal, dep_type
                );

                // Add to dependency graph (works for both changes and tags!)
                txn.put_revdep(&dep_internal, internal)?;
                txn.put_dep(internal, &dep_internal)?;
            } else {
                warn!("Dependency {:?} has no node type set", dep_internal);
            }
        } else {
            debug!(
                "Dependency {:?} not found in internal map (not yet registered)",
                dep
            );
        }
    }
    // ... rest unchanged
}
```

**Test**: `libatomic/tests/tag_dependencies.rs`
```rust
#[test]
fn test_change_depends_on_tag() {
    let repo = test_repo();
    let mut txn = repo.pristine.arc_txn_begin().unwrap();

    // Register a tag first
    let tag = create_test_tag("v1.0.0");
    let tag_merkle = calculate_tag_merkle(&tag);
    let tag_id = ChangeId::from(1);
    register_tag(&mut *txn.write(), &tag_id, &tag_merkle, &tag).unwrap();

    // Create a change that depends on the tag
    let tag_hash: Hash = tag_merkle.into();
    let change = create_test_change_with_deps("new work", vec![tag_hash]);
    let change_hash = save_change(&repo.changes, &change).unwrap();
    let change_id = ChangeId::from(2);

    // Register the change - should handle tag dependency!
    register_change(&mut *txn.write(), &change_id, &change_hash, &change).unwrap();

    // Verify dependency graph
    let deps: Vec<_> = txn.read().iter_dep(&change_id).unwrap().collect();
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].unwrap().1, tag_id);

    // Verify reverse dependency
    let revdeps: Vec<_> = txn.read().iter_revdep(&tag_id).unwrap().collect();
    assert_eq!(revdeps.len(), 1);
    assert_eq!(revdeps[0].unwrap().1, change_id);
}

#[test]
fn test_mixed_dependencies() {
    let repo = test_repo();
    let mut txn = repo.pristine.arc_txn_begin().unwrap();

    // Register a change
    let change1 = create_test_change("change 1");
    let change1_hash = save_change(&repo.changes, &change1).unwrap();
    let change1_id = ChangeId::from(1);
    register_change(&mut *txn.write(), &change1_id, &change1_hash, &change1).unwrap();

    // Register a tag
    let tag = create_test_tag("v1.0.0");
    let tag_merkle = calculate_tag_merkle(&tag);
    let tag_id = ChangeId::from(2);
    register_tag(&mut *txn.write(), &tag_id, &tag_merkle, &tag).unwrap();

    // Create change that depends on BOTH
    let tag_hash: Hash = tag_merkle.into();
    let change2 = create_test_change_with_deps(
        "change 2",
        vec![change1_hash, tag_hash],
    );
    let change2_hash = save_change(&repo.changes, &change2).unwrap();
    let change2_id = ChangeId::from(3);

    register_change(&mut *txn.write(), &change2_id, &change2_hash, &change2).unwrap();

    // Verify both dependencies recorded
    let deps: Vec<_> = txn.read()
        .iter_dep(&change2_id)
        .unwrap()
        .map(|r| r.unwrap().1)
        .collect();
    assert_eq!(deps.len(), 2);
    assert!(deps.contains(&change1_id));
    assert!(deps.contains(&tag_id));
}
```

**Acceptance Criteria**:
- [ ] Changes can depend on tags
- [ ] Dependency graph includes tag relationships
- [ ] Reverse dependencies work
- [ ] Mixed dependencies (change + tag) work
- [ ] All tests pass

---

## Phase 4: Update Apply Logic

### Task 4.1: Update apply_change to Call register_tag for Tags

**File**: `libatomic/src/apply.rs`

**Modify apply_change_ws function**:
```rust
pub fn apply_change_ws<T, C>(
    // ... params
) -> Result<(u64, Merkle), ApplyError<C::Error, T::GraphError>> {
    // ... existing apply logic

    // If this change contains consolidating tag metadata, register it as a tag
    if let Some(ref tag_metadata) = change.hashed.tag {
        let (n, merkle) = result;

        debug!("Change contains tag metadata, registering as tag");

        // Reconstruct Tag struct
        let tag = Tag {
            state: merkle,
            message: change.hashed.header.message.clone(),
            timestamp: change.hashed.header.timestamp,
            consolidated_changes: tag_metadata.consolidated_changes.clone(),
            dependency_reduction: tag_metadata.dependency_reduction,
        };

        // Get internal ID for this change
        let internal_id = txn.get_internal(&hash.into())
            .map_err(ApplyError::Txn)?
            .ok_or_else(|| ApplyError::ChangeNotInChannel { hash: *hash })?;
