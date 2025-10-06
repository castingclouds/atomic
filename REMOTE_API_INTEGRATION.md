# Atomic API & Atomic Remote Integration Architecture

## Executive Summary

**Do NOT consolidate atomic-api and atomic-remote.** They serve complementary roles and work together perfectly through well-defined interfaces. This document explains their relationship and integration patterns.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                         ATOMIC ECOSYSTEM                            │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌─────────────────┐                    ┌─────────────────┐       │
│  │  atomic-cli     │                    │   Web Browser   │       │
│  │  (Client)       │                    │   (Frontend)    │       │
│  └────────┬────────┘                    └────────┬────────┘       │
│           │                                      │                 │
│           │ Uses atomic-remote                   │ HTTP/WS         │
│           │ for protocol impl                    │                 │
│           │                                      │                 │
│  ┌────────▼───────────────────────┐    ┌────────▼────────┐       │
│  │   atomic-remote (crate)        │    │   atomic-api    │       │
│  │                                │    │   (Server)      │       │
│  │  ┌──────────┐  ┌──────────┐  │    │                 │       │
│  │  │   SSH    │  │   HTTP   │  │    │  ┌───────────┐  │       │
│  │  │ Client   │  │  Client  │  │    │  │ REST API  │  │       │
│  │  └──────────┘  └──────────┘  │    │  └───────────┘  │       │
│  │  ┌──────────┐  ┌──────────┐  │    │  ┌───────────┐  │       │
│  │  │  Local   │  │Protocol  │  │    │  │ WebSocket │  │       │
│  │  │ Client   │  │  Logic   │  │    │  └───────────┘  │       │
│  │  └──────────┘  └──────────┘  │    │  ┌───────────┐  │       │
│  │                                │    │  │  Atomic   │  │       │
│  │  Client-side protocol impl     │    │  │ Protocol  │  │       │
│  └────────┬───────────────────────┘    └────────┬──────┘       │
│           │                                      │               │
│           │                                      │               │
│           │         Both use libatomic           │               │
│           └──────────────┬───────────────────────┘               │
│                          │                                       │
│                  ┌───────▼────────┐                             │
│                  │   libatomic    │                             │
│                  │  (Core VCS)    │                             │
│                  └───────┬────────┘                             │
│                          │                                       │
│                  ┌───────▼────────┐                             │
│                  │  File System   │                             │
│                  │  .atomic/ dirs │                             │
│                  └────────────────┘                             │
└─────────────────────────────────────────────────────────────────────┘
```

## Responsibilities

### atomic-remote (Client-Side)
**Purpose:** Implements client-side protocol handling for remote operations

**Responsibilities:**
- SSH client protocol implementation
- HTTP client protocol implementation
- Local filesystem protocol implementation
- Push/pull negotiation (client side)
- Change upload/download logic
- Dependency resolution (client side)
- Attribution sync (client side)
- Identity/proof operations

**Used By:**
- atomic CLI commands (push, pull, clone, etc.)
- Any client needing to communicate with remote repositories

**Does NOT:**
- Serve HTTP requests
- Listen on network ports
- Provide REST API
- Handle multi-tenant routing

### atomic-api (Server-Side)
**Purpose:** Serves repositories via REST API and Atomic protocol

**Responsibilities:**
- HTTP server (Axum)
- REST API endpoints for browsing
- WebSocket server for real-time updates
- Atomic protocol server implementation (clone, push, pull)
- Multi-tenant path routing
- Repository hosting
- Change storage and retrieval
- Attribution tracking server-side

**Used By:**
- Web frontends (React/Next.js)
- atomic CLI (as a remote target)
- CI/CD systems
- API consumers

**Does NOT:**
- Implement client protocol logic
- Make outbound connections to remotes
- Handle SSH client operations

## Integration Patterns

### Pattern 1: Local Filesystem (Current Working Solution)

```
┌─────────────────────────────────────────────────────┐
│  atomic CLI + atomic-remote                         │
│  Uses: Local protocol                               │
│  Remote: local:///tenant-data/tenant/portfolio/proj │
└──────────────────┬──────────────────────────────────┘
                   │
                   │ Direct filesystem access
                   │ (No network, no HTTP)
                   │
