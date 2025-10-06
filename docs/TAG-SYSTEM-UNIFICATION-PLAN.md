# Tag System Unification Refactoring Plan

## Executive Summary

This document outlines the plan to unify Atomic's tag system by eliminating the confusion between "tags" and "consolidating tags". The refactoring will establish a single, coherent tag concept throughout the codebase.

## Current State Analysis

### Database Architecture (Current - BEFORE Refactoring)

**Two Separate Storage Locations:**

1. **Channel's `tags` table** (`Db<L64, Pair<SerializedMerkle, SerializedMerkle>>`)
   - Located within each channel structure
   - Key: L64 (timestamp/sequence number)
   - Value: Pair of Merkles (individual tag merkle, cumulative state merkle)
   - Purpose: Track which tags exist in this channel and when
   - Used by: `atomic log`, iteration, tag listing
   - **Problem**: Only stores merkles, no metadata!

2. **Global `consolidating_tags` table** (`UDb<SerializedHash, ConsolidatingTagBytes>`)
   - Located at transaction root level
   - Key: SerializedHash (tag's hash)
   - Value: Full serialized ConsolidatingTag metadata
   - Purpose: Store rich tag metadata (version, consolidated changes, etc.)
   - Used by: `atomic change` (display), tag creation, dependency resolution
   - **Problem**: Separate from channel tags, can get out of sync!

### Current Problems

1. **Semantic Confusion**: Two names for the same concept
   - "tag" vs "consolidating tag" - they're the same thing!
   - All tags ARE consolidating tags (they consolidate dependency trees)
   - Creates unnecessary complexity in code and docs

2. **Data Synchronization Issues**:
   - Tags get added to channel's `tags` table (just merkles)
   - Tag metadata stored separately in global `consolidating_tags` table
   - **Server bug**: `atomic log` queries channel tags table, extracts hash, looks up in `consolidating_tags` → NOT FOUND!
   - Two separate operations that can fail independently = inconsistent state

3. **Lookup Complexity**:
   - Channel tags table → get merkle → convert to hash → lookup in consolidating_tags table
   - Multiple steps, multiple failure points
   - Fallback logic: try `consolidating_tags` → try loading as change file → fail
   - Error-prone and difficult to reason about

4. **Code Duplication**:
   - Similar tag handling logic scattered across multiple files
   - Each command implements its own tag retrieval strategy
   - No single source of truth

5. **Storage Inefficiency**:
   - Tag information stored twice (merkle in channel table, metadata in global table)
   - Requires two separate database lookups to get complete tag information

### Why Two Tables Was A Mistake

The two-table design seems logical at first (normalization, separate concerns), but in practice:

1. **Tags are inherently channel-specific** - they represent a state of a specific channel
2. **Synchronization is error-prone** - two separate operations = two failure points
3. **Lookups are slow** - need two database queries to get full tag information
4. **Code is complex** - every operation must coordinate between two tables
5. **No real benefit** - tag metadata isn't that large, and tags aren't shared across channels

The "global" table made sense in theory (avoid duplication if same tag in multiple channels), but:
- Tags are channel-specific by nature
- The synchronization complexity outweighs any storage savings
- Bugs arise from the split architecture

## Proposed Solution: True Unification

### Core Principle

**All tags are consolidating tags.** There is only ONE type of tag in Atomic VCS, stored in ONE place.

### Architecture Decision

**REMOVE the global `consolidating_tags` table entirely.**

Store full tag metadata directly in the channel's `tags` table:

```
BEFORE: Db<L64, Pair<SerializedMerkle, SerializedMerkle>>
AFTER:  UDb<L64, SerializedConsolidatingTag>
```

**Single source of truth**: Channel's tags table contains everything.

```
Channel Tags Table (AFTER Refactoring)
───────────────────────────────────────
timestamp → full ConsolidatingTag metadata
  ├─ tag_hash
  ├─ channel
  ├─ version
  ├─ consolidated_change_count
  ├─ consolidated_changes
  ├─ previous_consolidation
  ├─ consolidation_timestamp
  └─ ai_attribution
```

### Implementation Strategy

#### Phase 1: Change Channel Tags Table Structure

**Goal**: Expand channel tags table to store full metadata instead of just merkles

**Changes**:

1. **Update Channel struct** (`libatomic/src/pristine/sanakirja.rs`):
   ```rust
   // BEFORE
   pub struct Channel {
       pub tags: Db<L64, Pair<SerializedMerkle, SerializedMerkle>>,
       // ...
   }
   
   // AFTER
   pub struct Channel {
       pub tags: UDb<L64, SerializedConsolidatingTag>,
       // ...
   }
   ```

2. **Update ChannelTxnT trait** (`libatomic/src/pristine/mod.rs`):
   ```rust
   pub trait ChannelTxnT: TxnT {
       // Change signature to return new type
       fn tags<'a>(&self, channel: &'a Self::Channel) -> &'a UDb<L64, SerializedConsolidatingTag>;
       
       // Update all tag-related methods
       fn iter_tags(/*...*/) -> Result<TagIterator<'_, Self>, Self::GraphError>;
       fn is_tagged(/*...*/) -> Result<bool, Self::GraphError>;
   }
   ```

**Rationale**: 
- Store everything in one place - no more synchronization issues
- Single database query gets complete tag information
- Natural location for channel-specific data

#### Phase 2: Remove Global consolidating_tags Table

**Goal**: Delete the global table and all code that uses it

**Changes**:

1. **Remove from GenericTxn** (`libatomic/src/pristine/sanakirja.rs`):
   ```rust
   // DELETE THIS FIELD
   // pub(crate) consolidating_tags: UDb<SerializedHash, ConsolidatingTagBytes>,
   ```

2. **Remove from Root enum**:
   ```rust
   // DELETE THIS VARIANT
   // ConsolidatingTags,
   ```

3. **Remove ConsolidatingTagTxnT trait entirely**:
   - Delete `ConsolidatingTagTxnT` trait
   - Delete `ConsolidatingTagMutTxnT` trait
   - Remove all implementations

4. **Remove all references**:
   ```bash
   # Delete these functions everywhere:
   - get_consolidating_tag()
   - put_consolidating_tag()
   - iter_consolidating_tags()
   ```

**Rationale**: 
- Eliminate the source of synchronization bugs
- Simplify the codebase significantly
- One place to look for tag data

#### Phase 3: Update Apply Logic

**Goal**: Store full tag metadata in channel tags table during apply

**Changes**:

1. **Update apply_change_ws** (`libatomic/src/apply.rs`):
   ```rust
   // BEFORE: Store in two places
   if let Some(ref tag_metadata) = change.hashed.consolidating_tag {
       txn.put_consolidating_tag(&tag_hash, &serialized)?;
       txn.put_tags(tags, n.into(), &merkle)?;
   }
   
   // AFTER: Store in one place with full metadata
   if let Some(ref tag_metadata) = change.hashed.consolidating_tag {
       let consolidating_tag = ConsolidatingTag {
           tag_hash,
           channel: channel_name.to_string(),
           consolidated_change_count: tag_metadata.consolidated_change_count,
           dependency_count_before: tag_metadata.dependency_count_before,
           consolidated_changes: tag_metadata.consolidated_changes.clone(),
           previous_consolidation: tag_metadata.previous_consolidation,
           consolidates_since: tag_metadata.consolidates_since,
           consolidation_timestamp: tag_metadata.consolidation_timestamp,
           version: tag_metadata.version.clone(),
           ai_attribution: tag_metadata.ai_attribution.clone(),
           change_file_hash: Some(*hash),
       };
       
       // Single operation, single source of truth
       txn.put_tag(tags, n, &consolidating_tag)?;
   }
   ```

**Rationale**: 
- Single atomic operation
- No synchronization issues
- Simpler code

#### Phase 4: Update Log Command

**Goal**: Simplify log iteration - no more complex lookups

**Changes**:

1. **Update LogIterator** (`atomic/src/commands/log.rs`):
   ```rust
   // BEFORE: Complex multi-step lookup
   for tag_entry in self.txn.iter_tags(self.txn.tags(&*channel_read), 0)? {
       let (_, tag_pair) = tag_entry?;
       let merkle_hash: libatomic::Merkle = tag_pair.a.into();
       let tag_hash = libatomic::pristine::Hash::from_merkle(&merkle_hash);
       
       // Query separate table
       if let Some(serialized) = self.txn.get_consolidating_tag(&tag_hash)? {
           let tag = serialized.to_tag()?;
           // ... display tag
       }
   }
   
   // AFTER: Direct iteration with full metadata
   for tag_entry in self.txn.iter_tags(self.txn.tags(&*channel_read), 0)? {
       let (timestamp, tag) = tag_entry?;
       // tag is already a ConsolidatingTag with all metadata!
       // ... display tag directly
   }
   ```

**Rationale**: 
- No more "tag not found" errors
- Single query gets everything
- Much simpler code

#### Phase 5: Simplify Tag Display

**Goal**: Consistent tag display across all commands

**Changes**:

1. **Unified tag formatting**:
   ```rust
   pub fn format_tag_for_display(tag: &ConsolidatingTag) -> String {
       let version = tag.version
           .as_ref()
           .map(|v| format!(" v{}", v))
           .unwrap_or_default();
       
       format!(
           "🏷️  Tag{} (consolidates {} changes)",
           version,
           tag.consolidated_change_count
       )
   }
   ```

2. **Update commands**:
   - `atomic log`: Use `format_tag_for_display()`
   - `atomic change`: Use `format_tag_for_display()`
   - `atomic tag --list`: Use `format_tag_for_display()`

## Implementation Checklist

### Phase 1: Change Table Structure (Breaking)
- [ ] Update `Channel` struct: `tags` field type to `UDb<L64, SerializedConsolidatingTag>`
- [ ] Update `SerializedChannel` to store new table page
- [ ] Update `ChannelTxnT` trait: `tags()` return type
- [ ] Update `ChannelMutTxnT` trait: `put_tag()`, `del_tag()` signatures
- [ ] Implement new trait methods in `sanakirja.rs`
- [ ] Update all channel creation/loading code

### Phase 2: Remove Global Table (Breaking)
- [ ] Remove `consolidating_tags` field from `GenericTxn`
- [ ] Remove `Root::ConsolidatingTags` enum variant
- [ ] Delete `ConsolidatingTagTxnT` trait
- [ ] Delete `ConsolidatingTagMutTxnT` trait
- [ ] Delete all implementations of these traits
- [ ] Remove all `get_consolidating_tag()` calls
- [ ] Remove all `put_consolidating_tag()` calls

### Phase 3: Update Apply Logic (Critical)
- [ ] Update `apply_change_ws()` to store full tag in channel table
- [ ] Update `apply_change_rec_ws()` similarly
- [ ] Remove dual-storage logic
- [ ] Test apply operations with tags
- [ ] Verify single atomic operation

### Phase 4: Simplify Log Command (Bug Fix)
- [ ] Update `LogIterator::for_each()` to use direct iteration
- [ ] Remove `get_consolidating_tag()` lookup
- [ ] Test `atomic log` displays tags correctly
- [ ] Test on server after push
- [ ] Verify performance improvement

### Phase 5: Update Protocol Handlers (Critical)
- [ ] Update `protocol.rs` tagup handler
- [ ] Update `server.rs` tagup handler
- [ ] Update `pull` command in `pushpull.rs`
- [ ] Update `pull` in `atomic-remote/src/lib.rs`
- [ ] Test push/pull with tags

### Phase 6: Migration (Compatibility)
- [ ] Write migration function
- [ ] Add schema version tracking
- [ ] Test migration with old repositories
- [ ] Add migration tests
- [ ] Document migration process
#### Phase 5: Update Push/Pull Protocol

**Goal**: Ensure server receives and stores full tag metadata

**Changes**:

1. **Update tagup handler** (`atomic/src/commands/protocol.rs`):
   ```rust
   // BEFORE: Store in consolidating_tags table
   txn.put_consolidating_tag(&tag_hash, &serialized)?;
   txn.put_tags(&mut channel.write().tags, last_t.into(), &state)?;
   
   // AFTER: Store in channel tags table with full metadata
   let mut consolidating_tag = ConsolidatingTag::new(tag_hash, channel_name.to_string());
   consolidating_tag.consolidation_timestamp = original_timestamp;
   // ... populate from tag file header ...
   txn.put_tag(&mut channel.write().tags, last_t, &consolidating_tag)?;
   ```

2. **Update HTTP API** (`atomic-api/src/server.rs`):
   - Same changes as protocol.rs
   - Ensure consistency across all remote protocols

**Rationale**: 
- Server's tag table has complete information
- No separate metadata table to keep in sync

#### Phase 6: Display (Polish)
- [ ] Create `format_tag_for_display()` function
- [ ] Update all commands to use consistent formatting
- [ ] Ensure "🏷️ Tag" indicator appears everywhere
- [ ] Test display in various scenarios

## Testing Strategy

### Unit Tests
- [ ] Test `iter_tags()` returns full ConsolidatingTag structs
- [ ] Test `put_tag()` stores serialized tag correctly
- [ ] Test tag lookup by timestamp
- [ ] Test channel tags table stores all metadata

### Integration Tests
- [ ] Test tag creation → full metadata in channel table
- [ ] Test push tag → pull tag → log shows tag correctly
- [ ] Test clone repository with tags → log works immediately
- [ ] Test tag dependency resolution uses channel table
- [ ] Test `atomic log` on server after receiving pushed tag

### Regression Tests
- [ ] Existing repositories with old format work after migration
- [ ] Migration function correctly converts old tables
- [ ] All existing tag tests pass with new architecture

## Migration Strategy

### Backwards Compatibility

**Key Principle**: Old repositories must continue to work.

**Strategy**: Automatic migration on first repository open

**Migration Function**:
```rust
/// Migrate old two-table tag system to unified single-table system
pub fn migrate_tags_v1_to_v2(txn: &mut impl MutTxnT) -> Result<(), MigrateError> {
    // For each channel
    for channel_name in txn.iter_channels("")? {
        let channel = txn.load_channel(&channel_name)?.unwrap();
        let channel_read = channel.read();
        
        // Check if already migrated (tags table has ConsolidatingTag values)
        if is_already_migrated(&channel_read.tags) {
            continue;
        }
        
        // Create new tags table
        let mut new_tags = btree::create_db_(&mut txn.txn)?;
        
        // Iterate old tags (merkle pairs)
        for tag_entry in iter_old_tags(&channel_read.tags)? {
            let (timestamp, (individual_merkle, cumulative_merkle)) = tag_entry?;
            let tag_hash = Hash::from_merkle(&cumulative_merkle);
            
            // Try to get metadata from old consolidating_tags table
            let consolidating_tag = if let Some(serialized) = txn.get_consolidating_tag(&tag_hash)? {
                serialized.to_tag()?
            } else {
                // Fallback: Load from tag file or create minimal metadata
                load_or_create_tag_metadata(tag_hash, channel_name)?
            };
            
            // Store in new unified tags table
            put_tag(&mut new_tags, timestamp, &consolidating_tag)?;
        }
        
        // Replace old tags table with new one
        drop(channel_read);
        let mut channel_write = channel.write();
        channel_write.tags = new_tags;
    }
    
    // Delete old consolidating_tags table
    txn.delete_consolidating_tags_table()?;
    
    // Update schema version
    txn.set_schema_version(2)?;
    
    Ok(())
}
```

### Database Schema Version

**Schema v1** (old): Channel tags + global consolidating_tags  
**Schema v2** (new): Channel tags only (with full metadata)

Migration runs automatically on repository open if schema version is v1.

## Benefits

### 1. Conceptual Clarity
- One concept: "tags" (which happen to consolidate dependencies)
- No confusion about "consolidating tags" vs "regular tags"
- Single table = single source of truth
- Easier to explain to users and contributors

### 2. Reliability
- Impossible to have inconsistent state (only one table!)
- No more "tag exists in one place but not another"
- `atomic log` works reliably always
- Single atomic operation = all or nothing

### 3. Maintainability
- 50% less tag-related code (remove entire global table + trait)
- Simple iteration: one query gets everything
- Natural location: tags live in channels where they belong
- Easier to add features (e.g., tag signing, tag attribution)

### 4. Performance
- Single database query instead of two
- No cross-table lookups
- Less storage overhead (no duplicate hash keys)
- Faster iteration for log commands

## Timeline

### Week 1: Preparation
- Review and approve this plan
- Create feature branch
- Set up comprehensive test suite

### Week 2: Implementation
- Phase 1 (Naming): 2 days
- Phase 2 (Consistency): 2 days
- Phase 3 (Unified Lookup): 1 day

### Week 3: Bug Fixes & Testing
- Phase 4 (Server Bug Fix): 2 days
- Phase 5 (Display): 1 day
- Integration testing: 2 days

### Week 4: Polish & Merge
- Documentation updates
- Code review
- Merge to main

## Success Criteria

1. ✅ All tests pass (including new integration tests)
2. ✅ `atomic log` works on server after receiving pushed tags
3. ✅ No confusion about "tag types" in codebase or docs
4. ✅ Old repositories continue to work
5. ✅ Code is simpler than before (measured by lines of tag-handling code)

## Initial Implementation Findings

### Complexity Discovered

After starting implementation, we discovered several complications:

1. **TagTxn Implementation**: Separate implementation for reading compressed tag files on disk
   - Currently reads old merkle-pair format from tag files
   - Would need to handle both old and new formats
   - Adds migration complexity for existing tag files

2. **Unsized Type Challenges**: `ConsolidatingTagBytes` is a DST (dynamically sized type)
   - Requires `UDb` (unsized database) not `Db`
   - Requires `Page_unsized` not `Page`
   - More complex cursor implementations
   - Trait bound complications with `Sized`

3. **Remote Tables**: Similar changes needed for remote tag storage
   - Remotes also have a `tags` table with same structure
   - Need to update remote implementations consistently

4. **File Format Migration**: Tag files on disk need migration
   - Existing tag files use merkle-pair format
   - Need reader that handles both formats
   - Migration strategy for repositories with existing tags

### Revised Recommendation

Given the complexity discovered, we recommend a **simpler incremental approach**:

#### Alternative Approach: Fix Synchronization Without Restructuring

Instead of restructuring the database schema, we can:

1. **Ensure Consistency**: Make sure `consolidating_tags` table is ALWAYS populated
   - Fix all places where tags are created to store in both tables atomically
   - Add validation checks to catch missing metadata

2. **Unified Lookup Function**: Create single entry point for tag retrieval
   ```rust
   pub fn get_tag(txn: &impl TxnT, tag_hash: &Hash) -> Result<Option<ConsolidatingTag>> {
       // Always check consolidating_tags table first
       if let Some(serialized) = txn.get_consolidating_tag(tag_hash)? {
           return Ok(Some(serialized.to_tag()?));
       }
       // Fallback for backwards compatibility
       Ok(None)
   }
   ```

3. **Fix Server Bug Immediately**: Focus on the immediate issue
   - Ensure tagup protocol stores metadata correctly (already done)
   - Ensure pull operations store metadata correctly (already done)
   - Add integration test for push → log workflow

4. **Rename for Clarity**: Use consistent naming without schema changes
   - Document that "all tags are consolidating tags"
   - Use consistent terminology in docs and comments
   - No database restructuring required

### Benefits of Incremental Approach

1. **Lower Risk**: No breaking changes to database schema
2. **Faster Delivery**: Fix server bug immediately
3. **Backwards Compatible**: Old repositories work without migration
4. **Incremental Testing**: Can test each fix independently
5. **Less Code Churn**: Minimal changes to working code

### Full Refactoring: Future Work

The complete table unification can be pursued later as a separate major version change:
- Design comprehensive migration strategy
- Handle TagTxn file format changes
- Update all remote implementations
- Test extensively with old and new formats

## Conclusion

The original plan to unify tables is architecturally sound, but implementation complexity suggests an incremental approach is more practical:

1. **Phase 1** (Immediate): Fix synchronization bugs - ensure metadata always stored
2. **Phase 2** (Near-term): Unified lookup functions and consistent naming
3. **Phase 3** (Long-term): Full table unification with migration (separate major version)

This approach fixes the server bug immediately while preserving the option for complete refactoring in the future.

**Recommended Next Steps**:
1. Verify push/pull/clone all populate `consolidating_tags` table
2. Add integration tests for tag workflows
3. Test `atomic log` on server after push
4. Document tag architecture clearly
5. Consider full refactoring for v2.0