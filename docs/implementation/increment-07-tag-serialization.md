# Increment 7: Tag Change File Serialization

**Status**: 📋 Planned  
**Dependencies**: Increment 6 (Dependency Resolution)  
**Estimated Duration**: 4-5 days  
**Priority**: High - Required for push/pull functionality  

---

## Executive Summary

**Objective**: Serialize consolidating tags to `.change` files so they can be pushed/pulled to remote repositories and viewed with `atomic change <tag-hash>`.

**Current State**: Consolidating tags exist only in the pristine database (`ConsolidatingTags` table), making them:
- ❌ Invisible to `atomic log`
- ❌ Unreadable with `atomic change <tag-hash>`
- ❌ Unable to sync to remotes
- ❌ Lost during repository clones

**Desired State**: Consolidating tags are first-class changes that:
- ✅ Appear in `.atomic/changes/<hash>.change` files
- ✅ Show in `atomic log` with special formatting
- ✅ Can be viewed with `atomic change <tag-hash>`
- ✅ Sync automatically during push/pull
- ✅ Clone correctly with repositories

---

## Problem Statement

### Issues Discovered in Testing

From Increment 6 testing, we discovered:

1. **No change files**: `atomic change W33KUFBC...` fails with "No such file or directory"
2. **Push fails**: `atomic push` errors with "Cannot add tag...channel does not have that state"
3. **Not in log**: Tags don't appear in `atomic log` output
4. **Database-only**: Tags exist only in pristine DB, not in changestore

### Root Cause

Consolidating tags are created with:
```rust
// Only stored in database
txn.write().put_consolidating_tag(&tag_hash, &serialized)?;
```

But regular changes are stored in **two places**:
1. **Pristine database** - For fast lookups and graph operations
2. **Change files** - For serialization, sync, and human viewing

Consolidating tags are missing the second part!

---

## Design Overview

### Architecture

```
Tag Creation Flow (Current):
  atomic tag create --consolidate
    ↓
  Create ConsolidatingTag struct
    ↓
  Serialize with bincode
    ↓
  Store in pristine DB ✅
    ↓
  (MISSING: Write .change file) ❌

Tag Creation Flow (After Increment 7):
  atomic tag create --consolidate
    ↓
  Create ConsolidatingTag struct
    ↓
  Generate Change structure for tag
    ↓
  Write to .atomic/changes/<hash>.change ✅
    ↓
  Store in pristine DB ✅
    ↓
  Update channel state ✅
```

### Change File Format

Consolidating tags will use a special change format:

```
# Example: .atomic/changes/W3/3KUFBC4X6OMRZD5F6WAA64P534ZFY4PKS64XEAUYE4NHBKIQOAC.change

message = 'Release v1.0 - Consolidated Milestone'
timestamp = '2025-09-30T19:34:18.337518Z'

[[authors]]
key = '3rAPLeTJdKPwzE3eFHGEeeH7iW9fPTXGotmUPeWAekph'

# Consolidating Tag Metadata
[consolidating_tag]
version = '1.0.0'
channel = 'main'
consolidated_change_count = 25
dependency_count_before = 25
effective_dependencies = 1
dependency_reduction = 24

# Consolidated Changes (explicit list)
[[consolidated_changes]]
hash = 'CHANGE1HASH...'

[[consolidated_changes]]
hash = 'CHANGE2HASH...'

# ... (all 25 changes)

# No Hunks - Tags don't modify files
```

---

## Implementation Plan

### Phase 1: Change Structure Extension (Day 1)

**Goal**: Extend `libatomic::change::Change` to support tag metadata.

**Tasks**:
1. Add optional `consolidating_tag` field to `Change` struct
2. Define `ConsolidatingTagMetadata` structure for serialization
3. Update change serialization to handle tag metadata
4. Add unit tests for tag serialization/deserialization

**Files Modified**:
- `libatomic/src/change.rs` - Add tag metadata field
- `libatomic/src/change/serialize.rs` - Handle tag serialization

