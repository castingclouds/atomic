# Atomic API Demo Quick Start Guide

## TL;DR - Get Push/Pull Working Today

You have **two options** for enabling push/pull in your demo this week:

### Option 1: Use Local Remotes (Works RIGHT NOW - 5 minutes)
No code changes needed. Use filesystem-based remotes with atomic-api providing REST API.

### Option 2: Complete HTTP Protocol (2-3 days of work)
Finish the atomic protocol implementation in atomic-api for full HTTP push/pull.

---

## Option 1: Local Remotes (Recommended for Immediate Demo)

This leverages the **fully functional** `atomic-remote` Local implementation.

### Setup (5 minutes)

```bash
# 1. Start atomic-api for REST API and WebSocket
atomic-api /tenant-data &
API_PID=$!

# Atomic API is now serving:
# - REST API: http://localhost:8080/tenant/{t}/portfolio/{p}/project/{pr}/changes
# - WebSocket: ws://localhost:8081/
# - Repository data: /tenant-data/{tenant}/{portfolio}/{project}/.atomic/

# 2. Create a test repository structure
mkdir -p /tenant-data/acme-corp/web-platform/frontend-app
cd /tmp/my-project

# 3. Initialize and configure
atomic init
atomic remote add demo-server "local:///tenant-data/acme-corp/web-platform/frontend-app"

# 4. Push your changes
echo "console.log('Hello world');" > app.js
atomic add app.js
atomic record -m "Initial commit"
atomic push demo-server

# 5. Clone from another location
cd /tmp
atomic clone local:///tenant-data/acme-corp/web-platform/frontend-app my-clone
```

### How It Works

```
┌─────────────────────────────────────────────────────┐
│  Web Browser                                        │
│  ↓ REST API calls                                   │
│  http://localhost:8080/tenant/acme-corp/...changes  │
└──────────────────┬──────────────────────────────────┘
                   │
                   ↓
┌─────────────────────────────────────────────────────┐
│  atomic-api (REST + WebSocket Server)               │
│  - Serves change lists via REST                     │
│  - Real-time updates via WebSocket                  │
│  - Reads from: /tenant-data/                        │
└──────────────────┬──────────────────────────────────┘
                   │
                   │ (same filesystem)
                   │
┌──────────────────┴──────────────────────────────────┐
│  /tenant-data/ (Shared Storage)                     │
│  ├── acme-corp/                                     │
│  │   └── web-platform/                             │
│  │       └── frontend-app/                         │
│  │           └── .atomic/ (repository database)    │
└──────────────────┬──────────────────────────────────┘
                   │
                   ↑ local:// protocol
                   │
┌──────────────────┴──────────────────────────────────┐
│  atomic CLI (Push/Pull Client)                      │
│  - Uses atomic-remote's Local implementation        │
│  - Full push/pull support                           │
│  - Zero latency (filesystem operations)             │
└─────────────────────────────────────────────────────┘
```

### Advantages

✅ **Works immediately** - No code changes required
✅ **Full push/pull support** - atomic-remote Local is 100% complete
✅ **REST API works** - atomic-api serves web UI needs
✅ **Zero network latency** - Direct filesystem access
✅ **Multi-tenant support** - Path-based tenant isolation
✅ **Attribution tracking** - Works with existing implementation

### Demo Script

```bash
#!/bin/bash
# demo.sh - Complete atomic-api demo with push/pull

set -e

echo "🚀 Starting Atomic API Demo..."

# Start servers
echo "1. Starting atomic-api server..."
atomic-api /tenant-data &
API_PID=$!
sleep 2

echo "2. Setting up test repository..."
DEMO_PATH="/tenant-data/acme-corp/web-platform/frontend-app"
mkdir -p $DEMO_PATH

# Initialize project
cd /tmp/demo-project
atomic init
atomic remote add server "local://$DEMO_PATH"

echo "3. Creating and pushing changes..."
cat > README.md <<EOF
# Frontend App

This is a demo of Atomic VCS with atomic-api.
EOF

atomic add README.md
atomic record -m "Add README"
atomic push server

echo "4. Testing REST API..."
curl -s "http://localhost:8080/tenant/acme-corp/portfolio/web-platform/project/frontend-app/changes?limit=10" | jq

echo "5. Cloning from another location..."
cd /tmp
atomic clone "local://$DEMO_PATH" demo-clone

echo "6. Making changes in clone..."
cd demo-clone
echo "console.log('Demo');" > app.js
atomic add app.js
atomic record -m "Add app.js"
atomic push

echo "7. Pulling changes in original..."
cd /tmp/demo-project
atomic pull server

echo "8. Verifying sync..."
ls -la
cat app.js

echo "9. Testing WebSocket..."
wscat -c ws://localhost:8081 <<EOF
{"id":"test","timestamp":"$(date -Iseconds)","payload":{"type":"health_check"}}
EOF

echo "✅ Demo complete!"
kill $API_PID
```

---

## Option 2: Complete HTTP Protocol (For Production Use)

