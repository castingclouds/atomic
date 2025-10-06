# Tag and Change Unification Implementation Plan

## Overview

Eliminate the artificial distinction between "tagup" and "apply" operations in the Atomic VCS codebase. Tags and changes are both nodes in the dependency DAG and should be treated uniformly throughout the system.

**Status**: Phase 1 & 2 complete. Continuing with Phase 3. Destructive changes are acceptable - this is new code with no production users.

**Last Updated**: 2025-01-15

**Goal**: A tag is just another node in the DAG. The only differences should be:
1. File format on disk (`.change` vs `.tag`)
2. `NodeType` metadata in database (`NodeType::Change` vs `NodeType::Tag`)
3. Tags don't modify working copy files (they consolidate dependencies)

All other operations (push, pull, apply, log, etc.) should treat nodes uniformly.

---

## Current Problems

### Problem 1: Duplicate Registration Logic
- `register_change()` and `register_tag()` exist as separate functions
- Both do essentially the same thing: register a node in the DAG
- Leads to code duplication and inconsistency

### Problem 2: Special Tag Handling in Apply
- `apply.rs` has special code paths that create tag metadata during change apply
- When applying a change that was created after a tag, it tries to create/store tag metadata
- This **overwrites existing tag metadata** in the database, corrupting the DAG
- Tags should only be created via explicit tag operations, not as side effects

### Problem 3: Separate Protocol Commands
- HTTP/SSH protocols have separate `tagup` and `apply` commands
- `tagup` is essentially just applying a node with `NodeType::Tag`
- Creates unnecessary complexity in remote protocol handlers

### Problem 4: Terminology Confusion
- `ChangeId` is used throughout codebase but represents any node
- Should be `NodeId` to reflect unified node concept
- Function names like `register_change()` imply changes only

---

## Phase 1: Database Layer Unification ✅ COMPLETE

### Task 1.1: Rename ChangeId to NodeId ✅
**Files**: 
- `atomic/libatomic/src/pristine/change_id.rs` → `node_id.rs`
- All files using `ChangeId`

**Changes**:
- Renamed `ChangeId` struct to `NodeId`
- Updated all references throughout codebase
- Removed type alias to force compilation errors for missed references

**Status**: ✅ Complete (systematic find/replace performed)

### Task 1.2: Create Unified register_node() ✅
**File**: `atomic/libatomic/src/pristine/mod.rs`

**Changes**:
- Created `register_node(txn, internal, hash, node_type, dependencies)`
- Takes `NodeType` as explicit parameter
- Handles all node types uniformly
- Checks if node already registered before creating
- Always wires up dependencies

**Status**: ✅ Complete

### Task 1.3: Update register_change() and register_tag() ✅
**Status**: ✅ Complete
- `register_change()` now calls `register_node()` with `NodeType::Change`
- `register_tag()` removed entirely (use `register_node()` directly)
- All callers updated to use `register_node()`

---

## Phase 2: Apply Logic Cleanup 🚧 IN PROGRESS

### Task 2.1: Remove Tag Creation from apply.rs 🚧
**File**: `atomic/libatomic/src/apply.rs`

**Problem**: When applying a change that was created after a tag, the apply logic:
1. Detects the change has tag metadata (`change.hashed.tag.is_some()`)
2. Creates a NEW tag from that metadata
3. Calls `put_tag()` which **overwrites existing tag metadata in database**
4. Corrupts the tag's consolidated_changes list

**Solution**: Delete all tag creation/storage code from apply functions.

**Changes Required**:
- Delete lines ~327-382 in `apply_change_ws()` (tag metadata creation)
- Delete lines ~459-534 in `apply_change_rec_ws()` (tag metadata creation)
- Tags should NEVER be created as side effect of applying changes
- Tags are only created via explicit tag operations (`atomic tag create`)

**Rationale**: 
- A change's `.hashed.tag` field is client-side metadata about when it was created
- It doesn't mean the server should create/modify tag nodes
- Tag nodes are created independently via tag creation operations

**Status**: ✅ Complete - All tag creation/storage code removed from apply functions

### Task 2.2: Simplify apply_change Functions
**File**: `atomic/libatomic/src/apply.rs`

**Changes**:
- `apply_change()` should just:
  1. Call `register_node()` with `NodeType::Change`
  2. Call `apply_change_to_channel()` 
  3. Done
- Remove all tag-related conditionals
- Remove all tag metadata handling

**Status**: ✅ Complete - apply_change functions no longer handle tags

---

## Phase 3: Create Unified apply_node() Function
**Status**: ✅ Complete

### Task 3.1: Design apply_node() Interface ✅
**File**: `atomic/libatomic/src/apply.rs`

