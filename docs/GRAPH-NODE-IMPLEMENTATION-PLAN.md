# Graph Node Unification Implementation Plan

## Goal
Unify changes and tags into a single graph node system with type discrimination, enabling proper dependency resolution and extensibility.

## Architecture Overview

### Current (Broken)
```
Changes: internal_id ↔ hash mapping ✓
Tags: NO internal_id mapping ✗
Dependencies: Only work between changes ✗
```

### Target (Fixed)
```
GraphNodes: internal_id ↔ hash mapping (for ALL node types) ✓
  ├── node_type: 'change' or 'tag' (structural discrimination)
  ├── change_metadata (1:1 FK for changes)
  ├── tag_metadata (1:1 FK for tags)
  └── attribution_metadata (optional, any node)
Dependencies: Work between ANY node types ✓
```

## Implementation Tasks

---

## Phase 1: Add Node Type Discrimination

### Task 1.1: Create NodeType Enum
**File**: `libatomic/src/pristine/mod.rs`

**Add**:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum NodeType {
    Change = 0,
    Tag = 1,
}

impl NodeType {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(NodeType::Change),
            1 => Some(NodeType::Tag),
            _ => None,
        }
    }
}
```

**Test**: `libatomic/tests/node_type_test.rs`
```rust
#[test]
fn test_node_type_serialization() {
    assert_eq!(NodeType::Change as u8, 0);
    assert_eq!(NodeType::Tag as u8, 1);
    assert_eq!(NodeType::from_u8(0), Some(NodeType::Change));
    assert_eq!(NodeType::from_u8(1), Some(NodeType::Tag));
    assert_eq!(NodeType::from_u8(2), None);
}
```

**Verify**: `cargo test test_node_type_serialization`

---

### Task 1.2: Add Node Type Table to Sanakirja
**File**: `libatomic/src/pristine/sanakirja.rs`

**Modify** `Root` enum:
```rust
pub enum Root {
    Version,
    // ... existing
    NodeTypes,  // NEW: Maps ChangeId -> NodeType (u8)
}
```

**Modify** `GenericTxn`:
```rust
pub struct GenericTxn<T> {
    // ... existing fields
    pub(crate) node_types: Db<ChangeId, u8>,  // NEW
}
```

**Test**: Verify database initialization doesn't crash
```rust
#[test]
fn test_db_with_node_types_table() {
    let tmp = tempdir().unwrap();
    let mut pristine = Pristine::new(tmp.path()).unwrap();
    let txn = pristine.mut_txn_begin().unwrap();
    txn.commit().unwrap();
}
```

**Verify**: `cargo test test_db_with_node_types_table`

---

### Task 1.3: Add Node Type Trait Methods
**File**: `libatomic/src/pristine/mod.rs`

**Add to** `GraphTxnT`:
```rust
pub trait GraphTxnT: Sized {
    // ... existing methods
    
    /// Get the node type for an internal ID
    fn get_node_type(&self, id: &ChangeId) -> Result<Option<NodeType>, TxnErr<Self::GraphError>>;
}
```

**Add to** `GraphMutTxnT`:
```rust
pub trait GraphMutTxnT: GraphTxnT {
    // ... existing methods
    
    /// Set the node type for an internal ID
    fn put_node_type(&mut self, id: &ChangeId, node_type: NodeType) -> Result<(), TxnErr<Self::GraphError>>;
    
    /// Delete node type entry
    fn del_node_type(&mut self, id: &ChangeId) -> Result<bool, TxnErr<Self::GraphError>>;
}
```

**Test**: Compilation check only at this stage
```bash
cargo check
```

---

### Task 1.4: Implement Node Type Operations in Sanakirja
**File**: `libatomic/src/pristine/sanakirja.rs`

**Implement for** `GenericTxn`:
```rust
impl<T> GraphTxnT for GenericTxn<T> {
    // ... existing implementations
    