If you need true HTTP-based push/pull (not just local filesystem), here's what to implement:

### What's Already Working

✅ Clone via HTTP (GET operations)
✅ Apply changes (POST operations)
✅ REST API for browsing
✅ WebSocket for updates

### What Needs Implementation

❌ Push negotiation (which changes to send)
❌ Dependency resolution (correct order)
❌ Tag synchronization (state markers)

### Implementation Steps

#### 1. Add Push Negotiation Endpoint

**File:** `atomic-api/src/server.rs`

```rust
// Add to serve() routing:
.route(
    "/tenant/:tenant_id/portfolio/:portfolio_id/project/:project_id/code/.atomic/v1/negotiate",
    post(post_push_negotiate)
)

async fn post_push_negotiate(
    State(state): State<AppState>,
    Path((tenant_id, portfolio_id, project_id)): Path<(String, String, String)>,
    Json(request): Json<PushNegotiateRequest>,
) -> ApiResult<Json<PushNegotiateResponse>> {
    // 1. Open repository
    let repo_path = state.base_mount_path
        .join(&tenant_id)
        .join(&portfolio_id)
        .join(&project_id);
    
    let repository = Repository::find_root(Some(repo_path))?;
    let txn = repository.pristine.txn_begin()?;
    let channel = txn.load_channel(&request.channel)?.unwrap();
    
    // 2. Check which hashes we already have
    let mut need = Vec::new();
    let mut have = Vec::new();
    
    for hash_str in &request.hashes {
        let hash = hash_str.parse::<libatomic::Hash>()?;
        
        if txn.has_change(&channel, &hash)?.is_some() {
            have.push(hash_str.clone());
        } else {
            need.push(hash_str.clone());
        }
    }
    
    // 3. Return what we need
    Ok(Json(PushNegotiateResponse { need, have }))
}

#[derive(Deserialize)]
struct PushNegotiateRequest {
    channel: String,
    hashes: Vec<String>,
}

#[derive(Serialize)]
struct PushNegotiateResponse {
    need: Vec<String>,
    have: Vec<String>,
}
```

#### 2. Complete Tag Upload (State Sync)

```rust
// In post_atomic_protocol(), replace the tagup stub:
if let Some(tagup_hash) = params.get("tagup") {
    info!("Tag upload operation for state: {}", tagup_hash);
    
    // Parse state merkle
    let state = libatomic::Merkle::from_base32(tagup_hash.as_bytes())
        .ok_or_else(|| ApiError::internal("Invalid state format"))?;
    
    // Store tag file
    let mut tag_path = repository.changes_dir.clone();
    libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &state);
    
    // Create directory
    if let Some(parent) = tag_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    
    // Write tag data
    std::fs::write(&tag_path, &body)?;
    
    // Update channel tags in database
    let mut txn = repository.pristine.mut_txn_begin()?;
    let channel = txn.load_channel(&params.get("channel").unwrap_or(&"main".to_string()))?
        .ok_or_else(|| ApiError::internal("Channel not found"))?;
    
    // Find the change number for this state
    if let Some(n) = txn.channel_has_state(txn.states(&*channel.read()), &state.into())? {
        let tags = txn.tags_mut(&mut *channel.write());
        txn.put_tags(tags, n.into(), &state)?;
        txn.commit()?;
        
        info!("Successfully uploaded tag for state {}", tagup_hash);
    }
    
    return Ok(Response::builder()
        .status(200)
        .body(Body::empty())?);
}
```

#### 3. Add Dependency Validation

```rust
async fn validate_change_dependencies(
    txn: &libatomic::pristine::sanakirja::Txn,
    channel: &ChannelRef<libatomic::pristine::sanakirja::Txn>,
    hash: &libatomic::Hash,
    repository: &Repository,
) -> ApiResult<Vec<libatomic::Hash>> {
    let mut missing = Vec::new();
    
    // Load change to get dependencies
    let mut change_path = repository.changes_dir.clone();
    libatomic::changestore::filesystem::push_filename(&mut change_path, hash);
    
    if !change_path.exists() {
        return Ok(missing);
    }
    
    // Parse change file to extract dependencies
    let change_data = std::fs::read(&change_path)
        .map_err(|e| ApiError::internal(format!("Failed to read change: {}", e)))?;
    
    let change = libatomic::change::Change::deserialize(&change_path, Some(&change_data))
        .map_err(|e| ApiError::internal(format!("Failed to parse change: {}", e)))?;
    
    // Check each dependency
    for dep_hash in change.dependencies {
        if txn.has_change(channel, &dep_hash)?.is_none() {
            missing.push(dep_hash);
        }
    }
    
    Ok(missing)
}
```

### Testing the HTTP Protocol

```bash
# Test with curl
curl -X POST http://localhost:8080/tenant/test/portfolio/main/project/demo/code/.atomic/v1/negotiate \
  -H "Content-Type: application/json" \
  -d '{"channel":"main","hashes":["HASH1","HASH2"]}'

# Should return:
# {"need":["HASH1"],"have":["HASH2"]}
```

