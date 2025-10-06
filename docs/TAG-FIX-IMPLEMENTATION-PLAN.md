# Tag Fix Implementation Plan

**Date**: 2025-01-16  
**Status**: Ready for Implementation  
**Priority**: Critical - Blocking push/pull functionality  

---

## Problem Statement

We implemented a hallucinated "consolidating tag as change" system that doesn't align with Atomic's actual tag architecture. Tags in Atomic are simple state references, not changes.

---

## Root Cause Analysis

### What We Got Wrong

1. **Created fake change files for tags** - Tags don't have `.change` files, they have `.tags/<merkle>` files
2. **Tried to apply tags as changes** - Tags aren't applied, they just mark positions in the channel
3. **Built complex metadata serialization** - Tag files are generated from channel state, not serialized metadata
4. **Pushed non-existent change files** - We pushed `CS::Change(hash)` for tags instead of `CS::State(merkle)`

### What Atomic Actually Does

- Tags are **position → Merkle** mappings in the `tags` table
- Tag files are **generated from channel state** using `libatomic::tag::from_channel()`
- Tag push/pull uses **short format** (header only), server regenerates full file
- Tags are **NOT changes** - they don't modify files, have hunks, or get applied

---

## Implementation Plan

### Phase 1: Remove Hallucinated Code (30 minutes)

**File**: `atomic/atomic/src/commands/tag.rs`

**Delete This Function** (lines 604-656):
```rust
fn write_consolidating_tag_as_change<C: ChangeStore>(...)
```

**Remove These Calls** (in tag create handler, around line 300-350):
```rust
// DELETE:
let change_hash = write_consolidating_tag_as_change(
    &change_store,
    &consolidating_tag,
    header.message.clone(),
    header.authors.first().cloned().unwrap_or_else(...),
    header.timestamp,
)?;

consolidating_tag.change_file_hash = Some(change_hash);

info!("Tag change file hash: {}", change_hash.to_base32());

writeln!(
    stdout,
    "{} ({} changes, change file: {})",
    h.to_base32(),
    consolidated_change_count,
    change_hash.to_base32()
)?;
```

**Replace With Simple Output**:
```rust
writeln!(stdout, "{}", h.to_base32())?;
```

**Remove Any Apply Operations for Tags**:
Search for any code that does:
```rust
txn.apply_change_rec(...tag_hash...)?;
```
And delete it if it's applying a tag.

---

### Phase 2: Verify Correct Tag Creation (15 minutes)

**File**: `atomic/atomic/src/commands/tag.rs` (tag create handler)

**Verify This Code Exists and Is NOT Deleted**:
```rust
// Create tag file - THIS IS CORRECT, KEEP IT
let mut w = std::fs::File::create(&temp_path)?;
let header = header(author.as_deref(), tag_message, timestamp).await?;
let h: libatomic::Merkle =
    libatomic::tag::from_channel(&*txn.read(), &channel_name, &header, &mut w)?;
libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &h);
std::fs::create_dir_all(tag_path.parent().unwrap())?;
std::fs::rename(&temp_path, &tag_path)?;

// Update tags table - THIS IS CORRECT, KEEP IT
txn.write().put_tags(&mut channel.write().tags, last_t.into(), &h)?;
```

**The Correct Flow Is**:
1. Generate tag file from channel state ✅
2. Save to `.atomic/changes/.tags/<merkle>` ✅
3. Update tags table ✅
4. Commit transaction ✅
5. Done - NO apply, NO change file, NO complex metadata

---

### Phase 3: Fix Push to Use CS::State (30 minutes)

**Context**: When pushing changes, tags should be identified and pushed as states, not changes.

**Find Where Changes Are Collected for Push**:
Look in `atomic/atomic/src/commands/pushpull.rs` or remote libraries.

**Current Issue**:
We might be trying to push tags as `CS::Change(hash)` where the hash doesn't exist.

**Required Fix**:
```rust
// When collecting items to push
for entry in txn.log(&channel, from_position)? {
    let (pos, (hash, merkle)) = entry?;
    
    // Check if this position is tagged
    if txn.is_tagged(&channel.tags, pos.into())? {
        // Get the tag's merkle hash
        if let Some(tag_merkle) = txn.get_tag(&channel.tags, pos.into())? {
            to_upload.push(CS::State(tag_merkle));
        }
    } else {
        // Regular change
        to_upload.push(CS::Change(hash));
    }
}
```

**Note**: This might already be correct - verify by checking how `CS::State` is used.

---

### Phase 4: Verify SSH Protocol Alignment (15 minutes)

**File**: `atomic/atomic/src/commands/protocol.rs`

**Read and Understand** (lines 186-222):
- How tagup receives short tag data
- How it regenerates full tag file from channel state
- How it updates tags table
- That it does NOT apply anything

