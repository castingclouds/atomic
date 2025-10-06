# HTTP API vs SSH Protocol Comparison

## Overview

This document compares the HTTP API implementation with the SSH protocol to ensure we're following the same patterns established in Atomic's remote protocol design.

## Key Principle

**The HTTP API should behave exactly like the SSH protocol** - it's just a different transport mechanism. The underlying logic for handling changes, tags, and repository operations should be identical.

## Operation Comparison

### 1. Downloading Changes (Pull/Clone)

#### SSH Protocol (protocol.rs)
```
Client sends: "change <hash>\n"
Server responds: <8 bytes length><change file data>
```

#### HTTP API
```
Client requests: GET ?change=<hash>
Server responds: <change file data>
```

**Status**: ✅ **Correct** - HTTP follows the pattern (returns raw change data)

---

### 2. Downloading Tags

#### SSH Protocol (protocol.rs)
```
Client sends: "tag <state>\n"
Server responds: <8 bytes length><SHORT tag data>
```
- Uses `tag.short()` to send only the header (lines 173-183)
- Server sends minimal data, not full tag file

#### HTTP API
```
Client requests: GET ?tag=<state>
Server responds: <8 bytes length><FULL tag data>
```

**Status**: ⚠️ **NEEDS FIX** - HTTP returns full tag file instead of short version

**Fix Required**:
```rust
// In get_atomic_protocol, around line 1147
let mut tag = libatomic::tag::OpenTagFile::open(&tag_path, &state)?;
let mut buf = Vec::new();
tag.short(&mut buf)?;  // Use short() instead of reading full file

let mut formatted_data = Vec::new();
formatted_data.write_u64::<BigEndian>(buf.len() as u64)?;
formatted_data.extend_from_slice(&buf);
response_data = formatted_data;
```

---

### 3. Uploading Tags (Push)

#### SSH Protocol (protocol.rs)
```
Client sends: "tagup <state> <channel> <length>\n<short tag data>"
Server:
  1. Receives short tag data
  2. Parses header with read_short()
  3. REGENERATES full tag file using from_channel()
  4. Saves tag file to disk
  5. Updates database
```
- Server is authoritative - generates tag files from its own channel state (lines 184-224)

#### HTTP API
```
Client sends: POST ?tagup=<state> with <full tag data>
Server:
  1. Receives data
  2. Writes directly to disk
  3. Updates database
```

**Status**: ⚠️ **NEEDS FIX** - HTTP doesn't follow the pattern

**Why This Matters**:
- SSH pattern ensures server generates authoritative tag files
- Client only needs to send minimal metadata
- Eliminates version mismatches and corruption issues
- Server state is always correct

**Fix Required**:
```rust
// In post_atomic_protocol, around line 817
// Instead of writing body directly:
let header = libatomic::tag::read_short(std::io::Cursor::new(&body[..]), &state)?;

// Generate full tag file from channel state
let mut temp_path = tag_path.with_extension("tmp");
let mut w = std::fs::File::create(&temp_path)?;
libatomic::tag::from_channel(&*txn, channel_name, &header, &mut w)?;
std::fs::rename(&temp_path, &tag_path)?;
```

---

### 4. Uploading Changes (Push)

#### SSH Protocol (protocol.rs)
```
Client sends: "apply <channel> <hash> <length>\n<change data>"
Server:
  1. Writes change file to disk
  2. Validates/deserializes change
  3. Applies to channel with apply_change_ws()
  4. Stores channel in 'applied' hashmap
  5. After all commands, outputs to working copy
  6. Commits transaction
```
- Server outputs changes to working copy (lines 358-371)

#### HTTP API
```
Client sends: POST ?apply=<hash> with <change data>
Server:
  1. Writes change file to disk
  2. Validates dependencies
  3. Applies to channel with apply_change_rec()
  4. Outputs to working copy with output_repository_no_pending()
  5. Commits transaction
```

**Status**: ✅ **Correct** - HTTP follows the pattern (as of v1.1.0)

**Note**: We recently fixed this to match SSH behavior by adding the output step.

---

### 5. Changelist (Synchronization)

#### SSH Protocol (protocol.rs)
```
Client sends: "changelist <channel> <from> <paths>\n"
Server responds:
  <n>.<hash>.<merkle>    (normal change)
  <n>.<hash>.<merkle>.   (tagged change - note trailing dot)
  <empty line>           (end of list)
```
- Trailing dot indicates this is a tag (lines 124-173)

#### HTTP API
```
Client requests: GET ?changelist=<from>
Server responds:
  <n>.<hash>.<merkle>    (normal change)
  <n>.<hash>.<merkle>.   (tagged change - note trailing dot)
```

**Status**: ✅ **Correct** - HTTP follows the pattern

---

### 6. State Query

#### SSH Protocol (protocol.rs)
```
Client sends: "state <channel> <n>\n"
Server responds: "<merkle>\n" or "-\n"
```

#### HTTP API
```
Client requests: GET ?state=<channel>
Server responds: "<merkle>\n"
```

**Status**: ✅ **Correct** - HTTP follows the pattern

---

## Critical Differences Found

### 1. Tag Download (GET ?tag=)
- **SSH**: Sends short version (header only)
- **HTTP**: Sends full tag file
- **Impact**: Wastes bandwidth, violates protocol

### 2. Tag Upload (POST ?tagup=)
- **SSH**: Server regenerates tag file from channel state
- **HTTP**: Server writes client data directly
- **Impact**: 
  - Server may have incorrect/corrupt tag files
  - Client needs complete tag file generation
  - Version mismatches possible
  - We added extra complexity trying to generate tag files everywhere

---

## Recommended Fixes

### Priority 1: Fix Tag Upload (tagup)
Refactor HTTP POST ?tagup= to match SSH:
1. Expect short tag data from client
2. Use `read_short()` to parse header
3. Use `from_channel()` to regenerate full tag file
4. This eliminates the need for:
   - Tag file generation in `atomic tag create`
   - Tag file generation after `apply`
   - Complex missing tag file handling

### Priority 2: Fix Tag Download (tag)
Refactor HTTP GET ?tag= to match SSH:
1. Open tag file with `OpenTagFile::open()`
2. Use `tag.short()` to get minimal data
3. Send with length prefix

### Priority 3: Verify All Other Operations
- Review each protocol command in protocol.rs
- Ensure HTTP API has equivalent behavior
- Test edge cases match

---

## Benefits of Following SSH Pattern

1. **Consistency**: Same behavior across all remote protocols
2. **Correctness**: Server is authoritative for tag files
3. **Simplicity**: Less code, fewer edge cases
4. **Reliability**: Tested patterns from existing protocol
5. **Maintainability**: One source of truth for protocol behavior

---

## Testing Checklist

After implementing fixes:
- [ ] Clone via HTTP produces same result as SSH
- [ ] Pull via HTTP produces same result as SSH  
- [ ] Push via HTTP produces same result as SSH
- [ ] Tag upload via HTTP generates correct server-side tag file
- [ ] Tag download via HTTP transfers minimal data
- [ ] Working copy updates correctly after push
- [ ] Changelist format matches exactly

---

## Conclusion

The HTTP API should be a **thin transport wrapper** around the same protocol logic used by SSH. We deviated from this pattern in tag handling, leading to unnecessary complexity. By refactoring to match the SSH protocol exactly, we can:

1. Remove tag file generation from multiple places
2. Simplify push/pull logic
3. Eliminate missing tag file errors
4. Ensure server state is always authoritative
5. Reduce maintenance burden

**Golden Rule**: When implementing any HTTP API feature, first check how the SSH protocol (protocol.rs) handles it, then replicate that exact behavior.