**Deliverables**:
```rust
pub struct Change {
    // Existing fields
    pub offsets: Offsets,
    pub hashed: Hashed,
    pub unhashed: Option<Unhashed>,
    pub contents: Vec<u8>,
    
    // NEW: Consolidating tag metadata
    pub consolidating_tag: Option<ConsolidatingTagMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidatingTagMetadata {
    pub version: Option<String>,
    pub channel: String,
    pub consolidated_change_count: u64,
    pub dependency_count_before: u64,
    pub consolidated_changes: Vec<Hash>,
    pub previous_consolidation: Option<Hash>,
    pub consolidates_since: Option<Hash>,
}
```

### Phase 2: Tag Change File Writing (Day 2)

**Goal**: Write consolidating tags to `.change` files.

**Tasks**:
1. Create `write_consolidating_tag_to_file()` function
2. Integrate with `atomic tag create --consolidate`
3. Generate proper change hash for tag
4. Write to `.atomic/changes/<hash>.change`
5. Test file creation and validation

**Files Modified**:
- `atomic/src/commands/tag.rs` - Add file writing
- `libatomic/src/changestore/filesystem.rs` - Helper functions

**Implementation**:
```rust
fn write_consolidating_tag_to_file(
    changes_dir: &Path,
    tag: &ConsolidatingTag,
    message: &str,
    author: &Author,
) -> Result<Hash, anyhow::Error> {
    // 1. Create Change structure with tag metadata
    let change = Change {
        offsets: Offsets::default(),
        hashed: Hashed {
            message: message.to_string(),
            timestamp: Utc::now(),
            authors: vec![author.clone()],
            ..Default::default()
        },
        unhashed: None,
        contents: Vec::new(),
        consolidating_tag: Some(ConsolidatingTagMetadata::from(tag)),
    };
    
    // 2. Serialize to TOML format
    let serialized = serialize_change(&change)?;
    
    // 3. Calculate hash
    let hash = Hash::blake3(&serialized);
    
    // 4. Write to file
    let file_path = changes_dir
        .join(&hash.to_base32()[0..2])
        .join(format!("{}.change", hash.to_base32()));
    
    fs::create_dir_all(file_path.parent().unwrap())?;
    fs::write(&file_path, serialized)?;
    
    Ok(hash)
}
```

### Phase 3: Tag Change File Reading (Day 3)

**Goal**: Support reading and displaying consolidating tags.

**Tasks**:
1. Update `atomic change <tag-hash>` to recognize tags
2. Format tag output differently from regular changes
3. Add tag indicator to `atomic log` output
4. Test viewing tags with CLI

**Files Modified**:
- `atomic/src/commands/change.rs` - Recognize and format tags
- `atomic/src/commands/log.rs` - Show tag indicators

**Output Format**:
```bash
$ atomic change W33KUFBC4X6OMRZD5F6WAA64P534ZFY4PKS64XEAUYE4NHBKIQOAC

Consolidating Tag: Release v1.0
Version: 1.0.0
Channel: main
Author: Lee Faus <lee@example.com>
Date: Tue, 30 Sep 2025 19:34:18 +0000

Consolidated Changes: 25
Dependencies Before: 25
Effective Dependencies: 1
Dependency Reduction: 24 (96%)

Consolidated Changes:
  [1] CHANGE1HASH... - "Initial commit"
  [2] CHANGE2HASH... - "Add feature X"
  ...
  [25] CHANGE25HASH... - "Complete sprint"
```

### Phase 4: Log Integration (Day 4)

**Goal**: Show consolidating tags in `atomic log` with special formatting.

**Tasks**:
1. Detect tags during log iteration
2. Add visual indicator for tags (🏷️ or [TAG])
3. Show condensed tag information
4. Ensure tags appear in chronological order

**Files Modified**:
- `atomic/src/commands/log.rs` - Add tag detection and formatting

