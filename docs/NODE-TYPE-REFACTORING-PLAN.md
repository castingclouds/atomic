# Node-Type-Aware Refactoring Plan

## Executive Summary

This document outlines a comprehensive refactoring to make Atomic VCS truly node-type aware, treating changes and tags as **first-class nodes in the same DAG** rather than separate entities. This follows the core principle from TAG-SYSTEM-UNIFICATION-PLAN.md and adheres to all AGENTS.md architectural patterns.

**Core Principle**: Changes and tags are just different types of nodes in the same Directed Acyclic Graph (DAG).

## Current State Analysis

### ✅ What Works
- Database has `node_types` table with `NodeType::Change` and `NodeType::Tag`
- `register_node()` function stores node types correctly
- `get_header_by_hash()` uses node types to route correctly between changes and tags
- Tag files can be regenerated from channel state

### ❌ What's Broken
- `CS` enum has confusing `Change(Hash)` / `State(Merkle)` distinction
- Remote table doesn't store or track node types
- Clone/pull operations don't properly use node type information
- API endpoints don't leverage node type detection
- Protocol handlers treat changes and tags as fundamentally different

## Architecture Overview

### The Node-Type-Aware Model

```rust
// BEFORE: Confusing separation
enum CS {
    Change(Hash),   // A change hash
    State(Merkle),  // Actually a tag's state hash
}

// AFTER: Clean node-type model
enum Node {
    WithType { hash: Hash, node_type: NodeType },
}

// NodeType already exists in the database
pub enum NodeType {
    Change = 0,
    Tag = 1,
}
```

### Remote Table Enhancement

The remote table currently stores `(u64, (Hash, Merkle))` pairs representing position, change hash, and state. We need to enhance it to track node types:

```rust
// Current remote table structure:
// remote: UDb<L64, Pair<SerializedHash, SerializedMerkle>>
// Maps: position -> (change_hash, state_merkle)

// Enhanced structure (no schema change needed, just usage):
// We'll query node_types table when reading from remote table
// When putting to remote table, we'll also register the node type
```

## Refactoring Plan: Phase-by-Phase

### Phase 1: Core Type System Refactoring

**Goal**: Replace confusing CS enum with node-type-aware structures

#### 1.1 Update CS Enum (`atomic-remote/src/lib.rs`)

```rust
// Remove old enum
#[deprecated(note = "Use Node with NodeType instead")]
pub enum CS {
    Change(Hash),
    State(Merkle),
}

// Add new node-aware structure
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub hash: Hash,
    pub node_type: NodeType,
    pub state: Merkle, // State after applying this node
}

impl Node {
    pub fn change(hash: Hash, state: Merkle) -> Self {
        Self {
            hash,
            node_type: NodeType::Change,
            state,
        }
    }

    pub fn tag(hash: Hash, state: Merkle) -> Self {
        Self {
            hash,
            node_type: NodeType::Tag,
            state,
        }
    }

    pub fn is_change(&self) -> bool {
        self.node_type == NodeType::Change
    }

    pub fn is_tag(&self) -> bool {
        self.node_type == NodeType::Tag
    }
}
```

**Files to modify**:
- `atomic-remote/src/lib.rs` - Define new Node structure
- `libatomic/src/pristine/mod.rs` - Export NodeType publicly if needed

#### 1.2 Add Node Type Tracking to Remote Operations

Update `RemoteRepo` to track node types during all operations:

```rust
impl RemoteRepo {
    // New method: Get node with type information
    async fn get_node(
        &mut self,
        txn: &impl TxnT,
        position: u64,
    ) -> Result<Option<Node>, anyhow::Error> {
        // Get hash and state from remote table
        let (hash, state) = self.get_remote(position)?;
        
        // Query node type from database
        let node_type = txn.get_node_type(&hash)
            .ok_or_else(|| anyhow!("Node type not found for {}", hash.to_base32()))?;
        
        Ok(Some(Node {
            hash,
            node_type,
            state,
        }))
    }

    // Update existing methods to use Node
    async fn download_node(
        &mut self,
        node: &Node,
        changes_dir: &Path,
    ) -> Result<(), anyhow::Error> {
        match node.node_type {
            NodeType::Change => self.download_change(&node.hash, changes_dir).await,
            NodeType::Tag => {
                // Tags are NOT downloaded - they're regenerated
                debug!("Skipping download of tag {}, will regenerate", node.hash.to_base32());
                Ok(())
            }
        }
    }
}
```

