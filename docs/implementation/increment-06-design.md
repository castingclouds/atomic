# Increment 6: Dependency Resolution - Design Document

**Status**: 📋 Planning  
**Target Date**: 2025-01-16  
**Estimated Duration**: 4-5 days  
**Dependencies**: Increment 5 (complete) ✅  

---

## Executive Summary

Increment 6 implements the core value proposition of consolidating tags: **automatic dependency simplification**. This increment modifies the `record` and `apply` operations to recognize consolidating tags, allowing new changes to depend on a single tag instead of accumulating dependencies on all previous changes.

**Key Insight**: Users already have full control over dependencies via `atomic record -e` (edit mode). Tags simply need to traverse the DAG correctly and automatically include all reachable changes.

**Answer**: Keep it simple - tags consolidate via DAG traversal, users control insertion via `-e`.

---

## Problem Statement

### Current State (After Increment 5)

```bash
# Record 5 changes
atomic record -m "change 1"  # Dependencies: [] (0)
atomic record -m "change 2"  # Dependencies: [1] (1)
atomic record -m "change 3"  # Dependencies: [1, 2] (2)
atomic record -m "change 4"  # Dependencies: [1, 2, 3] (3)
atomic record -m "change 5"  # Dependencies: [1, 2, 3, 4] (4)

# Create consolidating tag
atomic tag create v1.0 --consolidate -m "Release 1.0"
# Tag metadata: {consolidated_changes: 5, dependency_count_before: 10}

# Record another change
atomic record -m "change 6"  
# ❌ PROBLEM: Dependencies: [1, 2, 3, 4, 5] (5)
#    Should be: [tag v1.0] (1) ✅
```

**The Problem**: Consolidating tags exist but don't affect dependency resolution.

### Desired State (After Increment 6)

```bash
# Same setup as above...
atomic tag create v1.0 --consolidate -m "Release 1.0"

# Record another change
atomic record -m "change 6"  
# ✅ Dependencies: [tag v1.0] (1)
#    The tag represents changes 1-5

# Subsequent changes
atomic record -m "change 7"  
# ✅ Dependencies: [change 6] (1)
#    Which transitively depends on tag v1.0
```

**The Solution**: New changes automatically depend on the latest consolidating tag.

---

## Design Overview

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│              Atomic Record with -e (Edit Mode)                   │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  1. Compute working copy changes (existing)                     │
│  2. Open editor with change file (existing)                     │
│     │                                                            │
│     ├─ Show Dependencies section                                │
│     ├─ User can manually edit dependencies                      │
│     └─ User controls exact DAG position                         │
│                                                                   │
│  3. Create change record with user-specified dependencies       │
│  4. Store in database                                           │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
                                                                    
┌─────────────────────────────────────────────────────────────────┐
│              Consolidating Tag Creation                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  1. Start from channel tip                                      │
│  2. Traverse DAG backwards (NEW/ENHANCED) ──────┐               │
│     │                                            │               │
│     ├─ For each change:                         │               │
│     │  │                                         │               │
│     │  ├─ If regular dependency: continue       │               │
│     │  │                                         │               │
│     │  └─ If tag dependency: EXPAND TAG         │               │
│     │     └─ Include all its consolidated       │               │
│     │        changes and continue traversing    │               │
│     │                                            │               │
│     └─ Collect ALL reachable changes            │               │
│                                                  │               │
│  3. Create tag with full change list            │               │
│  4. Tag automatically includes inserted changes │               │
│                                                  │               │
└──────────────────────────────────────────────────────────────────┘
```

---

## Detailed Design

### 1. Tag Expansion During DAG Traversal

**File**: `libatomic/src/pristine/consolidating_tag.rs`

The key operation is expanding tag references during DAG traversal:

```rust
/// Traverse the DAG from a starting point, expanding any tag references
pub fn traverse_with_tag_expansion<T: ChannelTxnT + ConsolidatingTagTxnT>(
    txn: &T,
    start: Hash,
) -> Result<Vec<Hash>, TxnErr<T::GraphError>> {
    let mut all_changes = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = vec![start];
    
    while let Some(hash) = stack.pop() {
        if visited.contains(&hash) {
            continue;
        }
        visited.insert(hash);
        all_changes.push(hash);
        
        // Get dependencies for this change
        let deps = get_change_dependencies(txn, &hash)?;
        
        for dep_hash in deps {
            // Check if this dependency is a consolidating tag
            if let Some(tag) = txn.get_consolidating_tag(&dep_hash)? {
                // EXPAND: Add all changes from the tag to the stack
                stack.extend(tag.consolidated_changes.clone());
            } else {
                // Regular change dependency
                stack.push(dep_hash);
            }
        }
    }
    
    Ok(all_changes)
}
```

**Change File Format** (unchanged - existing format works):
```toml
# In change file (via atomic record -e):
message = 'My change'
timestamp = '2025-01-15T11:00:00.000000Z'

