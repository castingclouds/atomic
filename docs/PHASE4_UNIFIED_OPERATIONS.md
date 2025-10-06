# Phase 4: Unified Upload/Download Logic - BREAKING CHANGES EDITION

## Overview

Phase 4 completes the node-type unification by replacing the deprecated `CS` enum with `Node` throughout the remote operations codebase. **Breaking changes are acceptable** - we're doing a clean migration.

## Goals

1. **Remove `CS` enum entirely** - Replace with `Node` everywhere
2. **Unified operations** - Single `upload_nodes()` and `download_nodes()` methods
3. **Simplified logic** - Treat changes and tags uniformly
4. **Clean codebase** - No backward compatibility layers

## Status: 🚧 IN PROGRESS

## Architecture Changes

### Before (Phase 3)
```rust
// Using deprecated CS enum
pub enum CS {
    Change(Hash),
    State(Merkle),
}

pub async fn upload_changes(&mut self, changes: &[CS]) -> Result<()>;
pub async fn download_changes(&mut self, hashes: &mut Receiver<CS>) -> Result<()>;
```

### After (Phase 4)
```rust
// Using Node with NodeType
pub struct Node {
    pub hash: Hash,
    pub node_type: NodeType,
    pub state: Merkle,
}

pub async fn upload_nodes(&mut self, nodes: &[Node]) -> Result<()>;
pub async fn download_nodes(&mut self, nodes: &mut Receiver<Node>) -> Result<()>;
```

## Implementation Plan

### Step 1: Update Core Types in `atomic-remote/src/lib.rs`

#### A. Remove CS Enum
```rust
// DELETE THIS:
#[deprecated(note = "Use Node with NodeType instead")]
pub enum CS {
    Change(Hash),
    State(Merkle),
}

// CS is now completely removed from codebase
```

#### B. Update Data Structures
```rust
// BEFORE
pub struct PushDelta {
    pub to_upload: Vec<CS>,
    pub remote_unrecs: Vec<(u64, CS)>,
    pub unknown_changes: Vec<CS>,
}

// AFTER
pub struct PushDelta {
    pub to_upload: Vec<Node>,
    pub remote_unrecs: Vec<(u64, Node)>,
    pub unknown_changes: Vec<Node>,
}

// BEFORE
pub struct RemoteDelta<T: MutTxnTExt + TxnTExt> {
    pub to_download: Vec<CS>,
    pub ours_ge_dichotomy_set: HashSet<CS>,
    pub theirs_ge_dichotomy_set: HashSet<CS>,
    pub remote_unrecs: Vec<(u64, CS)>,
    // ...
}

// AFTER
pub struct RemoteDelta<T: MutTxnTExt + TxnTExt> {
    pub to_download: Vec<Node>,
    pub ours_ge_dichotomy_set: HashSet<Node>,
    pub theirs_ge_dichotomy_set: HashSet<Node>,
    pub remote_unrecs: Vec<(u64, Node)>,
    // ...
}
```

#### C. Update RemoteRepo Methods
```rust
impl RemoteRepo {
    // BEFORE
    pub async fn upload_changes<T: MutTxnTExt + 'static>(
        &mut self,
        txn: &mut T,
        local: PathBuf,
        to_channel: Option<&str>,
        changes: &[CS],
    ) -> Result<(), anyhow::Error>

    // AFTER
    pub async fn upload_nodes<T: MutTxnTExt + 'static>(
        &mut self,
        txn: &mut T,
        local: PathBuf,
        to_channel: Option<&str>,
        nodes: &[Node],
    ) -> Result<(), anyhow::Error>

    // BEFORE
    pub async fn download_changes(
        &mut self,
        progress_bar: ProgressBar,
        hashes: &mut tokio::sync::mpsc::UnboundedReceiver<CS>,
        send: &mut tokio::sync::mpsc::Sender<(CS, bool)>,
        path: &mut PathBuf,
        full: bool,
    ) -> Result<bool, anyhow::Error>

    // AFTER
    pub async fn download_nodes(
        &mut self,
        progress_bar: ProgressBar,
        nodes: &mut tokio::sync::mpsc::UnboundedReceiver<Node>,
        send: &mut tokio::sync::mpsc::Sender<(Node, bool)>,
        path: &mut PathBuf,
        full: bool,
    ) -> Result<bool, anyhow::Error>
}
```

### Step 2: Update Local Remote Client (`atomic-remote/src/local.rs`)