**Files to modify**:
- `atomic-remote/src/lib.rs` - Update all RemoteRepo methods
- `atomic-remote/src/ssh.rs` - Update SSH-specific implementations
- `atomic-remote/src/http.rs` - Update HTTP-specific implementations
- `atomic-remote/src/local.rs` - Update local channel implementations

### Phase 2: Remote Table Enhancement

**Goal**: Store and retrieve node type information with remote entries

#### 2.1 Extend Remote Put Operations

```rust
impl MutTxnT for MutTxn<()> {
    fn put_remote(
        &mut self,
        remote: &mut RemoteRef<Self>,
        k: u64,
        v: (Hash, Merkle),
    ) -> Result<bool, TxnErr<Self::GraphError>> {
        // Existing logic
        let mut remote = remote.db.lock();
        let h = (&v.0).into();
        let m: SerializedMerkle = (&v.1).into();
        btree::put(&mut self.txn, &mut remote.remote, &k.into(), &Pair { a: h, b: m.clone() })?;
        btree::put(&mut self.txn, &mut remote.states, &m, &k.into())?;
        
        // NEW: Register node type
        // Query node type from node_types table and ensure it's registered
        if let Some(node_type) = self.get_node_type(&v.0) {
            debug!("Remote entry {} has node type {:?}", v.0.to_base32(), node_type);
        } else {
            warn!("Remote entry {} missing node type, defaulting to Change", v.0.to_base32());
            // This shouldn't happen in the new system, but handle gracefully
        }
        
        Ok(btree::put(&mut self.txn, &mut remote.rev, &h, &k.into())?)
    }
}
```

#### 2.2 Node Type Query Helper

Add helper to TxnT trait:

```rust
pub trait TxnT: Sized {
    // ... existing methods ...

    /// Get the node type for a given hash
    fn get_node_type(&self, hash: &Hash) -> Option<NodeType> {
        self.get_node_types(hash).ok().flatten()
    }

    /// Check if a hash represents a tag
    fn is_tag(&self, hash: &Hash) -> bool {
        self.get_node_type(hash) == Some(NodeType::Tag)
    }

    /// Check if a hash represents a change
    fn is_change(&self, hash: &Hash) -> bool {
        self.get_node_type(hash) == Some(NodeType::Change)
    }
}
```

**Files to modify**:
- `libatomic/src/pristine/mod.rs` - Add node type helpers to TxnT
- `libatomic/src/pristine/sanakirja.rs` - Update put_remote implementation

### Phase 3: Protocol Handler Unification

**Goal**: Make SSH, HTTP, and Local protocols uniformly node-type aware

#### 3.1 Unified Protocol Command Structure

Update the protocol command parsing to be node-type aware:

```rust
// In atomic/src/commands/protocol.rs

// Current approach has separate handling for changes vs tags
// NEW approach: unified node handling

async fn handle_protocol_command(
    command: &str,
    txn: &ArcTxn<MutTxn<()>>,
    changes: &ChangeStore,
    s: &mut (impl Read + Write),
) -> Result<(), anyhow::Error> {
    if let Some(cap) = APPLY.captures(command) {
        let hash = Hash::from_base32(cap[1].as_bytes())?;
        
        // Register as a change node
        txn.write().register_node(&hash, NodeType::Change)?;
        
        // ... rest of apply logic
    } else if let Some(cap) = TAGUP.captures(command) {
        let state = Merkle::from_base32(cap[1].as_bytes())?;
        let channel_name = &cap[2];
        
        // Read SHORT tag header
        let header = read_short_tag_header(s)?;
        
        // Server REGENERATES full tag file from channel state
        let tag_hash = regenerate_tag_file(txn, channel_name, &state, &header)?;
        
        // Register as a tag node
        txn.write().register_node(&tag_hash, NodeType::Tag)?;
        
        // ... rest of tagup logic
    } else if let Some(cap) = CHANGE.captures(command) {
        let hash = Hash::from_base32(cap[1].as_bytes())?;
        
        // Check node type and serve accordingly
        let node_type = txn.read().get_node_type(&hash)
            .ok_or_else(|| anyhow!("Unknown node: {}", hash.to_base32()))?;
        
        match node_type {
            NodeType::Change => serve_change_file(&hash, changes, s)?,
            NodeType::Tag => serve_tag_short(&hash, s)?,
        }
    }
    
    Ok(())
}
```

#### 3.2 Changelist with Node Types

Update changelist to include node type indicators:

```rust
// Current format:
// position.hash.state           -> change
// position.hash.state.          -> tagged change

// NEW format (backwards incompatible, but cleaner):
// position.hash.state.C         -> change node
// position.hash.state.T         -> tag node

fn format_changelist_entry(position: u64, node: &Node) -> String {
    let type_marker = match node.node_type {
        NodeType::Change => "C",
        NodeType::Tag => "T",
    };
    
    format!(
        "{}.{}.{}.{}",
        position,
        node.hash.to_base32(),
        node.state.to_base32(),
        type_marker
    )
}

fn parse_changelist_entry(line: &str) -> Result<(u64, Node), anyhow::Error> {
    let parts: Vec<&str> = line.trim_end_matches('.').split('.').collect();
    
    if parts.len() < 4 {
        bail!("Invalid changelist entry: {}", line);
    }
    
    let position = parts[0].parse()?;
    let hash = Hash::from_base32(parts[1].as_bytes())?;
    let state = Merkle::from_base32(parts[2].as_bytes())?;
    let node_type = match parts[3] {
        "C" => NodeType::Change,
        "T" => NodeType::Tag,
        _ => bail!("Invalid node type marker: {}", parts[3]),
    };
    
    Ok((position, Node { hash, node_type, state }))
}
```

**Files to modify**:
- `atomic/src/commands/protocol.rs` - Unified protocol handling
- `atomic-remote/src/lib.rs` - Update parse_line function

### Phase 4: API Endpoint Unification

**Goal**: Make HTTP API endpoints node-type aware

#### 4.1 Unified Node Endpoint

Replace separate change/tag logic with unified node handling:

```rust
// In atomic-api/src/server.rs

async fn post_atomic_protocol(
    State(state): State<AppState>,
    Path((tenant_id, portfolio_id, project_id)): Path<(String, String, String)>,
    Query(params): Query<HashMap<String, String>>,
    body: Bytes,
) -> ApiResult<Response<Body>> {
    let repo = open_repository(&state, &tenant_id, &portfolio_id, &project_id)?;
    
    // Unified "apply" endpoint that detects node type
    if let Some(apply_hash) = params.get("apply") {
        let hash = Hash::from_base32(apply_hash.as_bytes())
            .ok_or_else(|| ApiError::invalid_hash(apply_hash))?;
        
        // NEW: Detect node type from incoming data or query parameter
        let node_type = detect_node_type(&body, &params)?;
        
        match node_type {
            NodeType::Change => {
                apply_change_node(&repo, &hash, &body).await?;
            }
            NodeType::Tag => {
                apply_tag_node(&repo, &hash, &body, &params).await?;
            }
        }
        
        return Ok(success_response());
    }
    
    // Unified "download" endpoint
    if let Some(node_hash) = params.get("node") {
        let hash = Hash::from_base32(node_hash.as_bytes())
            .ok_or_else(|| ApiError::invalid_hash(node_hash))?;
        
        // Query node type from database
        let txn = repo.pristine.txn_begin()?;
        let node_type = txn.get_node_type(&hash)
            .ok_or_else(|| ApiError::node_not_found(&hash))?;
        
        match node_type {
            NodeType::Change => serve_change_file(&repo, &hash).await,
            NodeType::Tag => serve_tag_short(&repo, &hash).await,
        }
    }
    
    // Keep legacy endpoints for compatibility during transition
    if let Some(change_hash) = params.get("change") {
        warn!("Using legacy 'change' parameter, migrate to 'node'");
        // ... handle as change node
    }
    
    if let Some(tag_state) = params.get("tag") {
        warn!("Using legacy 'tag' parameter, migrate to 'node'");
        // ... handle as tag node
    }
    
    Err(ApiError::invalid_request("No valid operation specified"))
}

fn detect_node_type(
    body: &Bytes,
    params: &HashMap<String, String>,
) -> Result<NodeType, ApiError> {
    // Option 1: Explicit parameter
    if let Some(type_param) = params.get("node_type") {
        return match type_param.as_str() {
            "change" | "C" => Ok(NodeType::Change),
            "tag" | "T" => Ok(NodeType::Tag),
            _ => Err(ApiError::invalid_node_type(type_param)),
        };
    }
    
    // Option 2: Detect from content
    // Tags have SHORT format, changes have full format
    if is_short_tag_format(body) {
        Ok(NodeType::Tag)
    } else {
        Ok(NodeType::Change)
    }
}
```

#### 4.2 Node Type in Changelist API