[[authors]]
key = '3rAPLeTJdKPwzE3eFHGEeeH7iW9fPTXGotmUPeWAekph'

# Dependencies - user edits this section
[0] BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB # C2
[1] EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE # C2.5

# Hunks
...
```

### 2. Enhanced Tag Creation

**File**: `libatomic/src/pristine/consolidating_tag.rs`

Modify tag creation to use DAG traversal with expansion:

```rust
impl ConsolidatingTag {
    /// Create a new consolidating tag by traversing the current DAG.
    /// 
    /// This automatically includes ALL changes reachable from the channel tip,
    /// expanding any tag references encountered during traversal.
    pub fn from_channel<T: ChannelTxnT + ConsolidatingTagTxnT>(
        txn: &T,
        channel: &ChannelRef<T>,
        tag_hash: Hash,
        previous_consolidation: Option<Hash>,
    ) -> Result<Self, TxnErr<T::GraphError>> {
        let channel_read = channel.read();
        
        // Get the current channel state (tip)
        let tip = txn.current_state(&*channel_read)?;
        
        // Traverse and expand to get ALL reachable changes
        let consolidated_changes = traverse_with_tag_expansion(txn, tip)?;
        
        let dependency_count_before = calculate_dependencies(&consolidated_changes);
        let consolidated_change_count = consolidated_changes.len() as u64;
        
        Ok(Self {
            tag_hash,
            channel: txn.name(&*channel_read).to_string(),
            consolidation_timestamp: chrono::Utc::now().timestamp() as u64,
            previous_consolidation,
            dependency_count_before,
            consolidated_change_count,
        })
    }
}
```

**Key Behavior**:
- Starts from channel tip
- Traverses ALL ancestors
- Expands tag references automatically
- Inserted changes are naturally included if reachable

### 3. No Changes Needed to Record Operation

**The existing `atomic record -e` mechanism already provides full control.**

Users can already:
- ✅ Edit dependencies manually in the change file
- ✅ Insert changes at arbitrary DAG positions
- ✅ Merge multiple paths
- ✅ Depend on specific changes or tags

**Example workflow** (already works):
```bash
# Create change with custom dependencies
atomic record -e -m "Insert between C2 and C3"

# Editor opens - user manually sets:
# [0] BBBB... # C2 (not latest!)

