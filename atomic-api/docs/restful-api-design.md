# RESTful API Design Proposal for Atomic API

## Problem Statement

The current `.atomic` endpoint is a **protocol multiplexer** that uses query parameters to determine operations. This is not RESTful and makes the API harder to understand and use.

### Current Non-RESTful Design

```
GET  /.atomic?channel=main&id              # Get channel ID
GET  /.atomic?channel=main&state=          # Get channel state
GET  /.atomic?channel=main&changelist=0    # Get changelist
GET  /.atomic?change={hash}                # Get change data
GET  /.atomic?tag={hash}                   # Get tag data
POST /.atomic?apply={hash}                 # Apply change
POST /.atomic?tagup={hash}&channel=main    # Upload tag
```

**Issues:**
- ❌ Operations determined by query parameters, not HTTP verbs
- ❌ Single endpoint handles multiple unrelated resources
- ❌ Not intuitive for REST API consumers
- ❌ Difficult to document and discover
- ❌ Hard to version or extend
- ❌ Doesn't follow HTTP semantics properly

## Proposed RESTful Design

### Option 1: Pure REST (Breaking Change)

**Channels as First-Class Resources:**
```
GET    /channels                           # List all channels
GET    /channels/{channel}                 # Get channel info (id, state, etc.)
GET    /channels/{channel}/state           # Get channel state
GET    /channels/{channel}/changes?from=0  # Get changelist
POST   /channels/{channel}/changes/{hash}  # Apply change to channel
POST   /channels/{channel}/tags/{hash}     # Add tag to channel
```

**Changes as First-Class Resources:**
```
GET    /changes/{hash}                     # Get change data
POST   /changes                            # Upload new change
DELETE /changes/{hash}                     # Remove change (if supported)
```

**Tags as First-Class Resources:**
```
GET    /tags/{hash}                        # Get tag data
POST   /tags                               # Upload new tag
GET    /tags?channel={channel}             # List tags for channel
```

**Advantages:**
- ✅ Pure REST - resources as nouns, operations as HTTP verbs
- ✅ Clear resource hierarchy
- ✅ Easy to understand and document
- ✅ Follows HTTP semantics
- ✅ Easy to extend

**Disadvantages:**
- ❌ Breaking change for atomic CLI
- ❌ Requires updating atomic-remote client
- ❌ Migration path needed

---

### Option 2: Hybrid (Backward Compatible) ⭐ RECOMMENDED

Keep `.atomic` for protocol compatibility, add RESTful routes for modern clients.

**Legacy Protocol (for atomic CLI):**
```
GET  /.atomic?channel=main&id              # [LEGACY]
GET  /.atomic?channel=main&state=          # [LEGACY]
GET  /.atomic?channel=main&changelist=0    # [LEGACY]
GET  /.atomic?change={hash}                # [LEGACY]
GET  /.atomic?tag={hash}                   # [LEGACY]
POST /.atomic?apply={hash}                 # [LEGACY]
POST /.atomic?tagup={hash}&channel=main    # [LEGACY]
```

**New RESTful API (for web UI and new clients):**
```
# Channels
GET    /api/channels                           # List channels
GET    /api/channels/{channel}                 # Get channel info
GET    /api/channels/{channel}/state           # Get state
GET    /api/channels/{channel}/changes         # List changes
POST   /api/channels/{channel}/apply/{hash}    # Apply change

# Changes
GET    /api/changes/{hash}                     # Get change data
GET    /api/changes/{hash}/diff                # Get change diff
GET    /api/changes/{hash}/dependencies        # Get dependencies
POST   /api/changes/{hash}/validate            # Validate dependencies

# Tags  
GET    /api/tags/{hash}                        # Get tag data
POST   /api/channels/{channel}/tags/{hash}     # Add tag to channel
GET    /api/channels/{channel}/tags            # List channel tags
```

**Full Path Structure with Multi-Tenancy:**
```
# Legacy protocol
/{tenant}/{portfolio}/{project}/.atomic?params

# RESTful API
/{tenant}/{portfolio}/{project}/api/channels/{channel}
/{tenant}/{portfolio}/{project}/api/changes/{hash}
/{tenant}/{portfolio}/{project}/api/tags/{hash}
```