Update changelist endpoint to include node types:

```rust
async fn get_changelist(
    State(state): State<AppState>,
    Path((tenant_id, portfolio_id, project_id)): Path<(String, String, String)>,
    Query(params): Query<ChangelistQuery>,
) -> ApiResult<Response<Body>> {
    let repo = open_repository(&state, &tenant_id, &portfolio_id, &project_id)?;
    let txn = repo.pristine.txn_begin()?;
    
    let channel = load_channel(&txn, &params.channel)?;
    
    let mut response = Vec::new();
    for (position, hash, state) in txn.log(&*channel.read(), params.from)? {
        // NEW: Query node type
        let node_type = txn.get_node_type(&hash)
            .unwrap_or(NodeType::Change); // Default to Change for safety
        
        let type_marker = match node_type {
            NodeType::Change => "C",
            NodeType::Tag => "T",
        };
        
        writeln!(
            response,
            "{}.{}.{}.{}",
            position,
            hash.to_base32(),
            state.to_base32(),
            type_marker
        )?;
    }
    
    Ok(Response::builder()
        .status(200)
        .header("content-type", "text/plain")
        .body(Body::from(response))?)
}
```

**Files to modify**:
- `atomic-api/src/server.rs` - Unified endpoint handling
- `atomic-api/src/error.rs` - Add node-type-specific errors

### Phase 5: Clone/Pull/Push Operations

**Goal**: Make all remote operations node-type aware

#### 5.1 Node-Aware Clone

```rust
impl RemoteRepo {
    pub async fn clone_channel(
        &mut self,
        repo: &Repository,
        txn: &ArcTxn<MutTxn<()>>,
        channel: &ChannelRef<MutTxn<()>>,
        lazy: bool,
    ) -> Result<(), anyhow::Error> {
        // Download changelist with node types
        let nodes = self.download_changelist_with_types(txn, channel).await?;
        
        info!("Cloning {} nodes", nodes.len());
        
        for (position, node) in nodes {
            info!(
                "Processing node {} at position {}: type={:?}",
                node.hash.to_base32(),
                position,
                node.node_type
            );
            
            match node.node_type {
                NodeType::Change => {
                    // Download and apply change
                    self.download_change(&node.hash, &repo.changes_dir).await?;
                    
                    let mut channel_guard = channel.write();
                    txn.write().apply_change_rec(
                        &repo.changes,
                        &mut channel_guard,
                        &node.hash,
                    )?;
                    
                    // Register node type
                    txn.write().register_node(&node.hash, NodeType::Change)?;
                }
                NodeType::Tag => {
                    // DO NOT download tag - regenerate it
                    info!("Regenerating tag for state {}", node.state.to_base32());
                    
                    let tag_hash = regenerate_tag_for_state(
                        txn,
                        channel,
                        &node.state,
                        &repo.changes_dir,
                    )?;
                    
                    // Register node type
                    txn.write().register_node(&tag_hash, NodeType::Tag)?;
                    
                    // Verify it matches expected hash
                    if tag_hash != node.hash {
                        warn!(
                            "Regenerated tag hash {} doesn't match expected {}",
                            tag_hash.to_base32(),
                            node.hash.to_base32()
                        );
                    }
                }
            }
            
            // Update remote tracking
            txn.write().put_remote(
                &mut remote_ref,
                position,
                (node.hash, node.state),
            )?;
        }
        
        txn.commit()?;
        
        // Output to working copy
        output_repository_no_pending(
            &repo.working_copy,
            &repo.changes,
            txn,
            channel,
            "",
            true,
            None,
            num_cpus::get(),
            0,
        )?;
        
        Ok(())
    }
    
    async fn download_changelist_with_types(
        &mut self,
        txn: &impl TxnT,
        channel: &ChannelRef<impl TxnT>,
    ) -> Result<Vec<(u64, Node)>, anyhow::Error> {
        let changelist = self.download_changelist(0).await?;
        
        let mut nodes = Vec::new();
        for line in changelist.lines() {
            let (position, node) = parse_changelist_entry(line)?;
            nodes.push((position, node));
        }
        
        Ok(nodes)
    }
}
```

#### 5.2 Node-Aware Pull

