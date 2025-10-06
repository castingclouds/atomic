# Getting Started with Atomic API Protocol Implementation

## Quick Start for Developers

This guide helps you get started implementing the HTTP protocol completion for atomic-api.

## Prerequisites

### Required Tools
- Rust 1.70+ with cargo
- atomic CLI installed (`cargo install --path=.` from atomic directory)
- curl or httpie for testing
- wscat for WebSocket testing (optional)

### Knowledge Required
- Familiarity with AGENTS.md principles
- Understanding of Atomic VCS concepts (changes, channels, merkle trees)
- Async Rust with tokio/axum
- Basic HTTP protocol knowledge

## Project Structure

```
atomic-api/
├── src/
│   ├── lib.rs          # Public API exports
│   ├── main.rs         # Binary entry point
│   ├── server.rs       # ⭐ Main work happens here
│   ├── error.rs        # Error types (may need updates)
│   ├── message.rs      # WebSocket messages
│   └── websocket.rs    # WebSocket server
├── tests/
│   └── protocol_integration.rs  # ⭐ Create this
├── examples/
│   └── http_server.rs  # ⭐ Create this
├── docs/
│   ├── implementation-plan.md   # Detailed plan
│   ├── progress-checklist.md    # Track progress
│   └── getting-started.md       # This file
└── Cargo.toml
```

## Development Environment Setup

### 1. Clone and Build

```bash
# Navigate to atomic-api
cd atomic/atomic-api

# Build the project
cargo build

# Run tests to ensure baseline works
cargo test

# Check for issues
cargo clippy
```

### 2. Set Up Test Environment

```bash
# Create test data directory
mkdir -p /tmp/atomic-test-data

# Create a test repository
mkdir -p /tmp/test-repo
cd /tmp/test-repo
atomic init
echo "hello" > test.txt
atomic add test.txt
atomic record -m "initial commit"
```

### 3. Run the Server

```bash
# Terminal 1: Start atomic-api
cd atomic/atomic-api
RUST_LOG=debug cargo run -- /tmp/atomic-test-data

# Terminal 2: Test it works
curl http://localhost:8080/health
```

## Understanding the Current Code

### Key Functions in server.rs

#### 1. `post_atomic_protocol()` (line ~420)
**Handles POST operations:**
- `?apply={hash}` - Upload and apply changes ✅ WORKING
- `?tagup={merkle}` - Upload tags ❌ STUBBED (Phase 1)

```rust
// Current stub at line ~570:
if let Some(_tagup_hash) = params.get("tagup") {
    info!("Tag upload operation received (placeholder implementation)");
    // TODO: Implement tag storage
    Ok(Response::builder().status(200).body(Body::empty())?)
}
```

#### 2. `get_atomic_protocol()` (line ~594)
**Handles GET operations:**
- `?channel=main&id` - Get channel ID ✅ WORKING
- `?channel=main&state=` - Get state ✅ WORKING
- `?channel=main&changelist=0` - List changes ✅ WORKING
- `?change={hash}` - Download change ✅ WORKING
- `?tag={merkle}` - Download tag ✅ WORKING

#### 3. `get_changes()` (line ~313)
**REST API endpoint** - Already works, don't touch unless needed

### Important Types

```rust
// In server.rs:
struct AppState {
    base_mount_path: PathBuf,  // e.g., /tenant-data
}

// Path structure:
// /tenant-data/{tenant_id}/{portfolio_id}/{project_id}/.atomic/
```

## Starting Phase 1: Tag Upload

### Step 1: Understand the Current Stub

```bash
# Find the tagup stub
cd atomic/atomic-api
grep -n "tagup" src/server.rs

# Should show line ~570 with stub implementation
```

### Step 2: Study the Required APIs

```rust
// Look at how tags work in libatomic:
cd ../libatomic
grep -r "push_tag_filename" .
grep -r "put_tags" .

// Key files to read:
// - libatomic/src/changestore/filesystem.rs
// - libatomic/src/pristine/sanakirja.rs
```

### Step 3: Look at Similar Working Code

The `?tag={merkle}` GET operation already works. Study it:

```rust
// In get_atomic_protocol(), line ~740:
if let Some(tag_hash) = params.get("tag") {
    if let Some(state) = libatomic::Merkle::from_base32(tag_hash.as_bytes()) {
        let mut tag_path = repository.changes_dir.clone();
        libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &state);
        
        if tag_path.exists() {
            let tag_data = std::fs::read(&tag_path)?;
            // ... format and return
        }
    }
}
```