**Advantages:**
- ✅ Backward compatible with atomic CLI
- ✅ Clean RESTful API for new clients
- ✅ Gradual migration path
- ✅ Both APIs can coexist
- ✅ No breaking changes

**Disadvantages:**
- ⚠️ Two APIs to maintain (temporarily)
- ⚠️ More routes in codebase

**Migration Strategy:**
1. Add new RESTful routes alongside `.atomic`
2. Update documentation to prefer new routes
3. Deprecate `.atomic` in 2.0 (but keep working)
4. Remove `.atomic` in 3.0 (if needed)

---

### Option 3: Namespaced Protocol

Keep protocol semantics but organize better with namespaces:

```
GET    /protocol/channels/{channel}/info
GET    /protocol/channels/{channel}/state
GET    /protocol/channels/{channel}/changes?from=0
POST   /protocol/channels/{channel}/apply/{hash}
POST   /protocol/channels/{channel}/tags/{hash}
GET    /protocol/changes/{hash}
GET    /protocol/tags/{hash}
```

**Advantages:**
- ✅ More RESTful than current `.atomic`
- ✅ Clear protocol namespace
- ✅ Easier to version (`/protocol/v1/`, `/protocol/v2/`)

**Disadvantages:**
- ❌ Still requires atomic CLI changes
- ⚠️ Not as clean as pure REST

---

## Recommended Approach: Option 2 (Hybrid)

### Implementation Plan

#### Phase 1: Add RESTful Routes (No Breaking Changes)

**New Routes to Add:**
```rust
// In src/server.rs serve() method:

// Channels API
.route("/api/channels", get(list_channels))
.route("/api/channels/{channel}", get(get_channel_info))
.route("/api/channels/{channel}/state", get(get_channel_state))
.route("/api/channels/{channel}/changes", get(get_channel_changes))
.route("/api/channels/{channel}/apply/{hash}", post(apply_change_to_channel))
.route("/api/channels/{channel}/tags", get(list_channel_tags))
.route("/api/channels/{channel}/tags/{hash}", post(add_tag_to_channel))

// Changes API
.route("/api/changes/{hash}", get(get_change_data))
.route("/api/changes/{hash}/diff", get(get_change_diff))
.route("/api/changes/{hash}/dependencies", get(get_change_dependencies))
.route("/api/changes/{hash}/validate", post(validate_change))

// Tags API
.route("/api/tags/{hash}", get(get_tag_data))
```

**Keep Existing:**
```rust
// Legacy protocol route (for atomic CLI)
.route("/.atomic", get(get_atomic_protocol).post(post_atomic_protocol))
```

#### Phase 2: Update Documentation

- Document new RESTful API as primary
- Mark `.atomic` as legacy but supported
- Provide migration guide

#### Phase 3: (Optional) Deprecate Legacy

- Add deprecation warnings to `.atomic` responses
- Set timeline for removal (2.0 or 3.0)

---

## Detailed RESTful API Specification

### Channels Resource

#### List Channels
```http
GET /api/channels
```

**Response:**
```json
{
  "channels": [
    {
      "name": "main",
      "id": "CHANNEL_ID_HASH",
      "state": "STATE_MERKLE_HASH",
      "change_count": 42
    }
  ]
}
```

#### Get Channel Info
```http
GET /api/channels/{channel}
```

**Response:**
```json
{
  "name": "main",
  "id": "CHANNEL_ID_HASH",
  "state": "STATE_MERKLE_HASH",
  "change_count": 42,
  "created_at": "2025-01-15T00:00:00Z",
  "last_modified": "2025-01-15T12:00:00Z"
}
```

#### Get Channel State
```http
GET /api/channels/{channel}/state
```

**Response:**
```json
{
  "state": "STATE_MERKLE_HASH",
  "change_number": 42
}
```

#### Get Channel Changes
```http
GET /api/channels/{channel}/changes?from=0&limit=50
```

**Response:**
```json
{
  "changes": [
    {
      "number": 1,
      "hash": "CHANGE_HASH",
      "merkle": "MERKLE_HASH",
      "is_tag": false
    }
  ],
  "next_offset": 50,
  "has_more": true
}
```