```rust
impl RemoteRepo {
    pub async fn pull(
        &mut self,
        repo: &Repository,
        txn: &ArcTxn<MutTxn<()>>,
        channel: &ChannelRef<MutTxn<()>>,
        delta: &RemoteDelta<MutTxn<()>>,
    ) -> Result<(), anyhow::Error> {
        // Get nodes to download with their types
        let to_download = delta.to_download.clone();
        
        for node in to_download {
            info!(
                "Pulling node {} (type: {:?})",
                node.hash.to_base32(),
                node.node_type
            );
            
            match node.node_type {
                NodeType::Change => {
                    // Download change if not present
                    if !change_exists(&repo.changes, &node.hash) {
                        self.download_change(&node.hash, &repo.changes_dir).await?;
                    }
                    
                    // Apply if not in channel
                    if !txn.read().has_change(channel, &node.hash)? {
                        let mut channel_guard = channel.write();
                        txn.write().apply_change_rec(
                            &repo.changes,
                            &mut channel_guard,
                            &node.hash,
                        )?;
                    }
                    
                    txn.write().register_node(&node.hash, NodeType::Change)?;
                }
                NodeType::Tag => {
                    // Regenerate tag if not present
                    let tag_path = get_tag_path(&repo.changes_dir, &node.state);
                    
                    if !tag_path.exists() {
                        info!("Regenerating missing tag for state {}", node.state.to_base32());
                        regenerate_tag_for_state(
                            txn,
                            channel,
                            &node.state,
                            &repo.changes_dir,
                        )?;
                    }
                    
                    txn.write().register_node(&node.hash, NodeType::Tag)?;
                }
            }
        }
        
        txn.commit()?;
        Ok(())
    }
}
```

**Files to modify**:
- `atomic-remote/src/lib.rs` - Update clone_channel, pull methods
- `atomic/src/commands/clone.rs` - Use new node-aware clone
- `atomic/src/commands/pull.rs` - Use new node-aware pull

### Phase 6: Tag Regeneration Helpers

**Goal**: Centralized tag regeneration logic

```rust
// In libatomic/src/tag.rs or atomic-remote/src/lib.rs

/// Regenerate a tag file from channel state
pub fn regenerate_tag_for_state(
    txn: &ArcTxn<MutTxn<()>>,
    channel: &ChannelRef<MutTxn<()>>,
    state: &Merkle,
    changes_dir: &Path,
) -> Result<Hash, anyhow::Error> {
    // Find the position in the channel with this state
    let position = txn
        .read()
        .find_position_by_state(&*channel.read(), state)?
        .ok_or_else(|| anyhow!("State {} not found in channel", state.to_base32()))?;
    
    // Create tag file path
    let tag_path = changes_dir.join("tags").join(state.to_base32());
    std::fs::create_dir_all(tag_path.parent().unwrap())?;
    
    // Generate tag file from channel state
    let temp_path = tag_path.with_extension("tmp");
    let mut writer = std::fs::File::create(&temp_path)?;
    
    // Use existing tag generation logic
    let tag_header = TagHeader {
        state: state.clone(),
        timestamp: chrono::Utc::now(),
        // ... other metadata
    };
    
    libatomic::tag::from_channel(
        &*txn.read(),
        &*channel.read(),
        &tag_header,
        &mut writer,
    )?;
    
    // Calculate hash of generated tag
    writer.flush()?;
    drop(writer);
    
    let tag_data = std::fs::read(&temp_path)?;
    let tag_hash = libatomic::Hash::from_data(&tag_data);
    
    // Move to final location
    std::fs::rename(&temp_path, &tag_path)?;
    
    info!(
        "Regenerated tag {} for state {}",
        tag_hash.to_base32(),
        state.to_base32()
    );
    
    Ok(tag_hash)
}
```

**Files to modify**:
- `libatomic/src/tag.rs` - Add regeneration helpers
- `atomic-remote/src/lib.rs` - Use regeneration helpers

## Implementation Order

1. **Week 1: Core Type System**
   - Phase 1.1: Define Node structure
   - Phase 1.2: Add node type tracking helpers
   - Write comprehensive tests for Node type

2. **Week 2: Database Layer**
   - Phase 2.1: Update remote put operations
   - Phase 2.2: Add node type query helpers
   - Test database operations with mixed node types

3. **Week 3: Protocol Handlers**
   - Phase 3.1: Unify protocol command handling
   - Phase 3.2: Update changelist format
   - Test SSH protocol with new format

4. **Week 4: API Endpoints**
   - Phase 4.1: Implement unified node endpoint
   - Phase 4.2: Update changelist API
   - Test HTTP API with node types

5. **Week 5: Remote Operations**
   - Phase 5.1: Node-aware clone
   - Phase 5.2: Node-aware pull/push
   - Integration testing