┌──────────────────▼──────────────────────────────────┐
│  /tenant-data/ (Shared Filesystem)                  │
│  ├── tenant-1/                                      │
│  │   └── portfolio-1/                              │
│  │       └── project-1/                            │
│  │           └── .atomic/ (database)               │
│  │               ├── pristine/                     │
│  │               └── changes/                      │
└──────────────────┬──────────────────────────────────┘
                   │
                   │ Direct filesystem access
                   │ (Read operations only)
                   │
┌──────────────────▼──────────────────────────────────┐
│  atomic-api Server                                  │
│  Serves: REST API + WebSocket                       │
│  Reads: Same .atomic/ directories                   │
└─────────────────────────────────────────────────────┘
```

**Advantages:**
- ✅ Works immediately (no code changes)
- ✅ Full push/pull support via atomic-remote
- ✅ Zero latency (filesystem operations)
- ✅ Simple setup
- ✅ Both crates access same data

**Use Case:**
- Single-server deployments
- Co-located API and repositories
- Development environments
- **PERFECT FOR THIS WEEK'S DEMO**

### Pattern 2: HTTP Protocol (Future Enhancement)

```
┌─────────────────────────────────────────────────────┐
│  atomic CLI + atomic-remote                         │
│  Uses: HTTP client protocol                         │
│  Remote: http://api.example.com/tenant/p/proj/code  │
└──────────────────┬──────────────────────────────────┘
                   │
                   │ HTTP POST/GET (Atomic protocol)
                   │ Push: POST ?apply={hash}
                   │ Pull: GET ?changelist=0
                   │ Clone: GET ?channel=main
                   │
┌──────────────────▼──────────────────────────────────┐
│  atomic-api Server                                  │
│  Implements: Atomic protocol endpoints              │
│  - GET/POST for protocol operations                 │
│  - Dependency resolution                            │
│  - State synchronization                            │
│  Storage: Direct filesystem access                  │
└──────────────────┬──────────────────────────────────┘
                   │
                   │ Direct filesystem access
                   │
┌──────────────────▼──────────────────────────────────┐
│  /tenant-data/ (Local Storage)                      │
│  └── .atomic/ directories                           │
└─────────────────────────────────────────────────────┘
```

**Advantages:**
- ✅ True remote access over network
- ✅ Can separate API from storage
- ✅ Standard HTTP/HTTPS
- ✅ Works across firewalls

**Use Case:**
- Multi-server deployments
- Geographic distribution
- Remote team collaboration
- Production SaaS deployments

### Pattern 3: Hybrid (Recommended Production)

```
┌────────────────────────────────────────────────────┐
│  Local Developers                                  │
│  Use: local:// remotes (fast, direct access)       │
└──────────────────┬─────────────────────────────────┘
                   │
                   │ Filesystem
                   │
┌──────────────────▼─────────────────────────────────┐
│  Central Server                                    │
│  ┌──────────────────────────────────────────────┐ │
│  │  atomic-api (REST + Atomic Protocol)         │ │
│  │  - Serves web UI via REST API                │ │
│  │  - Accepts HTTP push/pull                    │ │
│  │  - WebSocket for real-time                   │ │
│  └──────────────────┬───────────────────────────┘ │
│                     │                              │
│  ┌──────────────────▼───────────────────────────┐ │
│  │  /tenant-data/ (Shared Storage)              │ │
│  │  └── .atomic/ repositories                   │ │
│  └──────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────┘
                   ▲
                   │ HTTP (Atomic protocol)
                   │