    fn get_node_type(&self, id: &ChangeId) -> Result<Option<NodeType>, TxnErr<Self::GraphError>> {
        if let Some(type_u8) = btree::get(&self.txn, &self.node_types, id, None)? {
            Ok(NodeType::from_u8(*type_u8))
        } else {
            Ok(None)
        }
    }
}

impl GraphMutTxnT for MutTxn<()> {
    // ... existing implementations
    
    fn put_node_type(&mut self, id: &ChangeId, node_type: NodeType) -> Result<(), TxnErr<Self::GraphError>> {
        let type_u8 = node_type as u8;
        btree::put(&mut self.txn, &mut self.node_types, id, &type_u8)?;
        Ok(())
    }
    
    fn del_node_type(&mut self, id: &ChangeId) -> Result<bool, TxnErr<Self::GraphError>> {
        Ok(btree::del(&mut self.txn, &mut self.node_types, id, None)?)
    }
}
```

**Test**: `libatomic/tests/node_type_storage_test.rs`
```rust
#[test]
fn test_store_and_retrieve_node_type() {
    let tmp = tempdir().unwrap();
    let mut pristine = Pristine::new(tmp.path()).unwrap();
    
    let change_id = ChangeId::from(42);
    
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.put_node_type(&change_id, NodeType::Change).unwrap();
        txn.commit().unwrap();
    }
    
    {
        let txn = pristine.txn_begin().unwrap();
        let node_type = txn.get_node_type(&change_id).unwrap();
        assert_eq!(node_type, Some(NodeType::Change));
    }
}

#[test]
fn test_node_type_tags() {
    let tmp = tempdir().unwrap();
    let mut pristine = Pristine::new(tmp.path()).unwrap();
    
    let tag_id = ChangeId::from(100);
    
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.put_node_type(&tag_id, NodeType::Tag).unwrap();
        txn.commit().unwrap();
    }
    
    {
        let txn = pristine.txn_begin().unwrap();
        let node_type = txn.get_node_type(&tag_id).unwrap();
        assert_eq!(node_type, Some(NodeType::Tag));
    }
}
```

**Verify**: `cargo test test_store_and_retrieve_node_type test_node_type_tags`

---

## Phase 2: Update Change Registration

### Task 2.1: Modify register_change to Set Node Type
**File**: `libatomic/src/pristine/mod.rs`

**Modify** `register_change`:
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
    
    // NEW: Mark as a change node
    txn.put_node_type(internal, NodeType::Change)?;
    
    // ... rest of existing code (dependencies, etc.)
}
```

**Test**: `libatomic/tests/change_registration_test.rs`
```rust
#[test]
fn test_register_change_sets_node_type() {
    let tmp = tempdir().unwrap();
    let mut pristine = Pristine::new(tmp.path()).unwrap();
    
    let change_id = ChangeId::from(1);
    let hash = Hash::from_base32(b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap();
    
    let change = Change::default(); // minimal test change
    
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        register_change(&mut txn, &change_id, &hash, &change).unwrap();
        txn.commit().unwrap();
    }
    
    {
        let txn = pristine.txn_begin().unwrap();
        let node_type = txn.get_node_type(&change_id).unwrap();
        assert_eq!(node_type, Some(NodeType::Change));
    }
}
```

**Verify**: `cargo test test_register_change_sets_node_type`

---

## Phase 3: Add Tag Registration with Internal IDs

### Task 3.1: Create register_tag Function
**File**: `libatomic/src/pristine/mod.rs`