6. **Week 6: Tag Regeneration & Polish**
   - Phase 6: Centralize tag regeneration
   - End-to-end testing
   - Documentation updates

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_creation() {
        let hash = Hash::from_base32(b"ABCD").unwrap();
        let state = Merkle::from_base32(b"EFGH").unwrap();
        
        let change_node = Node::change(hash.clone(), state.clone());
        assert!(change_node.is_change());
        assert!(!change_node.is_tag());
        
        let tag_node = Node::tag(hash, state);
        assert!(tag_node.is_tag());
        assert!(!tag_node.is_change());
    }

    #[test]
    fn test_changelist_parsing() {
        let line = "42.MNYNGT2VGEQZ.STATEXXXX.C";
        let (pos, node) = parse_changelist_entry(line).unwrap();
        
        assert_eq!(pos, 42);
        assert_eq!(node.node_type, NodeType::Change);
    }

    #[test]
    fn test_node_type_persistence() {
        let repo = test_repository();
        let txn = repo.pristine.mut_txn_begin().unwrap();
        let hash = Hash::from_data(b"test");
        
        txn.register_node(&hash, NodeType::Tag).unwrap();
        txn.commit().unwrap();
        
        let txn = repo.pristine.txn_begin().unwrap();
        assert_eq!(txn.get_node_type(&hash), Some(NodeType::Tag));
    }
}
```

### Integration Tests

```rust
#[test]
fn test_clone_with_tags() {
    let source = create_repo_with_tags();
    let target = create_empty_repo();
    
    // Clone repository
    clone_repository(&source, &target).unwrap();
    
    // Verify all nodes present
    let txn = target.pristine.txn_begin().unwrap();
    
    for node in source.all_nodes() {
        let node_type = txn.get_node_type(&node.hash).unwrap();
        assert_eq!(node_type, node.node_type);
        
        if node.is_tag() {
            // Verify tag file was regenerated
            let tag_path = get_tag_path(&target.changes_dir, &node.state);
            assert!(tag_path.exists());
        }
    }
}

#[test]
fn test_pull_with_mixed_nodes() {
    let remote = create_repo_with_changes_and_tags();
    let local = create_repo_with_subset();
    
    // Pull new changes and tags
    pull_from_remote(&local, &remote).unwrap();
    
    // Verify all nodes synchronized
    let txn = local.pristine.txn_begin().unwrap();
    
    for node in remote.all_nodes() {
        assert!(txn.get_node_type(&node.hash).is_some());
    }
}
```

## Error Handling Following AGENTS.md

```rust
#[derive(Debug, Error)]
pub enum NodeError {
    #[error("Node not found: {}", hash)]
    NodeNotFound { hash: String },
    
    #[error("Invalid node type for {}: expected {:?}, got {:?}", hash, expected, actual)]
    InvalidNodeType {
        hash: String,
        expected: NodeType,
        actual: NodeType,
    },
    
    #[error("Node type not registered for {}", hash)]
    NodeTypeNotRegistered { hash: String },
    
    #[error("Failed to regenerate tag for state {}: {}", state, source)]
    TagRegenerationFailed {
        state: String,
        source: anyhow::Error,
    },
}
```

## Configuration Impact

No configuration changes required - this is purely an internal architectural refactoring.

## Migration Strategy

Since we don't care about backwards compatibility:

1. **Database Migration**: Add script to populate node_types for existing repositories
2. **Protocol Version Bump**: Increment PROTOCOL_VERSION constant
3. **Documentation**: Update all protocol documentation
4. **Deprecation Warnings**: Add warnings for old CS enum usage

```rust
// In atomic-remote/src/lib.rs
pub const PROTOCOL_VERSION: usize = 4; // Was 3

// Migration helper
pub fn migrate_repository_node_types(repo: &Repository) -> Result<(), anyhow::Error> {
    let txn = repo.pristine.mut_txn_begin()?;
    
    // Register all changes
    for change_hash in repo.changes.iter() {
        if txn.get_node_type(&change_hash).is_none() {
            txn.register_node(&change_hash, NodeType::Change)?;
        }
    }
    
    // Register all tags
    for tag_file in list_tag_files(&repo.changes_dir)? {
        let tag_hash = compute_tag_hash(&tag_file)?;
        if txn.get_node_type(&tag_hash).is_none() {
            txn.register_node(&tag_hash, NodeType::Tag)?;
        }
    }
    
    txn.commit()?;
    Ok(())