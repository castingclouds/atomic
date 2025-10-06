# HTTP API Quick Reference

## Golden Rule

**Always check `atomic/src/commands/protocol.rs` first, then replicate that exact behavior in the HTTP API.**

The HTTP API is a thin transport wrapper around the SSH protocol - they must behave identically.

---

## Protocol Commands Reference

### 1. Apply (Push Change)

**SSH Protocol:**
```
apply <channel> <hash> <length>
<change_data>
```

**HTTP API:**
```
POST /tenant/{id}/portfolio/{id}/project/{id}/code?apply=<hash>&to_channel=<channel>
Body: <change_data>
```

**Server Implementation:**
```rust
// 1. Write change file
std::fs::write(&change_path, &body)?;

// 2. Apply to channel (use ArcTxn for output compatibility)
let txn = repository.pristine.arc_txn_begin()?;
let mut channel = txn.write().open_or_create_channel(channel_name)?;
txn.write().apply_change_rec(&repository.changes, &mut channel.write(), &hash)?;

// 3. Output to working copy (REQUIRED - matches SSH protocol)
libatomic::output::output_repository_no_pending(
    &repository.working_copy,
    &repository.changes,
    &txn,
    &channel,
    "",
    true,
    None,
    std::thread::available_parallelism().unwrap().get(),
    0,
)?;

// 4. Commit
txn.commit()?;
```

---

### 2. Tagup (Push Tag)

**SSH Protocol:**
```
tagup <state> <channel> <length>
<short_tag_data>
```

**HTTP API:**
```
POST /tenant/{id}/portfolio/{id}/project/{id}/code?tagup=<state>&to_channel=<channel>
Body: <short_tag_data>
```

**Server Implementation:**
```rust
// 1. Parse SHORT tag header from client
let state = Merkle::from_base32(tagup_hash.as_bytes())?;
let header = libatomic::tag::read_short(std::io::Cursor::new(&body[..]), &state)?;

// 2. REGENERATE full tag file from server's channel state (server is authoritative)
let temp_path = tag_path.with_extension("tmp");
let txn = repository.pristine.txn_begin()?;
let mut w = std::fs::File::create(&temp_path)?;
libatomic::tag::from_channel(&txn, channel_name, &header, &mut w)?;
std::fs::rename(&temp_path, &tag_path)?;

// 3. Update database
let mut mut_txn = repository.pristine.mut_txn_begin()?;
let channel = mut_txn.load_channel(channel_name)?;
mut_txn.put_tags(&mut channel.write().tags, last_t.into(), &state)?;
mut_txn.commit()?;
```

**Key Point:** Server REGENERATES the full tag file from its own state. Client only sends header.

---

### 3. Change (Download Change)

**SSH Protocol:**
```
change <hash>
<8_bytes_length><change_data>
```

**HTTP API:**
```
GET /tenant/{id}/portfolio/{id}/project/{id}/code?change=<hash>
Response: <change_data>
```

**Server Implementation:**
```rust
if let Some(change_hash) = params.get("change") {
    let hash = change_hash.parse::<libatomic::Hash>()?;
    let mut change_path = repository.changes_dir.clone();
    libatomic::changestore::filesystem::push_filename(&mut change_path, &hash);
    
    let change_data = std::fs::read(&change_path)?;
    response_data.extend_from_slice(&change_data);
}
```

---

### 4. Tag (Download Tag)

**SSH Protocol:**
```
tag <state>
<8_bytes_length><short_tag_data>
```

**HTTP API:**
```
GET /tenant/{id}/portfolio/{id}/project/{id}/code?tag=<state>
Response: <8_bytes_length><short_tag_data>
```

**Server Implementation:**
```rust
if let Some(tag_hash) = params.get("tag") {
    let state = Merkle::from_base32(tag_hash.as_bytes())?;
    let mut tag_path = repository.changes_dir.clone();
    libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &state);
    
    // Open and get SHORT version (NOT full file)
    let mut tag = libatomic::tag::OpenTagFile::open(&tag_path, &state)?;
    let mut buf = Vec::new();
    tag.short(&mut buf)?;
    
    // Protocol format: <8 bytes length><short data>
    let mut formatted_data = Vec::new();
    formatted_data.write_u64::<BigEndian>(buf.len() as u64)?;
    formatted_data.extend_from_slice(&buf);
    response_data = formatted_data;
}
```

**Key Point:** Send SHORT version using `tag.short()`, NOT full file.

---

### 5. Changelist (Synchronization)

**SSH Protocol:**
```
changelist <channel> <from> <paths>
<n>.<hash>.<merkle>     (normal change)
<n>.<hash>.<merkle>.    (tagged change - note trailing dot)

```

**HTTP API:**
```
GET /tenant/{id}/portfolio/{id}/project/{id}/code?changelist=<from>&channel=<channel>
Response: Same format as SSH
```