**Add new function**:
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
    
    // Convert Merkle to Hash for unified storage
    let hash: Hash = merkle.into();
    let shash = hash.into();
    
    // Store in internal/external maps (NEW!)
    txn.put_external(internal, &shash)?;
    txn.put_internal(&shash, internal)?;
    
    // Mark as tag node (NEW!)
    txn.put_node_type(internal, NodeType::Tag)?;
    
    // Store tag-specific metadata (existing)
    let serialized = tag.to_serialized();
    txn.put_tag(&hash, &serialized)?;
    
    debug!("Successfully registered tag with internal ID {:?}", internal);
    Ok(())
}
```

**Test**: `libatomic/tests/tag_registration_test.rs`
```rust
#[test]
fn test_register_tag_creates_internal_id() {
    let tmp = tempdir().unwrap();
    let mut pristine = Pristine::new(tmp.path()).unwrap();
    
    let tag_id = ChangeId::from(1000);
    let merkle = Merkle::zero();
    let tag = Tag::default(); // minimal test tag
    
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        register_tag(&mut txn, &tag_id, &merkle, &tag).unwrap();
        txn.commit().unwrap();
    }
    
    {
        let txn = pristine.txn_begin().unwrap();
        
        // Verify node type is Tag
        let node_type = txn.get_node_type(&tag_id).unwrap();
        assert_eq!(node_type, Some(NodeType::Tag));
        
        // Verify internal/external mapping exists
        let hash: Hash = (&merkle).into();
        let retrieved_id = txn.get_internal(&hash.into()).unwrap();
        assert_eq!(retrieved_id, Some(&tag_id));
    }
}
```

**Verify**: `cargo test test_register_tag_creates_internal_id`

---

### Task 3.2: Call register_tag When Creating Tags
**File**: `libatomic/src/apply.rs`

**Modify** `apply_change_ws` where tags are created:
```rust
// If this change contains consolidating tag metadata, store it
if let Some(ref tag_metadata) = change.hashed.tag {
    let (n, merkle) = result;
    
    // Store tag metadata (existing)
    let hash: Hash = (&merkle).into();
    let serialized = SerializedTag::from_tag(tag_metadata);
    txn.put_tag(&hash, &serialized)?;
    
    // NEW: Register tag with internal ID
    let tag_internal_id = ChangeId::from(n);  // Reuse the change's position as ID
    register_tag(txn, &tag_internal_id, &merkle, tag_metadata)?;
    
    // Add to channel tags table (existing)
    txn.put_tags(tags, n.into(), &merkle)?;
}
```

**Test**: Integration test
```rust
#[test]
fn test_applying_change_with_tag_registers_tag() {
    let repo = test_repo().unwrap();
    
    // Create a change with tag metadata
    let mut change = create_test_change();
    change.hashed.tag = Some(Tag::default());
    
    // Apply the change
    apply_change(&repo, &change).unwrap();
    
    // Verify tag was registered with internal ID
    let txn = repo.pristine.txn_begin().unwrap();
    let merkle = compute_merkle_for_change(&change);
    let hash: Hash = (&merkle).into();
    
    let tag_id = txn.get_internal(&hash.into()).unwrap();
    assert!(tag_id.is_some());
    
    let node_type = txn.get_node_type(tag_id.unwrap()).unwrap();
    assert_eq!(node_type, Some(NodeType::Tag));
}
```

**Verify**: `cargo test test_applying_change_with_tag_registers_tag`

---

## Phase 4: Fix Dependency Resolution

### Task 4.1: Update register_change Dependency Loop
**File**: `libatomic/src/pristine/mod.rs`

**Modify** `register_change`:
```rust
pub(crate) fn register_change<T>(
    txn: &mut T,
    internal: &ChangeId,
    hash: &Hash,
    change: &Change,
) -> Result<(), TxnErr<T::GraphError>> {
    // ... existing code ...
    
    for dep in change.dependencies.iter() {
        debug!("Processing dependency: {:?}", dep);
        
        // Try to find internal ID (works for both changes AND tags now!)
        if let Some(dep_internal_ref) = txn.get_internal(&dep.into())? {
            let dep_internal = *dep_internal_ref;
            
            // Check what type it is (optional logging)
            if let Some(node_type) = txn.get_node_type(&dep_internal)? {
                debug!("{:?} depends on {:?} (type: {:?})", internal, dep_internal, node_type);
            }
            
            // Wire up dependency (works for ANY node type!)
            txn.put_revdep(&dep_internal, internal)?;
            txn.put_dep(internal, &dep_internal)?;
        } else {
            // This is now an ERROR - all dependencies should have internal IDs
            error!("Dependency {:?} not found in internal mapping", dep);
            return Err(TxnErr(GraphError::DependencyNotFound));
        }
    }
    
    // ... rest of existing code ...
}
```

**Test**: `libatomic/tests/dependency_resolution_test.rs`
```rust
#[test]
fn test_change_can_depend_on_tag() {
    let tmp = tempdir().unwrap();
    let mut pristine = Pristine::new(tmp.path()).unwrap();
    
    // Register a tag
    let tag_id = ChangeId::from(100);
    let tag_merkle = Merkle::zero();
    let tag = Tag::default();
    let tag_hash: Hash = (&tag_merkle).into();
    
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        register_tag(&mut txn, &tag_id, &tag_merkle, &tag).unwrap();
        txn.commit().unwrap();
    }
    
    // Register a change that depends on the tag
    let change_id = ChangeId::from(101);
    let change_hash = Hash::from_base32(b"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB").unwrap();
    let mut change = Change::default();
    change.hashed.dependencies = vec![tag_hash];
    
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        register_change(&mut txn, &change_id, &change_hash, &change).unwrap();
        txn.commit().unwrap();
    }
    
    // Verify dependency was recorded
    {
        let txn = pristine.txn_begin().unwrap();
        
        // Check dep table
        let deps: Vec<_> = txn.iter_dep(&change_id).unwrap().collect();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].unwrap(), tag_id);
        
        // Check revdep table
        let revdeps: Vec<_> = txn.iter_revdep(&tag_id).unwrap().collect();
        assert_eq!(revdeps.len(), 1);
        assert_eq!(revdeps[0].unwrap(), change_id);
    }
}
```

**Verify**: `cargo test test_change_can_depend_on_tag`

---

## Phase 5: Fix Header Loading

### Task 5.1: Make get_header Type-Aware
**File**: `libatomic/src/changestore/mod.rs`

**Modify** `get_header`:
```rust
fn get_header(&self, h: &Hash) -> Result<ChangeHeader, Self::Error> {
    // Try to determine if this is a tag by checking if tag file exists
    // (This is temporary - ideally we'd check node_type from db)
    let merkle = crate::Merkle::from_bytes(&h.to_bytes());
    
    // Try tag first (faster path for tags)
    if let Ok(header) = self.get_tag_header(&merkle) {
        debug!("Loaded header for tag: {}", h.to_base32());
        return Ok(header);
    }
    
    // Fall back to regular change
    debug!("Loading header for change: {}", h.to_base32());
    Ok(self.get_change(h)?.hashed.header)
}
```

**Test**: `libatomic/tests/header_loading_test.rs`
```rust
#[test]
fn test_get_header_works_for_changes() {
    let repo = test_repo();
    let change = create_test_change(&repo);
    
    let header = repo.changes.get_header(&change.hash).unwrap();
    assert_eq!(header.message, "Test change");
}