**Output Format**:
```bash
$ atomic log

Change MMGH5EARKYO7FVO4LC63LPHOANZI67BDQI6PCX6QZS34I3FNYLUAC
Author: Lee Faus <lee@fluxuate.ai>
Date: Tue, 30 Sep 2025 19:50:16 +0000

    post tag 2

Change RN4SQW3AJBZFXWFQ6EEYE5G3FMFCKO7A5GY7GTBBXYZZSMPXYDBAC
Author: Lee Faus <lee@fluxuate.ai>
Date: Tue, 30 Sep 2025 19:49:32 +0000

    post tag 1

🏷️  Tag N3MGRVJE7LXGCUBLELTUYIBX2UNTZWWSIBTQ3QQDPYEEFI23PXJAC
Version: 1.1.0
Date: Tue, 30 Sep 2025 19:50:45 +0000

    Release v1.1
    Consolidates: 6 changes | Deps: 6→1 (savings: 5)

Change AFIBMLBCAFCN25OQILZ74J3ED4S4EGICJ5VAXNJL4XUGT2DJVFTQC
Author: Lee Faus <lee@fluxuate.ai>
Date: Tue, 30 Sep 2025 19:41:02 +0000

    change 2
```

### Phase 5: Push/Pull Support (Day 5)

**Goal**: Ensure tags sync correctly to remote repositories.

**Tasks**:
1. Test push with consolidating tags
2. Test pull with consolidating tags
3. Test clone with consolidating tags
4. Verify both change file and pristine DB sync
5. Handle edge cases (missing tag metadata, corrupt files)

**Files Modified**:
- `atomic-remote/src/lib.rs` - Potentially add tag-specific handling
- Integration tests for push/pull

**Test Scenarios**:
```bash
# Scenario 1: Push tag to remote
atomic tag create --consolidate --version 1.0.0 -m "Release"
atomic push origin
# Remote should have both .change file and tag metadata

# Scenario 2: Pull tag from remote
atomic pull origin
atomic tag list --consolidating
# Should show all tags from remote

# Scenario 3: Clone with tags
atomic clone remote.atomic local-clone
cd local-clone
atomic tag list --consolidating
# Should show all tags from remote
```

---

## Data Structures

