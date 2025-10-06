# Graph Node Unification - Quick Reference

## Core Concept
**Tags and Changes are both nodes in the dependency DAG - just different types**

## Key Types

```rust
pub enum NodeType {
    Change = 0,  // Regular change with hunks
    Tag = 1,     // Consolidating tag
}

pub struct ChangeId(L64);  // Universal ID for ALL graph nodes
```

## Database Schema

```
graph_nodes (conceptual - stored across tables):
  ├── internal_id: ChangeId (PK)
  ├── hash: Hash (unique)
  └── node_type: NodeType (u8)

dependencies:
  ├── from_id: ChangeId (FK to any node)
  └── to_id: ChangeId (FK to any node)
```

## Registration Pattern

```rust
// Changes
register_change(txn, internal_id, hash, change);
  → put_external(internal_id, hash)
  → put_internal(hash, internal_id)
  → put_node_type(internal_id, NodeType::Change)  // NEW!
  → process dependencies (now works for tags too!)

// Tags  
register_tag(txn, internal_id, merkle, tag);
  → put_external(internal_id, hash)               // NEW!
  → put_internal(hash, internal_id)               // NEW!
  → put_node_type(internal_id, NodeType::Tag)     // NEW!
  → put_tag(hash, serialized)
```

## Dependency Resolution

```rust
// BEFORE (broken):
for dep in change.dependencies {
    if let Some(dep_id) = txn.get_internal(dep)? {
        // Only worked for changes
    } else {
        // Tags had no internal ID - skip
    }
}

// AFTER (fixed):
for dep in change.dependencies {
    let dep_id = txn.get_internal(dep)?.unwrap();  // Works for ALL nodes!
    txn.put_dep(internal, dep_id)?;
    txn.put_revdep(dep_id, internal)?;
}
```

## Header Loading

```rust
fn get_header(&self, h: &Hash) -> Result<ChangeHeader, Self::Error> {
    // Try tag first
    if let Ok(header) = self.get_tag_header(&merkle) {
        return Ok(header);
    }
    // Fall back to change
    Ok(self.get_change(h)?.hashed.header)
}
```

## Implementation Phases

1. **Phase 1**: Add NodeType enum and database table
2. **Phase 2**: Make register_change set node_type
3. **Phase 3**: Create register_tag with internal ID
4. **Phase 4**: Fix dependency resolution loop
5. **Phase 5**: Make get_header type-aware
6. **Phase 6**: End-to-end integration test

## Testing Commands

```bash
# Unit tests
cargo test test_node_type_serialization
cargo test test_store_and_retrieve_node_type
cargo test test_register_change_sets_node_type
cargo test test_register_tag_creates_internal_id
cargo test test_change_can_depend_on_tag

# Integration test
cargo test test_full_tag_dependency_workflow

# All tests
cargo test
```

## Key Files to Modify

- `libatomic/src/pristine/mod.rs` - NodeType enum, register_tag function
- `libatomic/src/pristine/sanakirja.rs` - Database schema, trait impls
- `libatomic/src/apply.rs` - Call register_tag when creating tags
- `libatomic/src/changestore/mod.rs` - Smart get_header()

## Success Criteria

✅ `txn.get_internal(&tag_hash)` returns `Some(internal_id)`
✅ `txn.get_node_type(&internal_id)` returns `Some(NodeType::Tag)`
✅ Dependencies work: change → tag, tag → change, tag → tag
✅ `atomic log` doesn't crash on tag dependencies
✅ `get_header()` works for both changes and tags

## Future Extensions

- Add `NodeType::Merge` for merge commits
- Add `NodeType::Rollback` for rollback operations
- Attribution metadata (separate table, 1:1 FK to internal_id)
- Review metadata (separate table, 1:1 FK to internal_id)

## Version

Database version bump: **1.1.0 → 2.0.0**
Breaking change: Old databases will be rejected (OK for MVP)