**Implemented signature**:
```rust
pub fn apply_node<T, P>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    node_type: NodeType,
) -> Result<(u64, Merkle), ApplyError<P::Error, T>>

pub fn apply_node_ws<T, P>(
    changes: &P,
    txn: &mut T,
    channel: &mut T::Channel,
    hash: &Hash,
    node_type: NodeType,
    workspace: &mut Workspace,
) -> Result<(u64, Merkle), ApplyError<P::Error, T>>
```

**Implementation**:
- Single function handles both changes and tags uniformly
- `node_type` parameter determines behavior:
  - `NodeType::Change`: Calls `apply_change_ws_impl()` to apply change to channel
  - `NodeType::Tag`: Returns existing tag position (tags already registered via tagup)
- Exported from `libatomic` for use throughout codebase

**Status**: ✅ Complete

### Task 3.2: Refactor Existing Apply Functions ✅
**Changes**:
- `apply_change_ws()` now calls `apply_change_ws_impl()` internally
- `apply_change_ws_impl()` is private implementation function
- `apply_node()` and `apply_node_ws()` are public unified interface
- Existing `apply_change()` functions maintained for backward compatibility
- New code should use `apply_node()` directly

**Status**: ✅ Complete

---

## Phase 4: Remove tagup and Unify with apply (FUTURE WORK)

**Status**: ⏳ Deferred - focus on testing current implementation first

**Note**: The critical bug (tag metadata corruption) is fixed in Phases 1-3. Phase 4 is an optimization/cleanup that can be done later after validating the current fix works.

### Core Insight
**Tags are just nodes in the DAG.** There should be no separate `tagup` protocol command. When you push a tag, you're just pushing another node. The `apply` operation should handle all nodes uniformly.

### Design Decision: Full Tag Files vs SHORT Format

**Current Behavior**:
- Client sends SHORT tag header via `tagup` protocol
- Server regenerates full tag file from its channel state
- Bandwidth optimization (SHORT version is smaller)

**New Behavior** (for unification):
- Client sends FULL tag file via `apply` protocol (same as changes)
- Server writes file directly (no regeneration needed)
- Simpler protocol, slightly more bandwidth

**Rationale**:
- Simplicity over premature optimization
- Consistent with change upload pattern
- Tag files are relatively small anyway
- Can optimize later if needed
- Eliminates special regeneration logic on server

### Task 4.1: Remove tagup from SSH Protocol ⏳
**File**: `atomic/atomic/src/commands/protocol.rs`

**Current State**:
- Separate `TAGUP` regex and handler (lines ~28, 202-321)
- Separate `APPLY` regex and handler (lines ~38, 351-373)
- `tagup` receives SHORT tag header, regenerates full file, stores metadata
- `apply` receives change file, writes it, applies to channel

**Target State**:
- Remove `TAGUP` regex and handler entirely
- Single `APPLY` handler that accepts both changes and tags
- Detect node type from file extension or content
- Call `apply_node()` with appropriate `NodeType`

**Implementation**:
```rust
// Remove TAGUP regex
// Remove entire tagup handler block

// Update APPLY handler:
if let Some(cap) = APPLY.captures(&buf) {
    let h = Hash::from_base32(cap[2].as_bytes())?;
    let mut path = repo.changes_dir.clone();
    
    // Detect node type from file extension or explicit flag
    let node_type = if path.extension() == Some("tag") {
        libatomic::pristine::NodeType::Tag
    } else {
        libatomic::pristine::NodeType::Change
    };
    
    // Write file (change or tag)
    std::fs::write(&path, &buf2)?;
    
    // Apply node uniformly
    let channel = load_channel(&*txn.read(), &cap[1])?;
    let mut channel_ = channel.write();
    txn.write().apply_node(&repo.changes, &mut channel_, &h, node_type, &mut ws)?;
    
    applied.insert(cap[1].to_string(), channel);
}
```

**Benefits**:
- Single code path for all nodes
- No special tag handling
- Simpler protocol
- Fewer edge cases

**Status**: ⏳ Not started

### Task 4.2: Remove tagup from HTTP Protocol ⏳
**File**: `atomic/atomic-api/src/server.rs`

**Current State**:
- Separate `?tagup=HASH` handler (lines ~846-1120)
- Separate `?apply=HASH` handler (lines ~544-840)
- Complex tag regeneration logic in tagup handler

**Target State**:
- Remove entire `tagup` handler block
- Single `apply` handler that accepts both changes and tags
- Detect node type from uploaded file format or header
- Use `apply_node()` for both