### ConsolidatingTagMetadata (for serialization)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsolidatingTagMetadata {
    /// Semantic version (e.g., "1.0.0", "2.1.0-beta.1")
    pub version: Option<String>,
    
    /// Channel this tag belongs to
    pub channel: String,
    
    /// Number of changes consolidated by this tag
    pub consolidated_change_count: u64,
    
    /// Number of dependencies before consolidation
    pub dependency_count_before: u64,
    
    /// Explicit list of changes consolidated
    pub consolidated_changes: Vec<Hash>,
    
    /// Previous consolidating tag (if any)
    pub previous_consolidation: Option<Hash>,
    
    /// Tag to consolidate from (flexible strategy)
    pub consolidates_since: Option<Hash>,
    
    /// User who created this tag
    pub created_by: Option<String>,
    
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl From<&ConsolidatingTag> for ConsolidatingTagMetadata {
    fn from(tag: &ConsolidatingTag) -> Self {
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
}
```

---

## Testing Strategy

### Unit Tests

1. **test_tag_change_serialization**
   - Create ConsolidatingTag
   - Convert to Change
   - Serialize to TOML
   - Deserialize back
   - Verify roundtrip correctness

2. **test_tag_change_hash_calculation**
   - Ensure tag hashes are stable
   - Verify deterministic hash generation

3. **test_tag_metadata_serialization**
   - Test ConsolidatingTagMetadata serialization
   - Verify all fields preserved

### Integration Tests

1. **test_tag_file_creation**
   - Create consolidating tag via CLI
   - Verify `.change` file exists
   - Verify file is readable
   - Verify content is correct

2. **test_tag_viewing**
   - Create consolidating tag
   - Run `atomic change <tag-hash>`
   - Verify output format
   - Verify all metadata displayed

3. **test_tag_in_log**
   - Create several changes and a tag
   - Run `atomic log`
   - Verify tag appears with indicator
   - Verify chronological order

4. **test_tag_push_pull**
   - Create tag in repo A
   - Push to remote
   - Clone to repo B
   - Verify tag exists in repo B
   - Verify metadata correct

### End-to-End Tests

1. **Full workflow test**:
```bash
# Create repository
atomic init test-repo
cd test-repo

# Create changes
echo "file1" > file1.txt
atomic add file1.txt
atomic record -m "Change 1"

echo "file2" > file2.txt
atomic add file2.txt
atomic record -m "Change 2"

# Create consolidating tag
atomic tag create --consolidate --version 0.0.1 -m "First tag"

# Verify tag file exists
ls .atomic/changes/*/*.change | wc -l
# Should be 3 (2 changes + 1 tag)

# View tag
atomic change <TAG_HASH>
# Should show tag metadata

# View log
atomic log
# Should show tag with indicator

# Push to remote
atomic push origin

# Clone and verify
cd ..
atomic clone remote.atomic clone-test
cd clone-test
atomic tag list --consolidating
# Should show tag
```

---

## Edge Cases & Error Handling

### 1. Corrupt Tag File

**Scenario**: Tag `.change` file is corrupted or incomplete.

**Handling**:
- Detect during deserialization
- Fall back to pristine DB lookup
- Log warning
- Continue operation

### 2. Missing Consolidated Changes

**Scenario**: Tag references changes that don't exist.

**Handling**:
- Validate during tag creation
- Warn if changes are missing
- Allow viewing tag anyway (show missing changes)

### 3. Tag/Change Hash Collision

**Scenario**: Tag hash collides with existing change hash.

**Handling**:
- Extremely unlikely (cryptographic hash)
- Detect during file write
- Error and abort tag creation

### 4. Large Tag Files

**Scenario**: Tag consolidates 10,000+ changes, creating huge file.

**Handling**:
- Consider compression
- Or: Store change list separately (reference file)
- Or: Limit to first N changes in file, rest in DB

### 5. Tag Without Version

**Scenario**: Tag created without semantic version.

**Handling**:
- Version is optional
- Display as "Unversioned tag"
- Still functional

---

## Performance Considerations

### File I/O

**Operations**:
- Tag creation: 1 file write
- Tag viewing: 1 file read
- Log with tags: N file reads

**Optimization**:
- Cache parsed tags in memory
- Batch tag loading during log
- Use memory-mapped files for large tags

### Hash Calculation

**Approach**: Use BLAKE3 for consistency with existing changes.

**Performance**: ~1 GB/s on modern hardware, negligible for tag metadata.

### Serialization

**Format**: TOML for human readability and consistency.

**Performance**: Fast enough for tag sizes (< 1KB typically).

---

## AGENTS.md Compliance

### ✅ Configuration-Driven Design

- Tag format configurable via `Change` structure
- Serialization format extensible (TOML)
- Metadata fields optional and extensible

### ✅ DRY Principles

- Reuse existing `Change` serialization infrastructure
- Convert between `ConsolidatingTag` and `ConsolidatingTagMetadata`
- No duplication of change file handling

### ✅ Type Safety

- Strong typing throughout (`ConsolidatingTagMetadata`)
- Hash types for correctness
- Result types for fallible operations

### ✅ Error Handling Strategy

- Proper Result types
- Context-rich error messages
- Graceful fallback to pristine DB

### ✅ Factory Patterns

- `ConsolidatingTagMetadata::from(tag)` conversion
- `write_consolidating_tag_to_file()` factory function

---

## Migration Path

### Backward Compatibility

**Existing tags (database-only)**: Will continue to work but:
- Won't appear in `atomic log`
- Can't be viewed with `atomic change`
- Won't sync to remotes

**Migration**: Run a migration command to generate `.change` files:
```bash
atomic tag migrate
# Scans pristine DB for tags without change files
# Generates missing .change files
```

### Forward Compatibility

**New tags**: Will have both:
- Pristine DB entry (fast lookups)
- Change file (viewing, syncing)

**Future**: Consider deprecating database-only tags.

---

## Dependencies

### Internal Dependencies
- Increment 6: Dependency Resolution ✅
- `libatomic::change` module (exists)
- `libatomic::changestore` module (exists)
- `atomic::commands::tag` (exists)

### External Dependencies
- `toml` crate (already used)
- `blake3` crate (already used)
- `serde` crate (already used)

---

## Success Criteria

### Functional Requirements
- ✅ Tags have corresponding `.change` files
- ✅ `atomic change <tag-hash>` displays tag metadata
- ✅ `atomic log` shows tags with indicators
- ✅ Tags sync during push/pull
- ✅ Tags clone correctly with repository

### Quality Requirements
- ✅ All unit tests pass
- ✅ All integration tests pass
- ✅ No breaking changes to existing functionality
- ✅ Performance acceptable (< 100ms for tag creation)

### User Experience Requirements
- ✅ Clear visual distinction between tags and changes
- ✅ Informative tag display (version, change count, reduction)
- ✅ Consistent with existing atomic UX

---

## Risks & Mitigations

### Risk 1: Breaking Existing Change Format

**Risk**: Adding tag metadata might break existing change parsing.

**Mitigation**:
- Make `consolidating_tag` field optional
- Ensure backward compatibility in parser
- Test with existing repositories

### Risk 2: File Format Bloat

**Risk**: Large consolidated_changes lists make files huge.

**Mitigation**:
- Start with full list in file
- Monitor file sizes in testing
- Add compression if needed (future optimization)

### Risk 3: Sync Complexity

**Risk**: Tags might not sync correctly (both file + DB needed).

**Mitigation**:
- Test push/pull extensively
- Ensure change files always sync
- Pristine DB will be rebuilt on pull

---

## Documentation Updates

### User Documentation

**New sections**:
1. "Viewing Consolidating Tags" - How to use `atomic change <tag-hash>`
2. "Tags in Log" - Interpreting tag indicators in `atomic log`
3. "Syncing Tags" - How tags behave during push/pull/clone

**Updated sections**:
1. "Creating Tags" - Note about change file creation
2. "Tag Management" - Complete workflow examples

### Developer Documentation

**New sections**:
1. "Tag Change File Format" - Detailed format specification
2. "Tag Serialization" - Implementation details
3. "Tag Migration" - Backward compatibility notes

---

## Future Enhancements (Post-Increment 7)

### 1. Compressed Tag Storage

For tags consolidating 10,000+ changes:
- Store change list in separate compressed file
- Reference from main `.change` file

### 2. Tag Signatures

Add cryptographic signatures to tags:
- Verify tag authenticity
- Prevent tampering

### 3. Tag Annotations

Allow additional annotations on tags:
- Release notes
- Breaking changes list
- Migration guides

### 4. Tag Search

Search tags by:
- Version range (e.g., "1.x.x")
- Date range
- Author
- Message content

---

## Completion Checklist

### Phase 1: Change Structure
- [ ] Add `consolidating_tag` field to `Change`
- [ ] Create `ConsolidatingTagMetadata` struct
- [ ] Update serialization/deserialization
- [ ] Unit tests for serialization
- [ ] Documentation

### Phase 2: File Writing
- [ ] Implement `write_consolidating_tag_to_file()`
- [ ] Integrate with `atomic tag create`
- [ ] Test file creation
- [ ] Verify hash calculation
- [ ] Documentation

### Phase 3: File Reading
- [ ] Update `atomic change <tag-hash>`
- [ ] Format tag output
- [ ] Test viewing
- [ ] Documentation

### Phase 4: Log Integration
- [ ] Detect tags in log
- [ ] Add visual indicators
- [ ] Format tag entries
- [ ] Test log output
- [ ] Documentation

### Phase 5: Sync Support
- [ ] Test push with tags
- [ ] Test pull with tags
- [ ] Test clone with tags
- [ ] Handle edge cases
- [ ] Documentation
- [ ] End-to-end tests

---

## Conclusion

Increment 7 transforms consolidating tags from database-only metadata into **first-class changes** that:
- Live alongside regular changes in `.atomic/changes/`
- Display beautifully in logs and change views
- Sync seamlessly to remote repositories
- Provide full transparency into what's consolidated

This completes the core consolidating tags feature, making it production-ready for team collaboration.

**After Increment 7, tags will be indistinguishable from regular changes in terms of visibility and sync behavior, while maintaining their special semantic meaning for dependency consolidation.**

---

**Status**: 📋 Planned  
**Ready to Start**: After Increment 6 Complete  
**Estimated Duration**: 4-5 days  
**Expected Outcome**: Production-ready consolidating tags with full sync support