#### A. Update upload_changes to upload_nodes
```rust
// BEFORE
impl Local {
    pub fn upload_changes(
        &mut self,
        progress_bar: ProgressBar,
        mut local: PathBuf,
        to_channel: Option<&str>,
        changes: &[CS],
    ) -> Result<(), anyhow::Error> {
        for c in changes {
            match c {
                CS::Change(c) => {
                    libatomic::changestore::filesystem::push_filename(&mut local, &c);
                    // ...
                }
                CS::State(c) => {
                    libatomic::changestore::filesystem::push_tag_filename(&mut local, &c);
                    // ...
                }
            }
        }
    }
}

// AFTER
impl Local {
    pub fn upload_nodes(
        &mut self,
        progress_bar: ProgressBar,
        mut local: PathBuf,
        to_channel: Option<&str>,
        nodes: &[Node],
    ) -> Result<(), anyhow::Error> {
        for node in nodes {
            match node.node_type {
                NodeType::Change => {
                    libatomic::changestore::filesystem::push_filename(&mut local, &node.hash);
                    // ...
                }
                NodeType::Tag => {
                    libatomic::changestore::filesystem::push_tag_filename(&mut local, &node.state);
                    // ...
                }
            }
        }
    }
}
```

#### B. Update standalone upload_changes function
```rust
// BEFORE
pub fn upload_changes<T: MutTxnTExt + 'static, C: libatomic::changestore::ChangeStore>(
    progress_bar: ProgressBar,
    store: &C,
    txn: &mut T,
    channel: &libatomic::pristine::ChannelRef<T>,
    changes: &[CS],
) -> Result<(), anyhow::Error> {
    for c in changes {
        match c {
            CS::Change(c) => {
                txn.apply_change_ws(store, &mut *channel, c, &mut ws)?;
            }
            CS::State(c) => {
                if let Some(n) = txn.channel_has_state(txn.states(&*channel), &c.into())? {
                    let tags = txn.tags_mut(&mut *channel);
                    txn.put_tags(tags, n.into(), c)?;
                }
            }
        }
    }
}

// AFTER
pub fn upload_nodes<T: MutTxnTExt + 'static, C: libatomic::changestore::ChangeStore>(
    progress_bar: ProgressBar,
    store: &C,
    txn: &mut T,
    channel: &libatomic::pristine::ChannelRef<T>,
    nodes: &[Node],
) -> Result<(), anyhow::Error> {
    for node in nodes {
        match node.node_type {
            NodeType::Change => {
                txn.apply_change_ws(store, &mut *channel, &node.hash, &mut ws)?;
            }
            NodeType::Tag => {
                if let Some(n) = txn.channel_has_state(txn.states(&*channel), &node.state)? {
                    let tags = txn.tags_mut(&mut *channel);
                    txn.put_tags(tags, n.into(), &node.state)?;
                }
            }
        }
    }
}
```

#### C. Update download_changes to download_nodes
```rust
// BEFORE
pub async fn download_changes(
    &mut self,
    progress_bar: ProgressBar,
    hashes: &mut tokio::sync::mpsc::UnboundedReceiver<CS>,
    send: &mut tokio::sync::mpsc::Sender<(CS, bool)>,
    mut path: &mut PathBuf,
) -> Result<(), anyhow::Error> {
    while let Some(c) = hashes.recv().await {
        match c {
            CS::Change(c) => {
                libatomic::changestore::filesystem::push_filename(&mut self.changes_dir, &c);
            }
            CS::State(c) => {
                libatomic::changestore::filesystem::push_tag_filename(&mut self.changes_dir, &c);
            }
        }
    }
}

// AFTER
pub async fn download_nodes(
    &mut self,
    progress_bar: ProgressBar,
    nodes: &mut tokio::sync::mpsc::UnboundedReceiver<Node>,
    send: &mut tokio::sync::mpsc::Sender<(Node, bool)>,
    mut path: &mut PathBuf,
) -> Result<(), anyhow::Error> {
    while let Some(node) = nodes.recv().await {
        match node.node_type {
            NodeType::Change => {
                libatomic::changestore::filesystem::push_filename(&mut self.changes_dir, &node.hash);
            }
            NodeType::Tag => {
                libatomic::changestore::filesystem::push_tag_filename(&mut self.changes_dir, &node.state);
            }
        }
    }
}
```

### Step 3: Update SSH Remote Client (`atomic-remote/src/ssh.rs`)

Apply same transformations:
- `upload_changes()` → `upload_nodes()`
- `download_changes()` → `download_nodes()`
- Replace `CS` matching with `NodeType` matching