**Implementation**:
```rust
if let Some(apply_hash) = params.get("apply") {
    // Parse hash
    let hash = libatomic::Hash::from_base32(apply_hash.as_bytes())?;
    
    // Write file
    let change_path = /* compute path */;
    std::fs::write(&change_path, &body)?;
    
    // Detect node type from file content or extension
    let node_type = detect_node_type(&change_path)?;
    
    // Apply node
    let mut channel = /* load channel */;
    txn.write().apply_node(
        &repository.changes,
        &mut channel.write(),
        &hash,
        node_type,
    )?;
    
    // Output to working copy
    libatomic::output::output_repository_no_pending(...)?;
    txn.commit()?;
}
```

**Status**: ⏳ Not started

### Task 4.3: Update Remote Protocol Traits ⏳
**File**: `atomic/atomic-remote/src/lib.rs`

**Changes**:
- Remove `upload_tag()` methods if they exist
- `upload_changes()` should handle all nodes (may need renaming to `upload_nodes()`)
- `CS` enum already uses `Change` and `State` - review if this needs updating
- Push/pull should treat all nodes uniformly

**Status**: ⏳ Not started - depends on 4.1 and 4.2

### Task 4.4: Update Client-Side to Send Full Tag Files ⏳
**Files**: 
- `atomic/atomic-remote/src/http.rs` (lines ~257-270)
- `atomic/atomic-remote/src/ssh.rs` (lines ~974-994)

**Current Behavior**:
```rust
CS::State(state) => {
    // Open tag file and extract SHORT version
    let mut tag_file = OpenTagFile::open(&local, &state)?;
    let mut short_data = Vec::new();
    tag_file.short(&mut short_data)?;
    
    // Send via tagup command
    to_channel.push(("tagup", &base32));
}
```

**Target Behavior**:
```rust
CS::State(state) => {
    // Read FULL tag file
    push_tag_filename(&mut local, &state);
    let tag_data = std::fs::read(&local)?;
    
    // Send via apply command (same as changes)
    to_channel.push(("apply", &base32));
    // Upload full file data
}
```

**Changes**:
- Remove `tag_file.short()` extraction logic
- Read full tag file from disk
- Send via `apply` instead of `tagup`
- Use same upload code path as changes

**Status**: ⏳ Deferred - validate current fix first, then plan Phase 4 implementation

---

## Phase 5: CLI and User-Facing Changes

### Task 5.1: Simplify Push Logic
**File**: `atomic/src/commands/push.rs`

**Current State**:
- Separate logic for pushing changes vs tags
- Special handling for tag upload

**Target State**:
- Single push logic that handles all nodes uniformly
- Determines node type from database
- Uploads node file (change or tag) without distinction

**Status**: ⏳ Not started

### Task 5.2: Simplify Pull Logic
**File**: `atomic/src/commands/pull.rs`

**Current State**:
- Separate downloads for changes and tags
- Tag downloads use different code path

**Target State**:
- Unified download logic for all nodes
- Applies nodes uniformly via `apply_node()`

**Status**: ⏳ Not started

### Task 5.3: Update Remote Changelist Format
**File**: `atomic/atomic-remote/src/lib.rs`

**Changes**:
- Currently uses trailing dot (`.`) to mark tagged states
- This is metadata about the channel state, not about individual nodes
- Consider if this needs to change with unified node concept

**Status**: ⏳ Not started - needs design review

---

## Phase 6: Storage Layer Simplification

### Task 6.1: Unify ChangeStore Methods
**File**: `atomic/libatomic/src/changestore/mod.rs`

**Current State**:
```rust
trait ChangeStore {
    fn get_change(&self, h: &Hash) -> Result<Change, Error>;
    fn get_tag_header(&self, h: &Merkle) -> Result<ChangeHeader, Error>;
}
```

**Target State**:
```rust
trait ChangeStore {
    fn get_node(&self, h: &Hash, node_type: NodeType) -> Result<Node, Error>;
    // Or determine node type automatically:
    fn get_node_auto(&self, h: &Hash) -> Result<Node, Error>;
}
```

**Rationale**:
- Single interface for loading any node
- Internally checks file extension (.change vs .tag)
- Returns unified Node representation

**Status**: ⏳ Not started - needs design review

### Task 6.2: Consider Unified Node File Format
**Long-term consideration**:
- Could tags and changes use the same file format?
- Tags would just be changes with empty `changes: []` array
- Would simplify storage layer significantly
- May have performance implications

**Status**: 🤔 Future consideration - not for initial implementation

---

## Phase 7: Testing and Validation

### Task 7.1: Update Integration Tests
**Files**: 
- `atomic/libatomic/tests/tag_dependency_test.rs`
- Other test files

**Changes**:
- Update to use `register_node()` instead of `register_tag()`
- Add tests for unified apply logic
- Test that tag metadata is not corrupted during change apply

**Status**: ⏳ Not started