**Key Insight**:
```rust
// Client sends SHORT tag
let size: usize = cap[3].parse().unwrap();
let mut buf = vec![0; size];
s.read_exact(&mut buf)?;
let header = libatomic::tag::read_short(std::io::Cursor::new(&buf[..]), &m)?;

// Server REGENERATES full tag from ITS channel state
let mut w = std::fs::File::create(&temp_path)?;
libatomic::tag::from_channel(&*txn.read(), &cap[2], &header, &mut w)?;

// Just update tags table
txn.write().put_tags(&mut channel.write().tags, last_t.into(), &m)?;
```

---

### Phase 5: Fix HTTP API (1 hour)

**File**: `atomic-api/src/server.rs` (or wherever HTTP handlers are)

**Find Tag Upload Handler**:
Look for code handling `?tagup=` or similar parameter.

**Must Match SSH Protocol Exactly**:
```rust
if let Some(tagup_hash) = params.get("tagup") {
    let state = Merkle::from_base32(tagup_hash.as_bytes())
        .ok_or_else(|| anyhow!("Invalid merkle hash"))?;
    
    let channel_name = params.get("to_channel")
        .ok_or_else(|| anyhow!("Missing to_channel parameter"))?;
    
    // Open repository for this tenant/portfolio/project
    let repo = open_repository(&tenant, &portfolio, &project)?;
    let txn = repo.pristine.arc_txn_begin()?;
    let mut channel = txn.write().open_or_create_channel(channel_name)?;
    
    // Verify state matches
    let current_state = libatomic::pristine::current_state(&*txn.read(), &*channel.read())?;
    if current_state != state {
        return Err(anyhow!("State mismatch: expected {}, got {}", 
            state.to_base32(), current_state.to_base32()));
    }
    
    // Check tag doesn't already exist
    let mut tag_path = repo.changes_dir.clone();
    libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &state);
    if tag_path.exists() {
        return Err(anyhow!("Tag already exists"));
    }
    
    // Get position
    let last_t = if let Some(n) = txn.read().reverse_log(&*channel.read(), None)?.next() {
        n?.0.into()
    } else {
        return Err(anyhow!("Channel is empty"));
    };
    
    // Check not already tagged
    if txn.read().is_tagged(&channel.read().tags, last_t)? {
        return Err(anyhow!("Current state already tagged"));
    }
    
    // Parse SHORT tag from request body
    let header = libatomic::tag::read_short(std::io::Cursor::new(&body), &state)?;
    
    // REGENERATE full tag file from server's channel state
    let temp_path = tag_path.with_extension("tmp");
    std::fs::create_dir_all(temp_path.parent().unwrap())?;
    let mut w = std::fs::File::create(&temp_path)?;
    libatomic::tag::from_channel(&*txn.read(), channel_name, &header, &mut w)?;
    std::fs::rename(&temp_path, &tag_path)?;
    
    // Update tags table
    txn.write().put_tags(&mut channel.write().tags, last_t.into(), &state)?;
    
    // Commit - that's it!
    txn.commit()?;
    
    return Ok(Response::new("Tag created successfully"));
}
```

**Critical Points**:
- ✅ Parse SHORT tag from body
- ✅ Regenerate full tag from server channel state  
- ✅ Just update tags table
- ❌ DON'T apply as change
- ❌ DON'T output to working copy
- ❌ DON'T create change files

---

### Phase 6: Fix Apply Handler (30 minutes)

**File**: HTTP API or protocol handler

**Issue**: When changes are applied after push, we might be trying to apply tags.

**Required Check**:
```rust
for change_or_state in to_apply {
    match change_or_state {
        CS::Change(hash) => {
            // Apply regular change
            txn.apply_change_rec(&changes, &mut channel, &hash)?;
        }
        CS::State(merkle) => {
            // For tags, just verify and update tags table
            if let Some(pos) = txn.channel_has_state(&channel.states, &merkle.into())? {
                txn.put_tags(&mut channel.tags, pos.into(), &merkle)?;
            } else {
                return Err(anyhow!(
                    "Cannot add tag {}: channel does not have that state",
                    merkle.to_base32()
                ));
            }
            // DON'T output to working copy for tags
        }
    }
}
```

**Find This Logic**:
- In `atomic/atomic/src/commands/pushpull.rs` pull handler
- In HTTP API apply handler
- Verify tags are handled correctly

---

### Phase 7: Remove Consolidating Tag Database Code (Optional - Future)

**Status**: Can be done later, not blocking

The `ConsolidatingTag` struct and database tables were hallucinated but might not be actively harmful if they're just unused. However, for cleanliness:

**Consider Removing**:
- `libatomic/src/pristine/consolidating_tag.rs`
- Database tables for consolidating tags
- Serialization code for tag metadata
- Any imports or uses of these structures

**Or Keep For Future Use**:
If we want to add rich metadata to tags later, these structures could be repurposed. For now, they're just unused code.

---

## Testing Checklist

### Test 1: Local Tag Creation
```bash
cd /tmp/test-repo
atomic init
echo "test" > file.txt
atomic add file.txt
atomic record -am "Initial commit"
atomic tag create -m "v1.0"
```