**Key insight**: Reverse this for tagup - write instead of read!

### Step 4: Write the Implementation

Open `src/server.rs` and replace the stub at line ~570:

```rust
if let Some(tagup_hash) = params.get("tagup") {
    info!("Tag upload operation for state: {}", tagup_hash);
    
    // Parse merkle from base32
    let state = libatomic::Merkle::from_base32(tagup_hash.as_bytes())
        .ok_or_else(|| ApiError::internal("Invalid state format"))?;
    
    // Get tag file path
    let mut tag_path = repository.changes_dir.clone();
    libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &state);
    
    // Create parent directories
    if let Some(parent) = tag_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ApiError::internal(format!("Failed to create tag directory: {}", e)))?;
    }
    
    // Write tag data
    std::fs::write(&tag_path, &body)
        .map_err(|e| ApiError::internal(format!("Failed to write tag file: {}", e)))?;
    
    // Update database
    let mut txn = repository.pristine.mut_txn_begin()
        .map_err(|e| ApiError::internal(format!("Failed to begin transaction: {}", e)))?;
    
    let channel_name = params.get("channel").unwrap_or(&"main".to_string());
    let channel = txn.load_channel(channel_name)
        .map_err(|e| ApiError::internal(format!("Failed to load channel: {}", e)))?
        .ok_or_else(|| ApiError::internal(format!("Channel {} not found", channel_name)))?;
    
    // Find change number for this state
    if let Some(n) = txn.channel_has_state(txn.states(&*channel.read()), &state.into())
        .map_err(|e| ApiError::internal(format!("Failed to check state: {}", e)))? {
        
        let tags = txn.tags_mut(&mut *channel.write());
        txn.put_tags(tags, n.into(), &state)
            .map_err(|e| ApiError::internal(format!("Failed to put tag: {}", e)))?;
        
        txn.commit()
            .map_err(|e| ApiError::internal(format!("Failed to commit: {}", e)))?;
        
        info!("Successfully uploaded tag for state {}", tagup_hash);
    } else {
        return Err(ApiError::internal(format!("State {} not found in channel", tagup_hash)));
    }
    
    return Ok(Response::builder()
        .status(200)
        .header("content-type", "application/octet-stream")
        .body(Body::empty())
        .map_err(|e| ApiError::internal(format!("Failed to build response: {}", e)))?);
}
```

### Step 5: Test It

```bash
# Rebuild
cargo build

# Run server
RUST_LOG=debug cargo run -- /tmp/atomic-test-data

# In another terminal, test with curl
# (You'll need actual tag data - see test section below)
```

## Testing Strategy

### Unit Testing Approach

Create tests in `src/server.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_tagup_with_valid_state() {
        // Create test repository
        // Upload a tag
        // Verify tag file exists
        // Verify database updated
    }
    
    #[test]
    fn test_merkle_parsing() {
        let valid = "MERKLE_BASE32_STRING";
        let result = libatomic::Merkle::from_base32(valid.as_bytes());
        assert!(result.is_some());
    }
}
```

### Integration Testing Approach

Use actual atomic CLI:

```bash
# Create test script
cat > test-tagup.sh << 'EOF'
#!/bin/bash
set -e

# Start server in background
cargo run -- /tmp/atomic-test-data &
SERVER_PID=$!
sleep 2

# Create and push a repository
cd /tmp/test-repo
atomic remote add server "http://localhost:8080/tenant/test/portfolio/main/project/demo/code"
atomic push server

# Create a tag
atomic tag mytag

# Push tag (this will use tagup)
atomic push server --tags

# Verify tag was uploaded
curl "http://localhost:8080/tenant/test/portfolio/main/project/demo/code?channel=main&state=" | grep mytag

# Cleanup
kill $SERVER_PID
EOF

chmod +x test-tagup.sh
./test-tagup.sh
```

## Debugging Tips

### Enable Debug Logging

```bash
# Maximum verbosity
RUST_LOG=trace cargo run -- /tmp/atomic-test-data

# Just atomic-api
RUST_LOG=atomic_api=debug cargo run -- /tmp/atomic-test-data

# Multiple modules
RUST_LOG=atomic_api=debug,libatomic=info cargo run -- /tmp/atomic-test-data
```

### Check What Requests Are Made