# Save and close - change is inserted at specified position
```

**No modifications needed to record operation.**

### 4. Apply Operation (Future Work)

**File**: `libatomic/src/apply/mod.rs`

When a change references a consolidating tag as a dependency, the apply operation needs to expand it.

**Note**: This is future work for when changes can explicitly depend on tags (not yet implemented in Increment 5).

```rust
/// Check if a dependency hash refers to a consolidating tag
fn resolve_dependency<T: ChannelTxnT + ConsolidatingTagTxnT>(
    txn: &T,
    dep_hash: &Hash,
) -> Result<Vec<Hash>, ApplyError> {
    // Check if this is a consolidating tag
    if let Some(tag) = txn.get_consolidating_tag(dep_hash)? {
        // Expand: return all consolidated changes
        Ok(tag.consolidated_changes.clone())
    } else {
        // Regular change: return as-is
        Ok(vec![*dep_hash])
    }
}
```

**For Increment 6**: Focus on tag creation/expansion only. Apply modification can be deferred to Increment 7.

### 5. No CLI Changes Needed for Record

**The existing `atomic record -e` already provides the interface.**

Users control dependencies by:
1. Running `atomic record -e`
2. Editing the Dependencies section in their editor
3. Saving and closing

**Example**:
```bash
atomic record -e -m "My change"
# Editor opens with Dependencies section
# User edits: [0] <any_hash> # Can be change or tag
# Save and close
```

**No new CLI flags needed for Increment 6.**

### 6. Storage Considerations

**No changes needed to change file format.**

The existing format already supports:
- Multiple dependencies per change
- Dependencies can be any hash (change or tag)
- User controls via `-e` flag

**Existing format works perfectly:**
```toml
# Dependencies
[0] AAAA... # Can be a regular change
[1] BBBB... # Can be a consolidating tag (it's just a hash)
[2] CCCC... # Can be another change
```

The system doesn't need to distinguish at storage time. During tag creation, the traversal algorithm checks each dependency to see if it's a tag (and expands if so).

---

## Implementation Plan (Simplified)

### Phase 1: Tag Traversal with Expansion (Day 1)

1. ✅ Implement `traverse_with_tag_expansion()` function
2. ✅ Add logic to detect and expand tag references
3. ✅ Unit tests for traversal algorithm

**Deliverable**: Can traverse DAG and expand tags

### Phase 2: Enhanced Tag Creation (Day 2)

1. ✅ Update `ConsolidatingTag::from_channel()` to use traversal
2. ✅ Store full list of consolidated changes
3. ✅ Add `consolidated_changes: Vec<Hash>` field
4. ✅ Integration tests

**Deliverable**: Tags automatically include all reachable changes

### Phase 3: Testing with Manual Insertion (Day 3)

1. ✅ Test manual insertion workflow with `-e`
2. ✅ Verify tags include inserted changes
3. ✅ Test multiple insertion scenarios
4. ✅ Document workflow

**Deliverable**: Full workflow tested and documented

### Phase 4: Performance & Optimization (Day 4)

1. ✅ Profile tag creation performance
2. ✅ Add caching if needed
3. ✅ Optimize traversal algorithm
4. ✅ Benchmark tests

**Deliverable**: Performance meets requirements

### Phase 5: Documentation (Day 5)

1. ✅ User guide for insertion workflow
2. ✅ API documentation
3. ✅ Example scenarios
4. ✅ Best practices

**Deliverable**: Complete documentation

---

## Testing Strategy

### Unit Tests

1. **Dependency Serialization**
   ```rust
   #[test]
   fn test_tag_dependency_roundtrip() { ... }
   
   #[test]
   fn test_backward_compatibility() { ... }
   ```

2. **Tag Lookup**
   ```rust
   #[test]
   fn test_latest_consolidating_tag() { ... }
   
   #[test]
   fn test_no_consolidating_tags() { ... }
   ```

3. **Dependency Resolution**
   ```rust
   #[test]
   fn test_compute_dependencies_with_tag() { ... }
   
   #[test]
   fn test_compute_dependencies_without_tag() { ... }
   ```

4. **Tag Expansion**
   ```rust
   #[test]
   fn test_expand_consolidating_tag() { ... }
   
   #[test]
   fn test_expand_incremental_tag() { ... }
   ```

### Integration Tests

1. **Basic Workflow**
   ```bash
   # Test automatic dependency on tag
   atomic init test-repo
   atomic record -m "c1"
   atomic record -m "c2"
   atomic tag create v1.0 --consolidate
   atomic record -m "c3"
   # Verify c3 depends on v1.0 tag
   ```

2. **Incremental Consolidation**
   ```bash
   atomic tag create v1.0 --consolidate
   atomic record -m "c4"
   atomic tag create v1.1 --consolidate --since v1.0
   atomic record -m "c5"
   # Verify c5 depends on v1.1 tag
   ```

3. **Explicit Dependencies**
   ```bash
   atomic tag create v1.0 --consolidate
   atomic record -m "c4" --no-auto-consolidate
   # Verify c4 depends on c1-c3, not tag
   
   atomic record -m "c5" --after-tag v1.0
   # Verify c5 explicitly depends on v1.0
   ```

4. **Apply with Tags**
   ```bash
   # Create change with tag dependency on one machine
   # Clone to another machine
   atomic clone original new-clone
   atomic apply change-with-tag-dep.change
   # Verify it resolves and applies correctly
   ```

### Performance Tests

1. **Tag Lookup Performance**
   - Measure time to find latest tag with 1, 10, 100, 1000 tags
   - Target: < 1ms for 100 tags

2. **Dependency Resolution Performance**
   - Compare traditional vs. tag-based dependency resolution
   - Target: 10x faster with tags for 100+ changes

3. **Memory Usage**
   - Measure memory during tag expansion
   - Ensure no memory leaks

---

## Edge Cases & Error Handling

### 1. No Consolidating Tags

**Scenario**: `atomic record` on channel with no consolidating tags

**Behavior**: Use traditional dependency resolution (current behavior)

**Test**: Verify backward compatibility

### 2. Multiple Channels

**Scenario**: Different channels have different consolidating tags

**Behavior**: Each channel tracks its own latest tag independently

**Test**: Create tags on multiple channels, verify isolation

### 3. Tag Deleted

**Scenario**: Apply a change that references a deleted tag

**Behavior**: Error with clear message: "Consolidating tag HASH not found"

**Test**: Delete tag, try to apply change

### 4. Incomplete Tag

**Scenario**: Tag claims 10 changes but channel only has 8

**Behavior**: Error during apply: "Invalid consolidating tag"

**Test**: Manually corrupt tag metadata, try to expand

### 5. Merge Scenarios

**Scenario**: Merging branches with different consolidating tags

**Behavior**: Depend on both tags (or manual resolution)

**Test**: Create branches with different tags, merge

### 6. Circular Dependencies

**Scenario**: Tag depends on changes that depend on tag (impossible but check)

**Behavior**: Cycle detection during apply

**Test**: Attempt to create circular dependency

---

## Performance Considerations

### Caching Strategy

```rust
/// Cache for latest consolidating tags per channel
pub struct TagCache {
    cache: HashMap<String, (Hash, ConsolidatingTag)>,
    max_age: Duration,
}

