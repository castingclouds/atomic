# Phase 5: Complete DAG Unification - Implementation Plan

**Status**: 🚀 In Progress  
**Started**: January 2025  
**Goal**: Unify all node operations into a single, consistent DAG-based apply system

---

## Executive Summary

Phase 5 completes the architectural transformation started in Phase 4 by eliminating the dual code paths for changes and tags, creating a truly unified DAG where all nodes (changes and tags) are handled through the same operations.

**Current State** (Post-Phase 4):
- ✅ Unified `Node` type with explicit hash and state
- ✅ Consistent `*_nodes()` API across remote operations
- ✅ Type-safe factory methods and semantic checks
- ⚠️  **Dual code paths**: Separate apply logic for changes vs tags
- ⚠️  **Tag application incomplete**: Tags not fully integrated into apply workflow

**Target State** (Phase 5):
- 🎯 Single `apply_node()` operation for all node types
- 🎯 Unified dependency resolution (changes can depend on tags)
- 🎯 Consistent channel state updates
- 🎯 Simplified transaction management
- 🎯 Complete DAG unification

---

## Current Architecture Analysis

### Existing Apply Functions

```rust
// Change application (fully implemented)
pub fn apply_change_ws<T, P>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    workspace: &mut Workspace,
) -> Result<(u64, Merkle), ApplyError<P::Error, T>>

// Node application (partially implemented)
pub fn apply_node_ws<T, P>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    node_type: NodeType,
    workspace: &mut Workspace,
) -> Result<(u64, Merkle), ApplyError<P::Error, T>>
```

### Current Tag Handling

Tags are currently handled through **separate code paths**:

1. **Tag Upload**: `tagup` command generates full tag file from channel state
2. **Tag Download**: Client receives short tag, regenerates full tag
3. **Tag Application**: Tags are added to `channel.tags` table, but not fully integrated into apply workflow

### Problems with Current Approach

1. **Dual Code Paths**: Changes and tags have separate application logic
2. **Incomplete Integration**: `apply_node_ws` for tags just returns success without proper state handling
3. **Missing Functionality**: Tags should update channel state markers
4. **Dependency Confusion**: Not clear how changes can depend on tags
5. **Transaction Complexity**: Different transaction patterns for changes vs tags

---

## Phase 5 Goals

### 1. Unified Apply Operation

**Objective**: Single `apply_node()` function that works identically for changes and tags

**Current Issues**:
```rust
// libatomic/src/apply.rs:290-310
crate::pristine::NodeType::Tag => {
    // Tags don't modify the channel, just register them
    // Tag registration happens via explicit tag operations (tagup)
    // Here we just return success - tags should already be registered
    // ... incomplete implementation
}
```

**Target Implementation**:
```rust
pub fn apply_node_ws<T, P>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    node_type: NodeType,
    workspace: &mut Workspace,
) -> Result<(u64, Merkle), ApplyError<P::Error, T>> {
    match node_type {
        NodeType::Change => apply_change_to_channel(txn, channel, hash, workspace),
        NodeType::Tag => apply_tag_to_channel(txn, channel, hash, workspace),
    }
}
```

### 2. Unified Dependency Resolution

**Objective**: Changes can depend on both changes and tags uniformly

**Current State**: Dependencies are resolved through `get_internal()` which works for both, but tag dependencies are not fully tested.

**Target**: 
- Changes can explicitly depend on tags (consolidation points)
- Tag dependencies are resolved through the same `iter_revdep()` mechanism
- Dependency graph shows both change and tag nodes uniformly

### 3. Unified Channel State Updates

**Objective**: Both changes and tags update channel state consistently

**Current Approach**:
- Changes update via `put_changes()` → updates `channel.changes` table
- Tags update via `put_tags()` → updates `channel.tags` table
- **State calculation** differs between the two

**Target Approach**:
- Both changes and tags call `update_channel_state()`
- Unified state progression: `State A` → `(apply node)` → `State B`
- Tags create explicit state markers in the DAG

### 4. Simplified Transaction Management

**Objective**: Same transaction pattern for all node types

**Current Complexity**:
```rust
// Different patterns for changes vs tags
match node.node_type {
    NodeType::Change => {
        txn.apply_change_rec_ws(&repo.changes, &mut channel, &node.hash, &mut ws)?;
    }
    NodeType::Tag => {
        if let Some(n) = txn.channel_has_state(&channel.states, &node.state.into())? {
            txn.put_tags(&mut channel.tags, n.into(), &node.state)?;
        }
    }
}
```

