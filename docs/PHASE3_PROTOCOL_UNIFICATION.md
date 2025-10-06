# Phase 3: Protocol Handler Unification

## Overview

Phase 3 makes protocol handlers (SSH and HTTP) fully node-type aware, using the infrastructure built in Phases 1 and 2. This enables consistent handling of changes and tags across all remote operations.

## Goals

1. **Unified Node Download**: Protocol handlers query node types from the database
2. **Node-Type Registration**: All remote operations register node types in the remote table
3. **Consistent Protocol Behavior**: SSH and HTTP protocols handle changes and tags uniformly
4. **Non-Breaking Changes**: Existing clients continue to work

## Architecture

Following AGENTS.md principles:

- **Database-Centric**: Node types stored in database after remote operations
- **Client-Side Registration**: Remote clients (ssh.rs, http.rs, local.rs) register nodes
- **Factory Pattern**: Use `Node::change()` and `Node::tag()` constructors
- **DRY Principles**: Reuse Phase 2 helper methods (`get_remote_node()`, `is_remote_tag()`)
- **Error Handling**: Graceful degradation when node types unavailable

## Architecture Clarification

### Server vs Client Distinction

**Protocol Handlers (Server Side)**:
- `atomic/atomic/src/commands/protocol.rs` - SSH server
- `atomic-api/src/server.rs` - HTTP API server
- These ARE the remote - they don't have RemoteRef
- Already node-type aware for changelist output
- No remote registration needed on server side

**Remote Clients (Client Side)**:
- `atomic-remote/src/ssh.rs` - SSH client
- `atomic-remote/src/http.rs` - HTTP client
- `atomic-remote/src/local.rs` - Local client
- These call remote operations and MUST register results
- Phase 3 focuses on enhancing these clients

## Implementation Plan

### 1. Local Remote Client Enhancement

**File**: `atomic-remote/src/local.rs`

#### Changes Needed

##### A. `upload_changes()` Function
- Currently applies changes/tags to channel
- Enhancement: Register each node in remote table with correct type
- Call `txn.put_remote(remote, position, hash, node_type)` after apply

##### B. `download_changes()` Method
- Currently downloads and writes changes to filesystem
- Enhancement: Register downloaded nodes in remote table
- Track node types during download

### 2. SSH Remote Client Enhancement

**File**: `atomic-remote/src/ssh.rs`

#### Changes Needed

##### A. `upload_changes()` Method
- Currently sends changes/tags to remote server
- Enhancement: Register each uploaded node in local remote table
- Enables tracking what's been pushed to remote

##### B. `download_changes()` Method
- Currently downloads changes from remote
- Enhancement: Register downloaded nodes with types
- Use `is_tag` flag from changelist to set correct type

### 3. HTTP Remote Client Enhancement

**File**: `atomic-remote/src/http.rs`

#### Changes Needed

##### A. `upload_changes()` Method
- Currently POSTs changes/tags to HTTP API
- Enhancement: Register uploaded nodes in remote table
- Track what's been pushed

##### B. `download_changes()` Method
- Currently GETs changes from HTTP API
- Enhancement: Register downloaded nodes
- Query node types from changelist

### 4. Remote Table Integration Pattern

Remote clients need to register nodes after successful operations:

```rust
// After applying a change
let state = txn.current_state(&*channel.read())?;
let node = Node::change(change_hash, state);
txn.put_remote(remote, position, &node.hash, node.node_type)?;
```

```rust
// After uploading a tag
let state = tag_merkle;
let node = Node::tag(tag_hash, state);
txn.put_remote(remote, position, &node.hash, node.node_type)?;
```

## Benefits

1. **Complete Node Tracking**: All remote operations tracked with node types
2. **Sync Correctness**: Remote table accurately reflects what's on remote
3. **Query Capabilities**: Can ask "what node type is at remote position X?"
4. **Foundation for Phase 4**: Enables unified upload/download logic
5. **Client-Side Tracking**: Clients know what they've pushed/pulled and the types

## Testing Strategy

### Unit Tests

1. Test `put_remote()` called with correct node types
2. Test `get_remote_node()` returns correct node after registration
3. Test `is_remote_tag()` accurately identifies tags

### Integration Tests

**File**: `atomic-remote/tests/phase3_client_registration.rs`

```rust
#[test]
fn test_local_upload_registers_changes() {
    // Upload change via Local client
    // Verify put_remote() called with NodeType::Change
}

#[test]
fn test_local_upload_registers_tags() {
    // Upload tag via Local client
    // Verify put_remote() called with NodeType::Tag
}

#[test]
fn test_ssh_upload_registers_nodes() {
    // Upload changes and tags via SSH client
    // Verify remote table has correct node types
}

#[test]
fn test_http_upload_registers_nodes() {
    // Upload changes and tags via HTTP client
    // Verify remote table has correct node types
}

#[test]
fn test_remote_node_query_after_push() {
    // Push changes and tags to remote
    // Query node types from remote table
    // Verify correct types returned
}

#[test]
fn test_download_registers_node_types() {
    // Download changes and tags from remote
    // Verify remote table tracks node types correctly
}
```

### System Tests

Use existing shell scripts:
- `test-tag-push-pull.sh` - Verify tag operations register correctly
- `phase3_remote_demo.sh` - Verify full sync workflow

## Implementation Steps

### Step 1: Enhance Local Remote Client

1. Update `upload_changes()` to register nodes in remote table
2. Update `download_changes()` to track node types
3. Add helper methods for node registration
4. Test with local-to-local operations

### Step 2: Enhance SSH Remote Client

1. Update `upload_changes()` to call `put_remote()` after successful upload
2. Update `download_changes()` to register downloaded nodes
3. Track node types from changelist `is_tag` flags
4. Test with SSH push/pull operations

### Step 3: Enhance HTTP Remote Client

1. Update `upload_changes()` to register uploaded nodes
2. Update `download_changes()` to track node types
3. Query node types from HTTP changelist endpoint
4. Test with HTTP API push/pull operations

### Step 4: Create Integration Tests

1. Create `phase3_client_registration.rs` test file
2. Implement Local client tests
3. Implement SSH client tests
4. Implement HTTP client tests
5. Run and validate all tests

### Step 5: Update Documentation

1. Update REMOTE_API_INTEGRATION.md with Phase 3 changes
2. Add remote client registration examples
3. Document node type tracking patterns

## Migration Strategy

**This phase is fully backward compatible**:

- Existing protocol clients work unchanged
- Node type registration is additive
- No breaking changes to protocol format
- Graceful handling of missing node types

## Success Criteria

- [ ] Local client registers changes in remote table during upload
- [ ] Local client registers tags in remote table during upload
- [ ] SSH client registers nodes after successful push
- [ ] HTTP client registers nodes after successful push
- [ ] Download operations track node types correctly
- [ ] `get_remote_node()` returns correct types after operations
- [ ] `is_remote_tag()` accurately identifies tags
- [ ] All integration tests pass
- [ ] No breaking changes to existing functionality

## Next Phase Preview

**Phase 4: Unified Upload/Download Logic**
- Single `upload_node()` method for changes and tags
- Single `download_node()` method using node types
- Deprecate separate change/tag methods
- Complete DAG unification

## References

- Phase 1: Core Type System Refactoring
- Phase 2: Remote Table Enhancement
- AGENTS.md: Architecture principles
- HTTP-API-PROTOCOL-COMPARISON.md: Protocol alignment patterns