**Expected**:
- ✅ Tag file created in `.atomic/changes/.tags/<merkle>`
- ✅ Tag entry in database tags table
- ✅ NO file in `.atomic/changes/<hash>.change`
- ✅ Command completes successfully
- ✅ Output: just the merkle hash

### Test 2: Multiple Tags
```bash
echo "more" >> file.txt
atomic record -am "Second commit"
atomic tag create -m "v2.0"
atomic log
```

**Expected**:
- ✅ Two tag files exist
- ✅ Both tags show in log
- ✅ No errors

### Test 3: Tag Push (SSH)
```bash
# Setup remote
cd /tmp/remote-repo
atomic init
cd /tmp/test-repo
atomic remote add origin ssh://localhost/tmp/remote-repo

# Push
atomic push
```

**Expected**:
- ✅ Changes pushed successfully
- ✅ Tags pushed as CS::State(merkle)
- ✅ Remote has tag files in `.tags/`
- ✅ Remote tags table updated
- ✅ No errors about missing change files

### Test 4: Tag Pull (SSH)
```bash
cd /tmp/another-repo
atomic clone ssh://localhost/tmp/remote-repo .
atomic log
```

**Expected**:
- ✅ Tags present in cloned repo
- ✅ Tag files in `.tags/`
- ✅ Tags show in log
- ✅ Channel state matches

### Test 5: HTTP API Tag Upload
```bash
curl -X POST \
  "http://localhost:8080/atomic/tenant/portfolio/project?tagup=<merkle>&to_channel=main" \
  -d @tag-short-data
```

**Expected**:
- ✅ 200 OK response
- ✅ Tag created on server
- ✅ Server regenerates from its channel state
- ✅ Tags table updated
- ✅ No apply operations

### Test 6: Round Trip Test
```bash
# Create tag locally
atomic tag create -m "v1.0"

# Push to remote
atomic push

# Clone fresh copy
cd /tmp/fresh
atomic clone <remote> .

# Verify tag present
atomic log | grep v1.0
ls .atomic/changes/.tags/
```

**Expected**: Tag present and functional in fresh clone

---

## Success Criteria

- [ ] No fake change files created for tags
- [ ] Tags pushed as CS::State(merkle), not CS::Change(hash)
- [ ] SSH protocol tag handling unchanged (already correct)
- [ ] HTTP protocol tag handling matches SSH exactly
- [ ] Tags don't get applied as changes
- [ ] Push/pull works end-to-end with tags
- [ ] All tests pass
- [ ] No errors in logs

---

## Rollback Plan

If issues arise:

1. **Backup current code**: `git stash` or `git branch backup-before-tag-fix`
2. **Identify issue**: Check which test fails
3. **Revert specific change**: Use git to revert problematic commits
4. **Document issue**: Add to this document for future reference

---

## Files to Modify

### Critical (Must Fix)
1. `atomic/atomic/src/commands/tag.rs` - Remove hallucinated function, fix output
2. `atomic-api/src/server.rs` - Fix HTTP tagup handler to match SSH protocol

### Verify (Might Be Correct Already)
3. `atomic/atomic/src/commands/pushpull.rs` - Verify tags pushed as CS::State
4. `atomic-remote/src/ssh.rs` - Verify tag upload sends short format
5. `atomic-remote/src/http.rs` - Verify tag upload sends short format

### Future Cleanup (Optional)
6. `libatomic/src/pristine/consolidating_tag.rs` - Remove if unused
7. `libatomic/src/pristine/sanakirja.rs` - Remove consolidating tag tables if unused

---

## Time Estimate

- Phase 1 (Remove code): 30 minutes
- Phase 2 (Verify tag creation): 15 minutes
- Phase 3 (Fix push): 30 minutes
- Phase 4 (Read protocol): 15 minutes
- Phase 5 (Fix HTTP): 1 hour
- Phase 6 (Fix apply): 30 minutes
- Phase 7 (Cleanup): Optional
- Testing: 1 hour

**Total**: ~4 hours for critical fixes + testing

---

## Key Principles

1. **Follow protocol.rs** - SSH protocol is the source of truth
2. **Tags ≠ Changes** - Never treat tags as changes
3. **Server is authoritative** - Server regenerates tag files from its channel state
4. **Keep it simple** - Tags are just position → merkle mappings
5. **Test thoroughly** - Push/pull must work end-to-end

---

## References

- `atomic/atomic/src/commands/protocol.rs` lines 186-222 - Correct tagup implementation
- `atomic/atomic/src/commands/protocol.rs` lines 173-183 - Correct tag download
- `atomic/AGENTS.md` - HTTP API Protocol Alignment section
- `atomic/docs/TAG-SYSTEM-ANALYSIS-AND-FIX.md` - Detailed analysis

---

**Status**: Ready to implement  
**Confidence**: High - We know exactly what's wrong and how to fix it  
**Risk**: Low - Mostly removing bad code, keeping good code  

Let's fix this! 🚀