**Target Simplification**:
```rust
// Single unified pattern
txn.apply_node(&repo.changes, &mut channel, &node)?;
```

---

## Implementation Plan

### Step 1: Complete `apply_node_ws()` for Tags

**File**: `libatomic/src/apply.rs`

**Current Implementation** (lines 266-310):
- Tags just return success without proper state handling
- No channel state update
- No position tracking

**Required Changes**:
1. Implement proper tag application logic
2. Update channel state after tag application
3. Track tag position in channel log
4. Handle tag dependencies correctly

**Pseudocode**:
```rust
NodeType::Tag => {
    // 1. Verify tag is registered
    let internal = get_or_register_tag(txn, hash)?;
    
    // 2. Calculate position in channel
    let position = txn.apply_counter(channel);
    
    // 3. Get current state
    let current_state = current_state(txn, channel)?;
    
    // 4. Verify tag state matches channel state
    // (tags can only be applied at the state they represent)
    if tag_state != current_state {
        return Err(ApplyError::TagStateMismatch);
    }
    
    // 5. Add tag to channel
    txn.put_tags(&mut channel.tags, position.into(), &current_state)?;
    
    // 6. Tag doesn't change state (it marks current state)
    Ok((position, current_state))
}
```

### Step 2: Unified `apply_node_rec()` with Dependencies

**Create**: New function that applies nodes recursively with dependency resolution

```rust
pub fn apply_node_rec_ws<T, P>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    node_type: NodeType,
    workspace: &mut Workspace,
    deps_only: bool,
) -> Result<(), ApplyError<P::Error, T>> {
    // 1. Check if already applied
    let internal = if let Some(i) = txn.get_internal(&hash.into())? {
        i
    } else {
        return Err(ApplyError::ChangeNotFound);
    };
    
    if txn.get_changeset(txn.changes(channel), internal)?.is_some() {
        return Ok(()); // Already applied
    }
    
    // 2. Apply dependencies first (unified for both changes and tags)
    for dep in get_dependencies(changes, hash, node_type)? {
        let dep_type = get_node_type(txn, &dep)?;
        apply_node_rec_ws(changes, txn, channel, &dep, dep_type, workspace, false)?;
    }
    
    // 3. Apply this node if not deps_only
    if !deps_only {
        apply_node_ws(changes, txn, channel, hash, node_type, workspace)?;
    }
    
    Ok(())
}
```

### Step 3: Update Remote Operations

**Files**: 
- `atomic-remote/src/lib.rs`
- `atomic-remote/src/local.rs`
- `atomic-remote/src/ssh.rs`
- `atomic-remote/src/http.rs`

**Current Pattern** (in pull operations):
```rust
// First pass: Apply all changes
for h in to_download.iter().rev() {
    if h.is_change() {
        txn.apply_change_rec_ws(&repo.changes, &mut channel, &h.hash, &mut ws)?;
    }
}

// Second pass: Apply all tags
for h in to_download.iter().rev() {
    if h.is_tag() {
        if let Some(n) = txn.channel_has_state(&channel.states, &h.state.into())? {
            txn.put_tags(&mut channel.tags, n.into(), &h.state)?;
        }
    }
}
```

**Target Pattern**:
```rust
// Single pass: Apply all nodes in order
for node in to_download.iter().rev() {
    txn.apply_node_rec(&repo.changes, &mut channel, &node.hash, node.node_type)?;
}
```

### Step 4: Update CLI Commands

**Files**:
- `atomic/src/commands/apply.rs`
- `atomic/src/commands/pushpull.rs`
- `atomic/src/commands/tag.rs`
- `atomic/src/commands/fork.rs`

**Replace**: All `apply_change_rec()` calls with `apply_node_rec()`

**Example**:
```rust
// Before
txn.apply_change_rec(&repo.changes, &mut channel, &hash)?;

// After
txn.apply_node_rec(&repo.changes, &mut channel, &hash, NodeType::Change)?;
```

### Step 5: Extend `MutTxnTExt` Trait

**File**: `libatomic/src/lib.rs`

**Add New Methods**:
```rust
pub trait MutTxnTExt: MutTxnT {
    // Unified node application
    fn apply_node<C: changestore::ChangeStore>(
        &mut self,
        changes: &C,
        channel: &mut Self::Channel,
        hash: &Hash,
        node_type: NodeType,
    ) -> Result<(u64, Merkle), ApplyError<C::Error, Self>>;
    
    fn apply_node_rec<C: changestore::ChangeStore>(
        &mut self,
        changes: &C,
        channel: &mut Self::Channel,
        hash: &Hash,
        node_type: NodeType,
    ) -> Result<(), ApplyError<C::Error, Self>>;
    
    fn apply_node_rec_ws<C: changestore::ChangeStore>(
        &mut self,
        changes: &C,
        channel: &mut Self::Channel,
        hash: &Hash,
        node_type: NodeType,
        workspace: &mut ApplyWorkspace,
    ) -> Result<(), ApplyError<C::Error, Self>>;
}
```