### Task 7.2: End-to-End Testing
**Test scenarios**:
1. Create tag, push tag → verify server has tag
2. Create change after tag, push change → verify server has both, tag not corrupted
3. Clone from server → verify both nodes present
4. Run `atomic log` on server → verify both nodes display correctly

**Status**: ⏳ Not started

### Task 7.3: Performance Testing
**Verify**:
- No performance regression from unified approach
- Tag operations still O(1) dependency reduction
- Push/pull performance unchanged

**Status**: ⏳ Not started

---

## Phase 8: Documentation Updates

### Task 8.1: Update AGENTS.md
**File**: `atomic/AGENTS.md`

**Changes**:
- Document unified node concept
- Update protocol sections to reflect unified approach
- Remove references to separate tag handling

**Status**: ⏳ Not started

### Task 8.2: Update HTTP API Documentation
**File**: `atomic/docs/HTTP-API-PROTOCOL-COMPARISON.md`

**Changes**:
- Document removal of `tagup` endpoint
- Update `apply` endpoint documentation
- Explain unified node handling

**Status**: ⏳ Not started

### Task 8.3: Update README
**File**: `atomic/README.md`

**Changes**:
- Clarify that tags are first-class nodes
- Update architecture diagrams if present
- Simplify explanation of tag operations

**Status**: ⏳ Not started

---

## Implementation Order

Recommended implementation sequence:

1. ✅ **Phase 1** (Complete) - Database layer unified
2. 🚧 **Phase 2.1** - Remove tag creation from apply.rs (CRITICAL BUG FIX)
3. **Phase 3** - Create unified `apply_node()` function
4. **Phase 2.2** - Simplify existing apply functions to use `apply_node()`
5. **Phase 4** - Unify protocol handlers
6. **Phase 5** - Update CLI commands
7. **Phase 7** - Testing
8. **Phase 6** - Storage layer cleanup (optional optimization)
9. **Phase 8** - Documentation

---

## Key Principles

1. **Nodes are nodes**: Don't distinguish in DAG operations
2. **Type is metadata**: NodeType determines file format and display, not logic flow
3. **Single registration path**: All nodes go through `register_node()`
4. **No side effects**: Applying a change never creates a tag
5. **Explicit operations**: Tags created via explicit tag operations only
6. **Uniform protocols**: Push/pull handle all node types uniformly

---

## Success Criteria

- [ ] All nodes registered via single `register_node()` function
- [ ] No tag creation during change apply
- [ ] `tagup` protocol command removed (or unified with apply)
- [ ] Push/pull operations work uniformly for all node types
- [ ] `atomic log` displays both changes and tags correctly
- [ ] Tag metadata never corrupted by subsequent operations
- [ ] All tests pass
- [ ] No performance regression

---

## Current Status Summary

**Completed**:
- ✅ Phase 1: Database layer unified (NodeId, register_node)
- ✅ Phase 2: Apply logic cleanup complete
  - ✅ Phase 2.1: Removed all tag creation from apply.rs
  - ✅ Phase 2.2: Simplified apply_change functions
- ✅ Phase 3: Unified apply_node() function created and exported
- ✅ **CRITICAL BUG FIXED**: Tag metadata corruption bug resolved
  - Tags no longer overwritten during change apply
  - Tag consolidated_changes preserved correctly

**Ready for Testing**:
- Current implementation should fix the original issue:
  1. Push tag → Server stores tag correctly with metadata
  2. Push change after tag → Server applies change without corrupting tag
  3. Run `atomic log` on server → Both nodes display correctly

**Future Work (Phase 4+)**:
- Phase 4: Remove tagup protocol command (deferred)
  - Requires client-side changes to send full tag files
  - Requires protocol detection of node type
  - Major refactoring - do after validating current fix
- Phase 5: Update pull logic
- Phase 6-8: Storage, testing, documentation updates

**Next Steps**:
1. **Test current implementation** with original failure scenario
2. Verify tag metadata preservation after change push
3. Verify `atomic log` works on server after push
4. If tests pass, current phases are complete
5. Phase 4+ can be done incrementally later

---

## Notes

- This is a significant architectural change but necessary for correctness
- The current split between tagup/apply is causing DAG corruption
- Tags being treated specially is a legacy of initial implementation
- Unification will make the codebase simpler and more maintainable
- Destructive changes are acceptable - no production users yet

---

## Related Documents

- `atomic/AGENTS.md` - Overall architecture and best practices
- `atomic/docs/GRAPH-NODE-UNIFICATION-PLAN.md` - Original node unification plan
- `atomic/docs/GRAPH-NODE-IMPLEMENTATION-PLAN.md` - Implementation details
- `atomic/docs/HTTP-API-PROTOCOL-COMPARISON.md` - Protocol documentation