#[test]
fn test_get_header_works_for_tags() {
    let repo = test_repo();
    let tag = create_test_tag(&repo);
    let tag_hash: Hash = (&tag.merkle).into();
    
    let header = repo.changes.get_header(&tag_hash).unwrap();
    assert_eq!(header.message, "Test tag");
}

#[test]
fn test_get_header_for_nonexistent_hash() {
    let repo = test_repo();
    let fake_hash = Hash::from_base32(b"ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ").unwrap();
    
    let result = repo.changes.get_header(&fake_hash);
    assert!(result.is_err());
}
```

**Verify**: `cargo test test_get_header_works`

---

## Phase 6: End-to-End Integration Test

### Task 6.1: Full Push/Pull/Log Workflow Test
**File**: `libatomic/tests/integration/tag_dependency_workflow.rs`

**Test**:
```rust
#[test]
fn test_full_tag_dependency_workflow() {
    // Setup client and server repos
    let client_repo = test_repo().unwrap();
    let server_repo = test_repo().unwrap();
    
    // 1. Client creates a tag
    let tag = create_tag_on_channel(&client_repo, "v1.0.0");
    let tag_hash: Hash = (&tag.merkle).into();
    
    // 2. Client creates a change that depends on the tag
    let change = create_change_with_dependencies(&client_repo, vec![tag_hash]);
    
    // 3. Push tag to server
    push_to_server(&client_repo, &server_repo, &tag_hash);
    
    // 4. Push change to server
    push_to_server(&client_repo, &server_repo, &change.hash);
    
    // 5. Verify server can resolve dependencies
    {
        let txn = server_repo.pristine.txn_begin().unwrap();
        
        // Get change's internal ID
        let change_internal = txn.get_internal(&change.hash.into()).unwrap().unwrap();
        
        // Get its dependencies
        let deps: Vec<_> = txn.iter_dep(change_internal).unwrap().collect();
        assert_eq!(deps.len(), 1, "Change should have one dependency");
        
        // Verify dependency is the tag
        let dep_internal = deps[0].as_ref().unwrap();
        let dep_node_type = txn.get_node_type(dep_internal).unwrap();
        assert_eq!(dep_node_type, Some(NodeType::Tag), "Dependency should be a tag");
    }
    
    // 6. Run log command - should NOT fail!
    let log_output = run_log_command(&server_repo);
    assert!(log_output.contains(&change.hash.to_base32()));
    assert!(log_output.contains(&tag_hash.to_base32()));
    
    // 7. Verify headers load for both
    let change_header = server_repo.changes.get_header(&change.hash).unwrap();
    assert_eq!(change_header.message, change.hashed.header.message);
    
    let tag_header = server_repo.changes.get_header(&tag_hash).unwrap();
    assert_eq!(tag_header.message, tag.header.message);
}
```

**Verify**: `cargo test test_full_tag_dependency_workflow`

---

## Phase 7: Update Database Version

### Task 7.1: Bump Version Number
**File**: `libatomic/src/pristine/sanakirja.rs`

**Update**:
```rust
const VERSION_MAJOR: u64 = 2;  // Was 1
const VERSION_MINOR: u64 = 0;  // Was 1
const VERSION_PATCH: u64 = 0;
```

**Test**: Verify old databases are rejected
```rust
#[test]
fn test_old_database_rejected() {
    // Create a v1.1.0 database
    let old_db = create_legacy_database();
    
    // Try to open with v2.0.0 code
    let result = Pristine::open(old_db.path());
    
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), SanakirjaError::Version));
}
```

**Verify**: `cargo test test_old_database_rejected`

---

## Phase 8: Clean Up and Documentation

### Task 8.1: Add Documentation Comments
**Files**: All modified files

**Add**:
- Module-level docs explaining the graph node architecture
- Function-level docs for `register_tag`
- Examples in doc comments

### Task 8.2: Update AGENTS.md
**File**: `atomic/AGENTS.md`

**Add section**:
```markdown
## Graph Node Architecture