impl TagCache {
    /// Get or fetch the latest consolidating tag
    pub fn get_or_fetch<T: ConsolidatingTagTxnT>(
        &mut self,
        txn: &T,
        channel_name: &str,
    ) -> Result<Option<(Hash, ConsolidatingTag)>, TxnErr<T::TagError>> {
        // Check cache first
        if let Some(entry) = self.cache.get(channel_name) {
            return Ok(Some(entry.clone()));
        }
        
        // Cache miss - fetch from database
        if let Some(tag) = txn.latest_consolidating_tag(channel_name)? {
            self.cache.insert(channel_name.to_string(), tag.clone());
            Ok(Some(tag))
        } else {
            Ok(None)
        }
    }
    
    /// Invalidate cache when new tag is created
    pub fn invalidate(&mut self, channel_name: &str) {
        self.cache.remove(channel_name);
    }
}
```

### Database Optimization

**Current**: O(n) scan through tags on channel

**Future Enhancement**: Add index:
```rust
// Map: channel_name -> latest_consolidating_tag_hash
consolidating_tags_by_channel: Db<&str, Hash>
```

---

## Migration Strategy

### Backward Compatibility

1. **Old Changes**: Work unchanged (no tag dependencies)
2. **New Changes**: Can use tag dependencies
3. **Mixed Mode**: Can have both old and new changes in same repo

### Migration Path

No migration needed! This is a **pure addition**:
- Old repos continue to work
- New features available immediately
- Users opt-in by creating consolidating tags

---

## Open Questions

### Q1: Should we store which specific changes are in a tag?

**Decision**: **YES** - Store explicit list.

```rust
pub struct ConsolidatingTag {
    // ... existing fields ...
    
    /// Explicit list of changes consolidated by this tag
    pub consolidated_changes: Vec<Hash>,
}
```

**Rationale**: 
- ✅ Required for accurate expansion during traversal
- ✅ Enables validation
- ✅ Allows inserted changes to be detected
- ✅ Storage cost is acceptable (list of hashes)

**Status**: Will implement in Increment 6

### Q2: How to handle tag dependencies across repositories?

**Scenario**: Pulling changes that reference tags not in local repo

**Options**:
1. Fetch tags automatically (like Git)
2. Error and require manual tag pull
3. Expand dependencies at push time

**Decision**: Defer to Increment 8 (Remote Operations)

### Q3: Should tags be immutable?

**Current**: Tags are immutable once created

**Alternative**: Allow tag updates

**Decision**: Keep immutable (simpler, more predictable)

---

## Success Criteria

1. ✅ New changes automatically depend on latest consolidating tag
2. ✅ Apply correctly expands tag dependencies
3. ✅ CLI flags provide control (auto/manual/explicit)
4. ✅ Backward compatible with existing repos
5. ✅ Performance: < 1ms tag lookup, 10x faster dependency resolution
6. ✅ All tests pass (unit + integration)
7. ✅ Documentation complete

---

## Risks & Mitigations

### Risk 1: Breaking Changes

**Risk**: Modify change format incorrectly

**Mitigation**: Extensive backward compatibility tests

### Risk 2: Performance Regression

**Risk**: Tag lookup slows down record operation

**Mitigation**: Caching + benchmarks + optimization

### Risk 3: Complexity

**Risk**: Dependency resolution becomes too complex

**Mitigation**: Clear separation of concerns, comprehensive tests

---

## Next Steps

After Increment 6 is complete:

- **Increment 7**: Flexible Consolidation Workflows (hotfix scenarios)
- **Increment 8**: Query APIs (programmatic access)
- **Increment 9**: Remote Operations (push/pull with tags)
- **Increment 10**: Performance Optimization (indexing, caching)

---

*Document Version: 1.0*  
*Author: AI Assistant*  
*Date: 2025-01-15*