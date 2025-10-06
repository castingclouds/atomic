# Push/Pull Integration Analysis for atomic-api and atomic-remote

## Executive Summary

After analyzing both `atomic-api` and `atomic-remote` crates, **we should NOT consolidate them**. Instead, we should complete the existing Atomic protocol implementation in `atomic-api` to enable full push/pull capabilities. The current implementation is ~70% complete and can be enhanced to support your demo requirements this week.

## Current State Analysis

### atomic-api (Current Capabilities)
✅ **Already Implemented:**
- REST API endpoints for reading changes
- WebSocket support for real-time updates
- Atomic protocol GET operations (clone, changelist, state, id)
- Atomic protocol POST operations (apply changes)
- Multi-tenant path routing
- Change file storage and retrieval
- Basic channel operations

❌ **Missing for Full Push/Pull:**
- Protocol-level push negotiation
- Dependency resolution during push
- State synchronization
- Tag management
- Identity/proof operations
- Archive operations for conflict resolution

### atomic-remote (Current Capabilities)
✅ **Fully Implemented:**
- Complete protocol implementations (SSH, HTTP, Local)
- Push/pull with dependency resolution
- Clone operations
- Tag management
- Identity/proof operations
- Archive operations
- Attribution sync support

## Architecture Recommendation: Keep Separate + Enhance

Following AGENTS.md principles:

```
┌─────────────────────────────────────────────────────────┐
│                    atomic-cli (client)                   │
│                                                          │
│    Uses: atomic-remote for client-side operations       │
└────────────────┬────────────────────────────────────────┘
                 │
                 │ HTTP/SSH/Local protocols
                 │
                 ▼
┌─────────────────────────────────────────────────────────┐
│              atomic-api (server/remote)                  │
│                                                          │
│    ✅ Complete Atomic protocol implementation            │
│    ✅ Multi-tenant repository hosting                    │
│    ✅ REST API for web interfaces                        │
│    ✅ WebSocket for real-time updates                    │
│                                                          │
│    Uses: libatomic directly (no atomic-remote needed)   │
└─────────────────────────────────────────────────────────┘
```

### Why This Approach?

1. **Single Responsibility (AGENTS.md)**
   - `atomic-remote`: Client-side remote operations (SSH, HTTP, Local)
   - `atomic-api`: Server-side protocol implementation + REST API

2. **No Circular Dependencies**
   - atomic-api depends on libatomic only
   - atomic-remote depends on libatomic only
   - They never need to depend on each other

3. **Direct Rust Integration (AGENTS.md)**
   - atomic-api uses libatomic directly for all operations
   - No need for atomic-remote abstractions server-side

4. **Minimal Dependencies (AGENTS.md)**
   - Keep atomic-api focused on server operations
   - atomic-remote handles client protocol complexity

## Implementation Plan for This Week

### Phase 1: Complete Atomic Protocol Server (2-3 hours)

Add these endpoints to `atomic-api/src/server.rs`:

#### 1. Protocol Discovery Endpoint
```rust
// GET /tenant/{tenant}/portfolio/{portfolio}/project/{project}/code/.atomic/v1
// Returns protocol version and capabilities
async fn get_protocol_info() -> Json<ProtocolInfo> {
    Json(ProtocolInfo {
        version: 3,
        capabilities: vec!["push", "pull", "clone", "attribution"],
    })
}
```

#### 2. Complete Push Protocol
```rust
// POST /tenant/{tenant}/portfolio/{portfolio}/project/{project}/code/.atomic/v1?push=true
// Body: list of change hashes to push
// Returns: list of missing dependencies
async fn post_push_negotiate() {
    // 1. Receive list of changes client wants to push
    // 2. Check which changes already exist in repository
    // 3. Return list of missing dependencies
    // 4. Client uploads missing changes via apply endpoint
}
```

#### 3. Pull Protocol Enhancement
```rust
// GET /tenant/{tenant}/portfolio/{portfolio}/project/{project}/code/.atomic/v1?pull=true&from=0
// Returns: list of changes client needs with dependencies
async fn get_pull_negotiate() {
    // 1. Return changelist from specified position
    // 2. Include dependency information
    // 3. Client downloads via existing change endpoint
}
```

#### 4. State Sync Enhancement
```rust
// GET /tenant/{tenant}/portfolio/{portfolio}/project/{project}/code/.atomic/v1?state={channel}
// Returns current channel state with merkle tree
// POST /tenant/{tenant}/portfolio/{portfolio}/project/{project}/code/.atomic/v1?tagup={state}
// Uploads a tag/state marker
```

### Phase 2: Test with Real Atomic Client (1 hour)

Create test script:
```bash
#!/bin/bash
# Test atomic-api as a remote

# Start atomic-api server
atomic-api /tenant-data &
API_PID=$!

# Create test repository
atomic init test-repo
cd test-repo

# Add atomic-api as remote
atomic remote add api-server "http://localhost:8080/tenant/test/portfolio/main/project/demo/code"

# Test operations
echo "Testing clone..."
atomic clone api-server test-clone

echo "Testing push..."
echo "test" > file.txt
atomic add file.txt
atomic record -m "test change"
atomic push api-server

echo "Testing pull..."
cd test-clone
atomic pull api-server

# Cleanup
kill $API_PID
```

### Phase 3: Attribution Integration (1 hour)

The attribution sync is already partially implemented via environment variables:

```rust
// In post_push()
if request.with_attribution {
    std::env::set_var("ATOMIC_ATTRIBUTION_SYNC_PUSH", "true");
}
```