Atomic uses a unified graph node system where both changes and tags are first-class nodes in the dependency DAG:

- **NodeType**: Discriminator enum (Change, Tag)
- **Internal IDs**: All graph nodes (changes and tags) receive internal IDs
- **Dependencies**: Can point to any node type
- **Metadata**: Type-specific data stored in separate tables
```

---

## Testing Checklist

Before considering complete, verify:

- [ ] All unit tests pass
- [ ] Integration tests pass
- [ ] Can create changes
- [ ] Can create tags
- [ ] Can create changes that depend on tags
- [ ] Can push changes and tags to server
- [ ] Server can resolve tag dependencies
- [ ] `atomic log` works with tag dependencies
- [ ] Headers load correctly for both changes and tags
- [ ] Old databases are properly rejected

---

## Success Criteria

✅ Tags have internal IDs
✅ Tags appear in internal/external mapping tables
✅ Dependencies work uniformly for changes and tags
✅ `get_header()` works for both changes and tags
✅ No special-casing in dependency resolution
✅ Graph algorithms work uniformly
✅ Foundation for future node types (merge, rollback, etc.)

---

## Notes

- This is a breaking change - no migration needed (MVP mode)
- Node types are stored as u8 for efficiency
- Attribution metadata will be added in separate phase
- Keep tag-specific metadata in separate tables (1:1 FK)