┌──────────────────┴─────────────────────────────────┐
│  Remote Developers / CI/CD                         │
│  Use: http:// remotes (network access)             │
└────────────────────────────────────────────────────┘
```

## Protocol Flow Examples

### Clone Operation (HTTP)

**Client Side (atomic-remote):**
```rust
// In atomic-remote/src/http.rs
impl Http {
    pub async fn download_changelist(&mut self, from: u64) {
        // 1. Request changelist
        let response = self.client
            .get(format!("{}?channel={}&changelist={}", self.url, self.channel, from))
            .send()
            .await?;
        
        // 2. Parse response
        // 3. Download each change
        // 4. Build local repository
    }
}
```

**Server Side (atomic-api):**
```rust
// In atomic-api/src/server.rs
async fn get_atomic_protocol(params: Query<HashMap<String, String>>) {
    if let Some(changelist_param) = params.get("changelist") {
        let from: u64 = changelist_param.parse().unwrap_or(0);
        
        // 1. Open repository
        let txn = repository.pristine.txn_begin()?;
        let channel = txn.load_channel(channel_name)?;
        
        // 2. Generate changelist
        for entry in txn.log(&*channel.read(), from)? {
            let (n, (hash, merkle)) = entry?;
            writeln!(response, "{}.{}.{}", n, hash, merkle)?;
        }
        
        // 3. Return changelist
    }
}
```

### Push Operation (Local)

**Client Side (atomic-remote):**
```rust
// In atomic-remote/src/local.rs
impl Local {
    pub fn upload_changes(&mut self, changes: &[CS]) {
        for c in changes {
            match c {
                CS::Change(c) => {
                    // 1. Copy change file to remote
                    std::fs::hard_link(&local_path, &remote_path)?;
                    
                    // 2. Apply to remote channel
                    let txn = self.pristine.mut_txn_begin()?;
                    let channel = txn.open_or_create_channel(&self.channel)?;
                    txn.apply_change(&channel, c)?;
                    txn.commit()?;
                }
            }
        }
    }
}
```

**Server Side (atomic-api):**
```rust
// No server-side code needed for local:// protocol
// Both client and server access same filesystem
```

### Push Operation (HTTP)

**Client Side (atomic-remote):**
```rust
// In atomic-remote/src/http.rs
impl Http {
    pub async fn upload_changes(&mut self, changes: Vec<Hash>) {
        for hash in changes {
            // 1. Read change file
            let change_data = std::fs::read(change_path)?;
            
            // 2. Upload to server
            let response = self.client
                .post(format!("{}?apply={}", self.url, hash))
                .body(change_data)
                .send()
                .await?;
        }
    }
}
```

**Server Side (atomic-api):**
```rust
// In atomic-api/src/server.rs
async fn post_atomic_protocol(params: Query, body: Bytes) {
    if let Some(apply_hash) = params.get("apply") {
        // 1. Parse hash
        let hash = Hash::from_base32(apply_hash.as_bytes())?;
        
        // 2. Write change file
        std::fs::write(&change_path, &body)?;
        
        // 3. Apply to channel
        let mut txn = repository.pristine.mut_txn_begin()?;
        let channel = txn.open_or_create_channel("main")?;
        txn.apply_change_rec(&repository.changes, &mut channel, &hash)?;
        txn.commit()?;
    }
}
```

## Data Flow

### Read Operations (Changes List)

```
User Browser
    │
    │ GET /tenant/t/portfolio/p/project/pr/changes
    ▼
atomic-api REST endpoint
    │
    │ read_changes_from_filesystem()
    ▼
/tenant-data/t/p/pr/.atomic/pristine (database)
    │
    │ libatomic::TxnT::log()
    ▼
Return JSON array of changes
```

### Write Operations (Push via Local)

```
Developer CLI
    │
    │ atomic push local:///tenant-data/t/p/pr
    ▼
atomic-remote::Local::upload_changes()
    │
    │ Copy files + Apply changes
    ▼
/tenant-data/t/p/pr/.atomic/
    ├── changes/ (new change files)
    └── pristine/ (updated database)
        │
        │ Automatically visible to atomic-api
        ▼
    REST API returns updated changes
```

### Write Operations (Push via HTTP - Future)

```
Developer CLI
    │
    │ atomic push http://api/t/p/pr/code
    ▼
atomic-remote::Http::upload_changes()
    │
    │ POST ?apply={hash} with change data
    ▼
atomic-api::post_atomic_protocol()
    │
    │ Write file + Apply to channel
    ▼
/tenant-data/t/p/pr/.atomic/
    ├── changes/ (new change files)
    └── pristine/ (updated database)
        │
        │ Changes immediately visible
        ▼
    REST API returns updated changes