Complete by:
1. Adding attribution metadata to protocol responses
2. Storing attribution bundles alongside changes
3. Syncing attribution on pull operations

## What's Already Working

### Clone Operation ✅
```bash
# This should already work:
atomic clone http://localhost:8080/tenant/test/portfolio/main/project/demo/code my-clone
```

The current implementation handles:
- Protocol discovery (GET with no params)
- Channel ID retrieval (GET ?channel=main&id)
- State retrieval (GET ?channel=main&state=)
- Changelist retrieval (GET ?channel=main&changelist=0)
- Change download (GET ?change={hash})
- Tag download (GET ?tag={merkle})

### Apply (Push Part 1) ✅
```bash
# This should already work:
atomic push http://localhost:8080/tenant/test/portfolio/main/project/demo/code
```

The current implementation handles:
- Change upload (POST ?apply={hash})
- Change application to channel
- Duplicate detection

## What Needs Completion

### Push Negotiation ❌
Currently, push doesn't negotiate what's needed. It blindly sends everything.

**Solution:** Add push negotiation endpoint:
```rust
// POST ?push_negotiate with list of hashes
// Returns: { "need": ["hash1", "hash2"], "have": ["hash3"] }
```

### Dependency Resolution ❌
Server doesn't validate dependency order during push.

**Solution:** Check dependencies before applying:
```rust
async fn validate_dependencies(txn: &Txn, channel: &Channel, hash: &Hash) -> bool {
    for dep in get_dependencies(hash) {
        if !txn.has_change(channel, &dep)? {
            return false;
        }
    }
    true
}
```

### State Synchronization ❌
Tag upload (tagup) is stubbed out.

**Solution:** Implement tag storage:
```rust
async fn post_atomic_protocol() {
    if let Some(tagup_hash) = params.get("tagup") {
        // Parse state merkle
        // Store tag file
        // Update channel tags
    }
}
```

## File Changes Needed

### 1. `atomic-api/src/server.rs`

Add these handlers:

```rust
// Add to serve() method routing:
.route(
    "/tenant/:tenant_id/portfolio/:portfolio_id/project/:project_id/code/.atomic/v1",
    get(get_atomic_protocol_v1).post(post_atomic_protocol_v1)
)

// New handler for versioned protocol
async fn get_atomic_protocol_v1(...) {
    // Enhanced version of get_atomic_protocol
    // Add push/pull negotiation
}

async fn post_atomic_protocol_v1(...) {
    // Enhanced version of post_atomic_protocol  
    // Add push negotiation
    // Complete tagup implementation
}
```

### 2. `atomic-api/src/protocol.rs` (New File)

Extract protocol logic into dedicated module:

```rust
pub mod protocol {
    pub struct ProtocolHandler {
        repo: Repository,
    }
    
    impl ProtocolHandler {
        pub async fn handle_push_negotiate(&self, hashes: Vec<Hash>) -> Vec<Hash> {
            // Return list of needed dependencies
        }
        
        pub async fn handle_pull_negotiate(&self, from: u64) -> Changelist {
            // Return changes with dependencies
        }
        
        pub async fn handle_tagup(&self, state: Merkle, data: Vec<u8>) {
            // Store tag
        }
    }
}
```

### 3. `atomic-api/tests/integration_test.rs` (New File)

```rust
#[tokio::test]
async fn test_full_push_pull_workflow() {
    // Start server
    // Clone repo
    // Make changes
    // Push changes
    // Pull changes
    // Verify sync
}
```

## Testing Strategy

### Unit Tests
- Test protocol negotiation logic
- Test dependency resolution
- Test state synchronization

### Integration Tests
- Test with real atomic CLI client
- Test multi-tenant isolation
- Test concurrent push/pull operations

### Manual Testing
1. Start atomic-api server
2. Clone a repository via HTTP
3. Make changes and push
4. Pull from another clone
5. Verify changes propagate correctly

## Timeline

**Day 1 (4 hours):**
- Complete push negotiation endpoint
- Implement dependency validation
- Complete tagup implementation

**Day 2 (2 hours):**
- Write integration tests
- Test with real atomic CLI
- Fix any protocol issues

**Day 3 (2 hours):**
- Add attribution sync
- Performance testing
- Documentation

## Alternative: Quick Workaround for Demo

If you need something working TODAY for a demo, you can:

1. **Use atomic-remote's Local implementation as-is**
   - Set up local filesystem remotes pointing to /tenant-data
   - Use atomic push/pull with local:// URLs
   - This works immediately with zero code changes

2. **Proxy through atomic-api**
   - Keep atomic-api for REST API
   - Use local filesystem for push/pull
   - Web UI reads via REST, CLI uses local://

Example:
```bash
# Demo setup
atomic-api /tenant-data &  # REST API for web UI

# CLI operations use local remotes
atomic remote add demo local:///tenant-data/tenant-1/portfolio-1/project-1
atomic push demo
atomic pull demo
```

## Conclusion

**Recommendation: Complete atomic-api protocol implementation**

- ✅ Maintains separation of concerns (AGENTS.md)
- ✅ No consolidation needed
- ✅ Can be completed in 2-3 days
- ✅ Enables full demo capabilities
- ✅ Keeps architecture clean

**For immediate demo needs:**
- Use local:// remotes with filesystem paths
- atomic-api provides REST API for web interface
- CLI uses local protocol for push/pull

**Do NOT consolidate the crates** - they serve different purposes and should remain independent following the Single Responsibility Principle from AGENTS.md.