```bash
# Use httpie for better output
http GET localhost:8080/tenant/test/portfolio/main/project/demo/code channel==main state==

# Use curl with verbose
curl -v "http://localhost:8080/tenant/test/portfolio/main/project/demo/code?channel=main&state="
```

### Inspect Database

```rust
// Add temporary debug code:
info!("Repository path: {:?}", repository.pristine_dir);
info!("Changes dir: {:?}", repository.changes_dir);

// List what's in database:
let channel = txn.load_channel("main")?;
for entry in txn.log(&*channel.read(), 0)? {
    let (n, (hash, merkle)) = entry?;
    info!("Change {}: {} {}", n, hash, merkle);
}
```

### Common Errors and Solutions

**Error: "Repository not found"**
- Check base_mount_path is correct
- Verify directory structure: `/path/tenant/portfolio/project/.atomic/`
- Ensure .atomic directory exists and is initialized

**Error: "Channel not found"**
- Initialize repository: `atomic init`
- Check channel name in request (default: "main")

**Error: "Invalid state format"**
- Verify merkle is valid base32
- Check merkle length (should be 53 characters)

**Error: "Failed to begin transaction"**
- Check database file permissions
- Ensure no other process has database locked
- Check disk space

## AGENTS.md Compliance Checklist

Before committing, verify:

- [ ] **Error Handling**: All errors use ApiError with context
- [ ] **Logging**: Use tracing macros (info!, debug!, warn!, error!)
- [ ] **Configuration**: Use environment variables where appropriate
- [ ] **Factory Pattern**: Use for complex object creation
- [ ] **Type Safety**: No unwrap() in production code, use ? operator
- [ ] **Documentation**: Add doc comments for new functions
- [ ] **Testing**: Unit tests for new functions
- [ ] **Code Quality**: Run clippy and fix warnings
- [ ] **Formatting**: Run cargo fmt

Example of good code following AGENTS.md:

```rust
/// Handle tag upload operation following AGENTS.md patterns
/// 
/// # Arguments
/// * `params` - Query parameters containing tagup merkle
/// * `body` - Tag data bytes
/// * `repository` - Repository to update
/// 
/// # Errors
/// Returns ApiError if state is invalid or database operation fails
async fn handle_tagup(
    params: &HashMap<String, String>,
    body: &[u8],
    repository: &Repository,
) -> ApiResult<()> {
    // Factory pattern for state creation
    let state = libatomic::Merkle::from_base32(
        params.get("tagup")
            .ok_or_else(|| ApiError::internal("Missing tagup parameter"))?
            .as_bytes()
    ).ok_or_else(|| ApiError::internal("Invalid state format"))?;
    
    // Error handling with context
    let mut txn = repository.pristine.mut_txn_begin()
        .map_err(|e| ApiError::internal(format!("Failed to begin transaction: {}", e)))?;
    
    // ... rest of implementation
    
    Ok(())
}
```

## Next Steps

1. **Complete Phase 1** (Tag Upload)
   - Follow implementation plan
   - Write tests
   - Update progress checklist

2. **Move to Phase 2** (Dependency Validation)
   - Read implementation plan
   - Study existing dependency code in libatomic
   - Create helper function

3. **Continue Through Phases**
   - Each phase builds on previous
   - Test thoroughly before moving on
   - Update documentation as you go

## Getting Help

### Resources
- **AGENTS.md** - Architecture and patterns
- **implementation-plan.md** - Detailed implementation guide
- **progress-checklist.md** - Track your progress
- **libatomic source** - Reference implementation
- **atomic-remote source** - Client-side protocol examples

### Common Questions

**Q: Do I need to modify atomic-remote?**
A: No! atomic-remote is the client, atomic-api is the server. They stay separate.

**Q: Can I break the REST API?**
A: No! Keep all existing routes. Only add new protocol endpoints.

**Q: How do I test with real atomic CLI?**
A: Set up a remote pointing to your local server and use normal atomic commands.

**Q: What if I get stuck on libatomic APIs?**
A: Look at how they're used in atomic-remote or atomic CLI source code.

## Reminder

This is NOT a consolidation project. We're:
- ✅ Completing server-side HTTP protocol
- ✅ Maintaining separation of concerns
- ✅ Following AGENTS.md principles
- ✅ Enabling distributed workflows

We are NOT:
- ❌ Consolidating crates
- ❌ Breaking existing functionality
- ❌ Adding dependencies on atomic-remote
- ❌ Changing the architecture

Good luck! 🚀