#### Apply Change to Channel
```http
POST /api/channels/{channel}/apply/{hash}
Content-Type: application/octet-stream

[binary change data]
```

**Response:**
```json
{
  "success": true,
  "message": "Change applied successfully",
  "change_hash": "HASH"
}
```

#### Add Tag to Channel
```http
POST /api/channels/{channel}/tags/{hash}
Content-Type: application/octet-stream

[binary tag data]
```

**Response:**
```json
{
  "success": true,
  "message": "Tag added successfully",
  "tag_hash": "HASH"
}
```

---

### Changes Resource

#### Get Change Data
```http
GET /api/changes/{hash}
```

**Response:**
```
Content-Type: application/octet-stream

[binary change data]
```

#### Get Change Diff
```http
GET /api/changes/{hash}/diff
```

**Response:**
```json
{
  "hash": "HASH",
  "message": "Commit message",
  "author": "Author <email>",
  "timestamp": "2025-01-15T00:00:00Z",
  "diff": "... diff content ..."
}
```

#### Get Change Dependencies
```http
GET /api/changes/{hash}/dependencies
```

**Response:**
```json
{
  "hash": "HASH",
  "dependencies": [
    "DEP_HASH_1",
    "DEP_HASH_2"
  ],
  "dependency_count": 2
}
```

#### Validate Change
```http
POST /api/changes/{hash}/validate?channel=main
```

**Response:**
```json
{
  "valid": true,
  "missing_dependencies": [],
  "message": "All dependencies satisfied"
}
```

Or if invalid:
```json
{
  "valid": false,
  "missing_dependencies": [
    "MISSING_HASH_1",
    "MISSING_HASH_2"
  ],
  "message": "Missing 2 dependencies"
}
```

---

### Tags Resource

#### Get Tag Data
```http
GET /api/tags/{hash}
```

**Response:**
```
Content-Type: application/octet-stream

[binary tag data]
```

---

## HTTP Status Codes

Following REST best practices:

- `200 OK` - Successful GET/POST
- `201 Created` - Resource created successfully
- `204 No Content` - Successful DELETE
- `400 Bad Request` - Invalid input
- `404 Not Found` - Resource doesn't exist
- `409 Conflict` - Dependency conflict
- `422 Unprocessable Entity` - Valid format but can't process (e.g., missing dependencies)
- `500 Internal Server Error` - Server error

---

## Benefits of This Approach

### For Web UI Developers
- Clear, intuitive API
- Standard REST patterns
- Easy to discover and document
- Consistent error handling
- Proper HTTP semantics

### For Atomic CLI
- No breaking changes
- Continues to work with `.atomic` endpoint
- Can migrate when ready

### For API Server Maintainers
- Two codepaths initially, but clean separation
- Can deprecate legacy gradually
- Modern clients use better API
- Easier to extend and version

---

## Implementation Checklist

### Phase 1: Foundation
- [ ] Create new handler functions for RESTful endpoints
- [ ] Extract common logic from `.atomic` handlers
- [ ] Add routing for new endpoints
- [ ] Write unit tests for new handlers

### Phase 2: Documentation
- [ ] Update README with new API
- [ ] Create OpenAPI/Swagger spec
- [ ] Add examples for common operations
- [ ] Document migration path

### Phase 3: Testing
- [ ] Integration tests for RESTful API
- [ ] Verify `.atomic` still works
- [ ] Performance testing both APIs
- [ ] Load testing

---

## Decision Required

**Question for Team:** Should we:

1. ✅ **Implement Option 2 (Hybrid)** - Add RESTful API alongside `.atomic`
2. ⏸️ **Wait and do Option 1 (Pure REST)** - Major version bump with breaking changes
3. ❌ **Keep current design** - Stay with `.atomic` query parameter multiplexer

**Recommendation:** Option 2 (Hybrid) gives us the best of both worlds with no breaking changes.

---

## Next Steps

If approved:
1. **This Week**: Implement Phase 1 - Add RESTful routes
2. **Next Week**: Update documentation
3. **Following Week**: Integration testing

Estimated effort: 4-6 hours for Phase 1 implementation.