### Step 4: Update HTTP Remote Client (`atomic-remote/src/http.rs`)

Apply same transformations:
- `upload_changes()` → `upload_nodes()`
- `download_changes()` → `download_nodes()`
- Replace `CS` matching with `NodeType` matching

### Step 5: Update RemoteDelta Logic

All the push/pull delta calculation logic needs updating:

```rust
// BEFORE
for x in txn.reverse_log(&*channel.read(), None)? {
    let (_, (h, _)) = x?;
    // ...
    to_upload.push(CS::Change(h.into()));
    if tags.contains(&state) {
        to_upload.push(CS::State(state));
    }
}

// AFTER
for x in txn.reverse_log(&*channel.read(), None)? {
    let (_, (h, m)) = x?;
    let state: Merkle = m.into();
    
    // Always create Node with proper type
    let node = Node::change(h.into(), state.clone());
    to_upload.push(node);
    
    if tags.contains(&state) {
        // Create tag node - hash is the merkle for tags
        let tag_node = Node::tag(Hash::from(&state), state);
        to_upload.push(tag_node);
    }
}
```

### Step 6: Update Call Sites

Find all places that call the old methods and update them:

```bash
# Search for calls to update
grep -r "upload_changes" atomic/
grep -r "download_changes" atomic/
grep -r "CS::" atomic/
```

Update each call site to use `Node` instead of `CS`.

### Step 7: Update Tests

- Phase 2 tests: May need updating if they use `CS`
- Phase 3 tests: Should continue to work (they test node types)
- Create Phase 4 tests for unified operations

## Benefits

1. **Cleaner Code**: Single code path for changes and tags
2. **Type Safety**: `Node` carries all necessary information
3. **Consistency**: Same abstraction everywhere
4. **Simpler Logic**: No more match statements on `CS` variants
5. **Better Performance**: Fewer allocations, more direct operations

## Migration Checklist

- [ ] Step 1: Update `atomic-remote/src/lib.rs` core types
  - [ ] Remove `CS` enum
  - [ ] Update `PushDelta` struct
  - [ ] Update `RemoteDelta` struct
  - [ ] Update `RemoteRepo` methods
- [ ] Step 2: Update `atomic-remote/src/local.rs`
  - [ ] Rename `upload_changes()` to `upload_nodes()`
  - [ ] Rename standalone `upload_changes()` to `upload_nodes()`
  - [ ] Rename `download_changes()` to `download_nodes()`
  - [ ] Update all `CS` matching to `NodeType` matching
- [ ] Step 3: Update `atomic-remote/src/ssh.rs`
  - [ ] Apply same transformations
- [ ] Step 4: Update `atomic-remote/src/http.rs`
  - [ ] Apply same transformations
- [ ] Step 5: Update all call sites
  - [ ] Find with grep
  - [ ] Update each call
- [ ] Step 6: Update tests
  - [ ] Fix any broken tests
  - [ ] Add Phase 4 integration tests
- [ ] Step 7: Update documentation
  - [ ] Update REMOTE_API_INTEGRATION.md
  - [ ] Add migration notes

## Testing Strategy

### Unit Tests
- Test `Node` construction and conversion
- Test upload/download with both change and tag nodes
- Test mixed node type operations

### Integration Tests
- Full push/pull workflow with mixed nodes
- Remote sync with tags
- Error handling for invalid nodes

### System Tests
- Run existing shell scripts to verify compatibility
- Test remote operations end-to-end

## Success Criteria

- [ ] `CS` enum completely removed from codebase
- [ ] All `upload_changes()` renamed to `upload_nodes()`
- [ ] All `download_changes()` renamed to `download_nodes()`
- [ ] All code uses `Node` instead of `CS`
- [ ] All tests passing
- [ ] No deprecation warnings
- [ ] Clean grep results for `CS::`

## Timeline

Phase 4 is a **breaking change** migration - all changes will be made at once:
1. Update core types and methods (1-2 hours)
2. Update all call sites (1-2 hours)
3. Fix tests (30 min - 1 hour)
4. Validation and cleanup (30 min)

**Total estimate**: 3-6 hours of focused work

## Next Phase Preview

**Phase 5: Complete DAG Unification**
- Unified channel operations for all node types
- Single apply operation for changes and tags
- Complete removal of change/tag distinction in core operations
- Pure graph-based operations

---

**Phase 4 Status**: 🚧 READY TO START - Breaking changes edition!