### Step 6: Deprecate Old Functions (Gradual)

**Approach**: Keep old functions as thin wrappers initially

```rust
// Old function becomes a wrapper
pub fn apply_change<T, P>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
) -> Result<(u64, Merkle), ApplyError<P::Error, T>> {
    // Just call the unified version
    apply_node(changes, txn, channel, hash, NodeType::Change)
}
```

**Later**: Mark as deprecated and eventually remove

---

## Testing Strategy

### Unit Tests

1. **Test: Apply tag to channel**
   - Create tag at specific state
   - Apply tag to channel
   - Verify tag is in `channel.tags` table
   - Verify position is tracked

2. **Test: Change depends on tag**
   - Create tag at state S1
   - Create change that depends on tag
   - Apply change
   - Verify dependency resolution works

3. **Test: Apply nodes in mixed order**
   - Create: Change A, Tag T1, Change B (depends on T1), Tag T2
   - Apply in various orders
   - Verify all resolve correctly

### Integration Tests

1. **Test: Push/pull with mixed changes and tags**
   - Local repo with changes and tags
   - Remote repo
   - Push mixed nodes
   - Pull on another client
   - Verify consistency

2. **Test: Clone with tags**
   - Repo with several tags
   - Clone repo
   - Verify all tags are present
   - Verify tag positions are correct

3. **Test: Tag consolidation workflow**
   - Make several changes
   - Create consolidating tag
   - Make more changes (depending on tag)
   - Push/pull
   - Verify tag dependency tracking

---

## Migration Strategy

### Phase 5A: Implementation (Non-Breaking)
1. Implement `apply_node_ws()` for tags
2. Add `apply_node_rec_ws()` with full dependency resolution
3. Add new trait methods to `MutTxnTExt`
4. Keep old functions as wrappers
5. Add comprehensive tests

### Phase 5B: Gradual Migration (Low Risk)
1. Update remote operations to use `apply_node_rec()`
2. Update CLI commands one by one
3. Update tests to use new API
4. Mark old functions as deprecated

### Phase 5C: Cleanup (Breaking)
1. Remove old `apply_change_*()` wrapper functions
2. Update documentation
3. Create migration guide

---

## Success Criteria

- [ ] `apply_node_ws()` correctly applies tags to channels
- [ ] `apply_node_rec_ws()` resolves dependencies for both changes and tags
- [ ] Changes can explicitly depend on tags
- [ ] All remote operations use unified `apply_node_rec()`
- [ ] All CLI commands use unified API
- [ ] Zero code duplication between change and tag application
- [ ] All existing tests pass
- [ ] New integration tests pass
- [ ] Documentation updated

---

## Risks & Mitigation

### Risk 1: Breaking Existing Tag Workflows
**Mitigation**: Keep old tag operations working in parallel, migrate gradually

### Risk 2: Performance Regression
**Mitigation**: Benchmark apply operations before and after, optimize hot paths

### Risk 3: Dependency Resolution Complexity
**Mitigation**: Extensive testing with complex dependency graphs

### Risk 4: State Mismatch Issues
**Mitigation**: Add validation that tag state matches channel state before applying

---

## Timeline Estimate

- **Phase 5A** (Implementation): 4-6 hours
- **Phase 5B** (Migration): 2-3 hours  
- **Phase 5C** (Cleanup): 1-2 hours
- **Testing & Validation**: 2-3 hours

**Total**: 9-14 hours

---

## Next Actions

1. ✅ Create Phase 5 implementation plan (this document)
2. 🔄 Implement `apply_tag_to_channel()` helper function
3. 🔄 Complete `apply_node_ws()` tag branch
4. 🔄 Implement `apply_node_rec_ws()` with unified dependencies
5. 🔄 Add new trait methods
6. 🔄 Write comprehensive tests
7. 🔄 Update remote operations
8. 🔄 Update CLI commands
9. 🔄 Documentation updates
10. 🔄 Final validation and cleanup

---

**Document Version**: 1.0  
**Last Updated**: January 2025  
**Status**: Ready to Begin Implementation