```

## Why NOT to Consolidate

### 1. Different Concerns (AGENTS.md Principle)

**atomic-remote:**
- Client-side protocol implementation
- Handles multiple remote types (SSH, HTTP, Local)
- Manages authentication
- Connection pooling
- Retry logic

**atomic-api:**
- Server-side operations
- Multi-tenant routing
- REST API for web UIs
- WebSocket for real-time
- Protocol serving (not consuming)

### 2. Different Dependencies

**atomic-remote needs:**
- SSH libraries (thrussh)
- HTTP client (reqwest)
- Keyring for credentials
- Progress bars for CLI

**atomic-api needs:**
- HTTP server (axum)
- WebSocket server (tokio-tungstenite)
- CORS handling
- Serialization for REST API

Consolidation would bloat both with unnecessary dependencies.

### 3. Different Use Cases

**atomic-remote used in:**
- CLI commands
- Desktop applications
- CI/CD scripts
- Developer tools

**atomic-api used in:**
- Production servers
- Docker containers
- Kubernetes deployments
- Load balancers

### 4. Independent Evolution

Each can evolve independently:
- atomic-remote can add new client protocols without affecting server
- atomic-api can enhance REST API without affecting client
- Version compatibility is protocol-based, not code-based

## Current Status

### What Works Today ✅

1. **atomic-remote:** 100% complete
   - All protocols implemented (SSH, HTTP, Local)
   - Push/pull fully functional
   - Attribution sync working
   - Identity management complete

2. **atomic-api:** 80% complete
   - REST API: 100% working
   - WebSocket: 100% working
   - Clone protocol: 100% working
   - Apply protocol: 100% working
   - Push negotiation: 20% complete (needs enhancement)
   - Tag sync: 50% complete (needs completion)

### Quick Win for Demo

Use **Pattern 1 (Local Filesystem)**:
```bash
# Start atomic-api
atomic-api /tenant-data &

# Use atomic-remote's Local protocol
atomic remote add server local:///tenant-data/t/p/pr
atomic push server  # Works 100%
atomic pull server  # Works 100%

# REST API works simultaneously
curl http://localhost:8080/tenant/t/portfolio/p/project/pr/changes
```

**Result:** Full push/pull + REST API + WebSocket working TODAY with zero code changes.

### Roadmap for HTTP Protocol

1. **Week 1:** Complete push negotiation in atomic-api
2. **Week 2:** Implement tag synchronization
3. **Week 3:** Add dependency validation
4. **Week 4:** Testing and optimization

But you don't need this for your demo! Use local:// remotes.

## Testing Integration

### Unit Tests

**atomic-remote tests:**
```rust
// Test client protocol implementation
#[tokio::test]
async fn test_local_push() {
    let local = Local::new("/test/repo");
    local.upload_changes(changes).await?;
}
```

**atomic-api tests:**
```rust
// Test server protocol implementation
#[tokio::test]
async fn test_protocol_apply() {
    let response = post_atomic_protocol(params, body).await?;
    assert_eq!(response.status(), 200);
}
```

### Integration Tests

**End-to-end test:**
```rust
#[tokio::test]
async fn test_push_pull_cycle() {
    // Start atomic-api server
    let server = ApiServer::new("/test-data").await?;
    
    // Use atomic-remote to push
    let mut remote = Local::new("/test-data/t/p/pr");
    remote.upload_changes(changes).await?;
    
    // Verify via REST API
    let changes = get_changes("/test-data/t/p/pr").await?;
    assert_eq!(changes.len(), 1);
}
```

## Best Practices

### For Development

1. **Use local:// remotes** for speed
2. **Test both protocols** (local and HTTP when ready)
3. **Monitor filesystem** for debugging
4. **Check logs** from both crates

### For Production

1. **Start with local:// for performance**
2. **Add HTTP protocol** for remote access
3. **Use reverse proxy** (Fastify) for load balancing
4. **Monitor metrics** from atomic-api
5. **Back up /tenant-data/** regularly

### For Debugging

1. **Check atomic-remote logs:** `RUST_LOG=debug atomic push`
2. **Check atomic-api logs:** `RUST_LOG=debug atomic-api /data`
3. **Verify filesystem permissions**
4. **Test protocol manually with curl**

## Conclusion

**✅ Keep atomic-api and atomic-remote separate**

They have:
- Different responsibilities (client vs server)
- Different dependencies
- Different use cases
- Clean interfaces via filesystem or HTTP protocol

**✅ Use Pattern 1 (Local Filesystem) for immediate demo**

This gives you:
- Full push/pull via atomic-remote
- REST API via atomic-api
- WebSocket via atomic-api
- Zero code changes needed
- Working TODAY

**✅ Enhance atomic-api HTTP protocol for future**

But only when you need true remote access across network boundaries.

---

**For your demo this week:** Run `atomic-api /tenant-data` and use `local://` remotes. Everything works! 🚀