**Server Implementation:**
```rust
if let Some(changelist_param) = params.get("changelist") {
    let from: u64 = changelist_param.parse().unwrap_or(0);
    let channel = txn.load_channel(channel_name)?;
    
    for (n, (hash, merkle)) in txn.log(&*channel.read(), from)? {
        let hash: libatomic::Hash = hash.into();
        let merkle: libatomic::Merkle = merkle.into();
        let is_tagged = txn.is_tagged(txn.tags(&*channel.read()), n.into())?;
        
        if is_tagged {
            writeln!(response, "{}.{}.{}.", n, hash.to_base32(), merkle.to_base32())?;
        } else {
            writeln!(response, "{}.{}.{}", n, hash.to_base32(), merkle.to_base32())?;
        }
    }
    writeln!(response)?; // Empty line to end
}
```

**Key Point:** Trailing dot indicates tagged change.

---

### 6. State (Query State)

**SSH Protocol:**
```
state <channel> <n>
<merkle>
```

**HTTP API:**
```
GET /tenant/{id}/portfolio/{id}/project/{id}/code?state=<channel>
Response: <merkle>\n
```

**Server Implementation:**
```rust
if let Some(state_param) = params.get("state") {
    let channel = txn.load_channel(channel_name)?;
    let state = libatomic::pristine::current_state(&txn, &*channel.read())?;
    writeln!(response, "{}", state.to_base32())?;
}
```

---

## Common Patterns

### Transaction Management

```rust
// For read-only operations
let txn = repository.pristine.txn_begin()?;

// For mutable operations that need output
let txn = repository.pristine.arc_txn_begin()?;  // ArcTxn for output_repository_no_pending

// For simple mutable operations
let mut txn = repository.pristine.mut_txn_begin()?;
```

### Path Construction

```rust
// Change file
let mut path = repository.changes_dir.clone();
libatomic::changestore::filesystem::push_filename(&mut path, &hash);

// Tag file
let mut path = repository.changes_dir.clone();
libatomic::changestore::filesystem::push_tag_filename(&mut path, &merkle);

// Clean up after use
libatomic::changestore::filesystem::pop_filename(&mut path);
```

### Error Handling

```rust
// Follow AGENTS.md error handling patterns
operation().map_err(|e| {
    ApiError::internal(format!("Failed to {}: {}", operation_name, e))
})?;
```

---

## Checklist for New Endpoints

When adding a new HTTP API endpoint:

- [ ] Check `atomic/src/commands/protocol.rs` for SSH protocol behavior
- [ ] Use same transaction types (ArcTxn vs MutTxn)
- [ ] Use same file operations (push_filename, push_tag_filename)
- [ ] Use same library functions (apply_change_rec, from_channel, etc.)
- [ ] Output to working copy after apply operations
- [ ] Handle tags with short version (not full file)
- [ ] Match response format exactly (including trailing dots, newlines)
- [ ] Add equivalent error handling
- [ ] Test against SSH protocol to verify identical behavior

---

## Anti-Patterns to Avoid

### ❌ Sending Full Tag Files

```rust
// WRONG
let tag_data = std::fs::read(&tag_path)?;
response.write_all(&tag_data)?;
```

```rust
// CORRECT
let mut tag = OpenTagFile::open(&tag_path, &state)?;
tag.short(&mut response)?;
```

### ❌ Writing Client Tag Data Directly

```rust
// WRONG - client may send corrupted data
std::fs::write(&tag_path, &body)?;
```

```rust
// CORRECT - server regenerates from its state
let header = read_short(Cursor::new(&body[..]), &state)?;
from_channel(&txn, channel, &header, &mut w)?;
```

### ❌ Skipping Working Copy Output

```rust
// WRONG - server files don't update
txn.apply_change_rec(&changes, &mut channel, &hash)?;
txn.commit()?;
```

```rust
// CORRECT - matches SSH protocol
txn.apply_change_rec(&changes, &mut channel, &hash)?;
output_repository_no_pending(&working_copy, &changes, &txn, &channel, ...)?;
txn.commit()?;
```

---

## Testing

```rust
#[test]
fn test_http_matches_ssh_protocol() {
    // Setup
    let ssh_repo = init_repo();
    let http_repo = init_repo();
    
    // Apply same operation via both protocols
    run_ssh_protocol(&ssh_repo, "apply main HASH 100\n<data>");
    http_post(&http_repo, "?apply=HASH&to_channel=main", data);
    
    // Verify identical results
    assert_eq!(read_channel_state(&ssh_repo), read_channel_state(&http_repo));
    assert_eq!(read_working_copy(&ssh_repo), read_working_copy(&http_repo));
}
```

---

## Reference

- **SSH Protocol Implementation:** `atomic/src/commands/protocol.rs`
- **HTTP API Implementation:** `atomic-api/src/server.rs`
- **Detailed Comparison:** `atomic/docs/HTTP-API-PROTOCOL-COMPARISON.md`
- **Architecture Guide:** `atomic/AGENTS.md` - Section 13: HTTP API Protocol Alignment