### Timeline for HTTP Implementation

**Day 1 (4 hours):**
- Implement push negotiation endpoint
- Add dependency validation
- Complete tagup implementation

**Day 2 (2 hours):**
- Write integration tests
- Test with atomic CLI over HTTP
- Fix protocol issues

**Day 3 (2 hours):**
- Add attribution sync to HTTP protocol
- Performance testing
- Documentation updates

---

## Multi-Tenant Demo Setup

### Directory Structure

```
/tenant-data/
├── acme-corp/
│   ├── web-platform/
│   │   ├── frontend-app/.atomic/
│   │   ├── backend-api/.atomic/
│   │   └── mobile-app/.atomic/
│   └── infrastructure/
│       └── terraform/.atomic/
└── startup-inc/
    └── product/
        └── mvp/.atomic/
```

### Remote Configuration

```bash
# In your repository
atomic remote add prod-frontend "local:///tenant-data/acme-corp/web-platform/frontend-app"
atomic remote add prod-backend "local:///tenant-data/acme-corp/web-platform/backend-api"
atomic remote add prod-mobile "local:///tenant-data/acme-corp/web-platform/mobile-app"

# Push to multiple remotes
atomic push prod-frontend
atomic push prod-backend
atomic push prod-mobile
```

### REST API Access

```javascript
// Frontend code accessing via REST API
const changes = await fetch(
  'http://localhost:8080/tenant/acme-corp/portfolio/web-platform/project/frontend-app/changes?limit=50'
).then(r => r.json());

console.log(`Found ${changes.length} changes`);
changes.forEach(change => {
  console.log(`${change.id.substr(0, 12)}: ${change.message}`);
});
```

### WebSocket Integration

```javascript
// Real-time updates
const ws = new WebSocket('ws://localhost:8081');

ws.onopen = () => {
  ws.send(JSON.stringify({
    id: crypto.randomUUID(),
    timestamp: new Date().toISOString(),
    payload: {
      type: 'subscribe',
      data: {
        message_types: ['change_status_update'],
        filters: {
          repository: 'acme-corp/web-platform/frontend-app'
        }
      }
    }
  }));
};

ws.onmessage = (event) => {
  const message = JSON.parse(event.data);
  console.log('Change update:', message);
  // Update UI with new changes
};
```

---

## Troubleshooting

### Issue: "Repository not found"

**Solution:** Ensure the path structure matches:
```bash
/tenant-data/{tenant_id}/{portfolio_id}/{project_id}/.atomic/
```

### Issue: "Channel not found"

**Solution:** Initialize with default channel:
```bash
cd /tenant-data/tenant/portfolio/project
atomic init
```

### Issue: "REST API returns empty array"

**Solution:** Verify changes exist:
```bash
atomic log
ls .atomic/changes/
```

### Issue: "Push fails with dependency error"

**Solution:** Push dependencies first or use `--all`:
```bash
atomic push --all server
```

---

## Performance Tips

1. **Use filesystem remotes for speed**
   - Local protocol has zero network overhead
   - Perfect for co-located web server and API

2. **Enable caching in REST API**
   - Changes are immutable
   - Cache aggressively (24 hours+)

3. **Batch operations**
   - Use `?limit=100` to reduce API calls
   - Push multiple changes in single operation

4. **WebSocket for real-time**
   - Avoid polling REST API
   - Subscribe to repository updates

---

## Production Deployment Considerations

### When to Use Local Remotes
✅ Single server deployment
✅ Web UI and repositories co-located
✅ High performance requirements
✅ Simple setup

### When to Implement HTTP Protocol
✅ Multi-server deployment
✅ Repositories on different hosts
✅ Need true remote access
✅ Complex networking

### Hybrid Approach (Recommended)
- Use local remotes for primary storage
- Add HTTP protocol for remote access
- Best of both worlds

---

## Next Steps

1. **For This Week's Demo:**
   - Use Option 1 (Local Remotes)
   - Run the demo script
   - Show push/pull working
   - Demonstrate REST API + WebSocket

2. **For Production:**
   - Complete HTTP protocol (Option 2)
   - Add authentication/authorization
   - Set up proper deployment
   - Monitor and optimize

3. **Enhancement Opportunities:**
   - Add GraphQL API
   - Implement webhooks
   - Real-time collaboration features
   - Advanced attribution analytics

---

## Summary

**Quick Demo Setup (5 minutes):**
```bash
# Terminal 1: Start API server
atomic-api /tenant-data

# Terminal 2: Use local remotes
atomic remote add server local:///tenant-data/tenant/portfolio/project
atomic push server
atomic pull server

# Terminal 3: Test REST API
curl http://localhost:8080/tenant/tenant/portfolio/portfolio/project/project/changes
```

**This gives you:**
- ✅ Full push/pull functionality
- ✅ REST API for web UI
- ✅ WebSocket for real-time updates
- ✅ Multi-tenant support
- ✅ Ready for demo THIS WEEK

No consolidation needed. Both crates work together perfectly via filesystem! 🚀