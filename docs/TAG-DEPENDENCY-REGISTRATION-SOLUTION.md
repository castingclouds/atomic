# Tag Dependency Registration Solution

## Problem Statement

When changes with tag dependencies are pushed to a server, the dependency resolution fails during subsequent operations (e.g., `atomic log`) because **tags do not have internal ChangeId mappings** in the database, while the system assumes all dependencies are regular changes.

### Error Symptom

```
Error: change 2FQUWZKQYP5P3NGFXC4G4JRFQ36DQPKZC3ZQWGEZIQOLCIXHP2YQC (hash OZCK6ZN5BXYCW...) not found in channel
```

The system tries to load headers for tag dependencies as if they were regular changes, which fails because tags are stored differently.

## Root Cause Analysis

### 1. Database Architecture Issue

The `register_change` function in `libatomic/src/pristine/mod.rs` handles change registration:

```rust
pub(crate) fn register_change<T>(
    txn: &mut T,
    internal: &ChangeId,
    hash: &Hash,
    change: &Change,
) -> Result<(), TxnErr<T::GraphError>> {
    debug!("registering change {:?}", hash);
    let shash = hash.into();
    txn.put_external(internal, &shash)?;  // Maps ChangeId -> Hash
    txn.put_internal(&shash, internal)?;  // Maps Hash -> ChangeId
    
    for dep in change.dependencies.iter() {
        debug!("dep = {:?}", dep);
        
        // Check if dependency has an internal ID (regular change) or is a tag
        // Tags are valid dependencies but don't have internal IDs
        if let Some(dep_internal_ref) = txn.get_internal(&dep.into())? {
            let dep_internal = *dep_internal_ref;
            debug!("{:?} depends on {:?}", internal, dep_internal);
            txn.put_revdep(&dep_internal, internal)?;
            txn.put_dep(internal, &dep_internal)?;
        } else {
            // Dependency doesn't have an internal ID - might be a tag
            // Tags don't need revdep/dep entries since they're not in the changes graph
            debug!(
                "{:?} has dependency {:?} without internal ID (likely a tag, skipping dep graph)",
                internal, dep
            );
        }
    }
    // ... rest of registration
}
```

**Key Issue**: Tags are explicitly skipped in the dependency graph because they don't have internal IDs. This is correct behavior for the dependency graph, but creates problems downstream.

### 2. Change Header Loading Issue

The `get_header` function in `libatomic/src/changestore/mod.rs` defaults to treating all hashes as regular changes:

```rust
fn get_header(&self, h: &Hash) -> Result<ChangeHeader, Self::Error> {
    Ok(self.get_change(h)?.hashed.header)  // ❌ Fails for tags!
}
```

However, a separate `get_tag_header` function exists but isn't used:

```rust
fn get_tag_header(&self, h: &crate::Merkle) -> Result<ChangeHeader, Self::Error>;
```

### 3. Hash Type Confusion

After the hash unification work (BLAKE3 → Ed25519/Merkle), the type system no longer distinguishes between:
- Regular change hashes (Ed25519)
- Tag hashes (Merkle)
- Dependencies that could be either

All are now stored as generic `Hash` types, making runtime detection necessary.

## Current vs Expected Behavior

### Current Behavior (Broken)

1. Client creates change with tag dependency
2. Client pushes change to server
3. Server calls `register_change()`
4. Tag dependency is found not to have internal ID
5. Tag dependency is skipped (no `put_dep`/`put_revdep`)
6. Later, `atomic log` calls `get_header(&tag_hash)`
7. `get_header()` calls `get_change(&tag_hash)` 
8. ❌ Fails: tag file not found (tries to open `.atomic/changes/TAG_HASH.change` instead of `.atomic/tags/TAG_HASH.tag`)

### Expected Behavior (Fixed)

1. Client creates change with tag dependency
2. Client pushes change to server
3. Server calls `register_change()`
4. Tag dependency is recognized as tag
5. Tag is registered separately (new mechanism)
6. Later, `atomic log` calls `get_header(&tag_hash)`
7. System detects hash is a tag
8. ✅ Calls `get_tag_header(&tag_hash)` instead
9. Returns header from tag file

## Proposed Solutions

### Solution 1: Smart Header Loading (Recommended)

**Approach**: Modify `get_header()` to automatically detect whether a hash is a change or tag, and route to the appropriate function.

**Pros**:
- Non-breaking change
- Minimal code modifications
- Follows existing architecture patterns
- Type-safe with proper error handling

**Cons**:
- Small performance overhead (filesystem check)
- Doesn't address lack of internal IDs for tags

**Implementation**:

```rust
// libatomic/src/changestore/mod.rs
fn get_header(&self, h: &Hash) -> Result<ChangeHeader, Self::Error> {
    // First, try to load as a regular change
    match self.get_change(h) {
        Ok(change) => Ok(change.hashed.header),
        Err(_) => {
            // If that fails, try as a tag
            // Tags use Merkle hashes, attempt conversion
            let merkle = crate::Merkle::from_bytes(&h.to_bytes());
            self.get_tag_header(&merkle)
        }
    }
}
```

**Better implementation with explicit detection**:

```rust
// libatomic/src/changestore/mod.rs
fn get_header(&self, h: &Hash) -> Result<ChangeHeader, Self::Error> {
    // Check if this is a tag by attempting to detect from filesystem
    if self.is_tag(h)? {
        let merkle = crate::Merkle::from_bytes(&h.to_bytes());
        self.get_tag_header(&merkle)
    } else {
        Ok(self.get_change(h)?.hashed.header)
    }
}

fn is_tag(&self, h: &Hash) -> Result<bool, Self::Error>;
```

For filesystem implementation:

```rust
// libatomic/src/changestore/filesystem.rs
fn is_tag(&self, h: &Hash) -> Result<bool, Self::Error> {
    let merkle = crate::Merkle::from_bytes(&h.to_bytes());
    let tag_path = self.tag_filename(&merkle);
    Ok(tag_path.exists())
}
```

### Solution 2: Tag Registration with Internal IDs

**Approach**: Give tags internal ChangeId mappings when they're pushed to a server, treating them as a special type of change in the database.

**Pros**:
- Consistent with existing architecture
- No special casing in dependency resolution
- Enables full graph traversal including tags

**Cons**:
- More invasive change
- Requires database schema consideration
- May violate "tags are not changes" principle
- Complex migration path

**Implementation**:

```rust
pub(crate) fn register_tag<T>(
    txn: &mut T,
    internal: &ChangeId,
    merkle: &Merkle,
    tag: &Tag,
) -> Result<(), TxnErr<T::GraphError>> {
    debug!("registering tag {:?}", merkle);
    
    // Convert Merkle to Hash for storage
    let hash = Hash::from_bytes(&merkle.to_bytes());
    let shash = hash.into();
    
    // Register in internal/external tables like a change
    txn.put_external(internal, &shash)?;
    txn.put_internal(&shash, internal)?;
    
    // Mark as tag in metadata (new table needed)
    txn.put_tag_marker(internal)?;
    
    // Don't process dependencies for tags (tags don't have dependency graph)
    
    Ok(())
}
```

### Solution 3: Skip Tag Header Loading

**Approach**: Don't try to load headers for tag dependencies at all.

**Pros**:
- Simplest implementation
- No database changes needed

**Cons**:
- Information loss (can't display tag metadata in logs)
- Doesn't solve the underlying architecture issue
- May break other features expecting dependency headers

**Implementation**:

```rust
// atomic/src/commands/log.rs - mk_log_entry function
let header = if self.is_tag_hash(&h) {
    // Return minimal header for tags
    ChangeHeader {
        message: "[TAG]".to_string(),
        authors: vec![],
        timestamp: chrono::Utc::now(),
        description: None,
    }
} else {
    self.repo.changes.get_header(&h.into())?
};
```

## Recommended Implementation Strategy

**Use Solution 1 (Smart Header Loading)** as the primary fix with the following approach:

### Phase 1: Immediate Fix

1. **Add `is_tag()` method to `ChangeStore` trait**
   ```rust
   fn is_tag(&self, h: &Hash) -> Result<bool, Self::Error>;
   ```

2. **Implement filesystem detection**
   ```rust
   impl ChangeStore for FileSystem {
       fn is_tag(&self, h: &Hash) -> Result<bool, Self::Error> {
           let merkle = crate::Merkle::from_bytes(&h.to_bytes());
           let tag_path = self.tag_filename(&merkle);
           Ok(tag_path.exists())
       }
   }
   ```

3. **Modify `get_header()` to route correctly**
   ```rust
   fn get_header(&self, h: &Hash) -> Result<ChangeHeader, Self::Error> {
       if self.is_tag(h)? {
           let merkle = crate::Merkle::from_bytes(&h.to_bytes());
           self.get_tag_header(&merkle)
       } else {
           Ok(self.get_change(h)?.hashed.header)
       }
   }
   ```

### Phase 2: Performance Optimization

1. **Add caching layer** to avoid repeated filesystem checks
2. **Consider adding a tag metadata table** in the database for faster lookups
3. **Benchmark impact** of filesystem checks vs database lookups

### Phase 3: Type System Enhancement

1. **Create explicit `HashType` enum**
   ```rust
   pub enum HashType {
       Change(Hash),
       Tag(Merkle),
   }
   ```

2. **Update dependency tracking** to use `HashType` instead of generic `Hash`
3. **Refactor APIs** to be explicit about hash types where possible

## Implementation Details

### File Modifications Required

1. **libatomic/src/changestore/mod.rs**
   - Add `is_tag()` method to trait
   - Modify `get_header()` implementation

2. **libatomic/src/changestore/filesystem.rs**
   - Implement `is_tag()` for filesystem storage

3. **libatomic/src/changestore/memory.rs**
   - Implement `is_tag()` for memory storage

4. **libatomic/src/pristine/mod.rs**
   - No changes needed (existing behavior is correct)

### Test Cases Required

```rust
#[test]
fn test_change_header_loading() {
    // Verify regular changes load correctly
    let repo = test_repo();
    let change_hash = create_test_change(&repo);
    let header = repo.changes.get_header(&change_hash).unwrap();
    assert_eq!(header.message, "Test change");
}

#[test]
fn test_tag_header_loading() {
    // Verify tags load correctly via get_header()
    let repo = test_repo();
    let tag_merkle = create_test_tag(&repo);
    let tag_hash = Hash::from_bytes(&tag_merkle.to_bytes());
    let header = repo.changes.get_header(&tag_hash).unwrap();
    assert_eq!(header.message, "Test tag");
}

#[test]
fn test_change_with_tag_dependency() {
    // Verify changes with tag dependencies work end-to-end
    let repo = test_repo();
    
    // Create a tag
    let tag_merkle = create_test_tag(&repo);
    let tag_hash = Hash::from_bytes(&tag_merkle.to_bytes());
    
    // Create a change that depends on the tag
    let change = create_change_with_dependencies(&repo, vec![tag_hash]);
    
    // Push to server
    push_change(&repo, &change);
    
    // Run log command - should not fail
    let log_output = run_log_command(&repo);
    assert!(log_output.contains(&change.hash.to_base32()));
}

#[test]
fn test_is_tag_detection() {
    let changestore = FileSystem::new();
    
    // Regular change should return false
    let change_hash = create_test_change();
    assert_eq!(changestore.is_tag(&change_hash).unwrap(), false);
    
    // Tag should return true
    let tag_merkle = create_test_tag();
    let tag_hash = Hash::from_bytes(&tag_merkle.to_bytes());
    assert_eq!(changestore.is_tag(&tag_hash).unwrap(), true);
}
```

## Migration Considerations

### Backward Compatibility

✅ **Fully backward compatible** - Solution 1 doesn't change any storage formats or database schemas.

### Existing Repositories

✅ **No migration needed** - All existing changes and tags will work with the new code.

### Performance Impact

⚠️ **Minor performance impact** - Adds one filesystem existence check per header load for tags. This is acceptable because:
- Tags are relatively rare compared to changes
- Header loading is already I/O bound
- Result can be cached

### Alternative Performance Approach

If filesystem checks prove too expensive:

1. **Add tag registry table** to database:
   ```rust
   table!(tag_registry);  // Maps Hash -> ()
   ```

2. **Populate during tag creation**:
   ```rust
   txn.put_tag_registry(&hash, &())?;
   ```

3. **Check database instead of filesystem**:
   ```rust
   fn is_tag(&self, h: &Hash) -> Result<bool, Self::Error> {
       Ok(txn.get_tag_registry(h)?.is_some())
   }
   ```

## Error Handling Strategy

Following Atomic's error handling patterns:

```rust
// Add new error variant
#[derive(Debug, Error)]
pub enum ChangeStoreError {
    #[error("Change not found: {}", hash)]
    ChangeNotFound { hash: String },
    
    #[error("Tag not found: {}", hash)]
    TagNotFound { hash: String },
    
    #[error("Invalid hash format: {}", hash)]
    InvalidHash { hash: String },
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

// Provide context-rich errors
impl ChangeStore for FileSystem {
    fn get_header(&self, h: &Hash) -> Result<ChangeHeader, Self::Error> {
        if self.is_tag(h)? {
            let merkle = crate::Merkle::from_bytes(&h.to_bytes());
            self.get_tag_header(&merkle)
                .map_err(|_| ChangeStoreError::TagNotFound {
                    hash: h.to_base32()
                })
        } else {
            self.get_change(h)
                .map(|c| c.hashed.header)
                .map_err(|_| ChangeStoreError::ChangeNotFound {
                    hash: h.to_base32()
                })
        }
    }
}
```

## Logging and Debugging

Add comprehensive logging following Atomic's patterns:

```rust
fn get_header(&self, h: &Hash) -> Result<ChangeHeader, Self::Error> {
    debug!("Loading header for hash: {}", h.to_base32());
    
    if self.is_tag(h)? {
        debug!("Hash identified as tag, loading tag header");
        let merkle = crate::Merkle::from_bytes(&h.to_bytes());
        let result = self.get_tag_header(&merkle);
        if result.is_ok() {
            debug!("Successfully loaded tag header");
        } else {
            warn!("Failed to load tag header for: {}", h.to_base32());
        }
        result
    } else {
        debug!("Hash identified as change, loading change header");
        let result = Ok(self.get_change(h)?.hashed.header);
        if result.is_ok() {
            debug!("Successfully loaded change header");
        } else {
            warn!("Failed to load change header for: {}", h.to_base32());
        }
        result
    }
}
```

## Summary

The tag dependency registration issue stems from the architectural decision that **tags are not changes** and thus don't participate in the dependency graph with internal IDs. After hash unification, the type system no longer distinguishes between change and tag hashes, causing header loading to fail.

**Solution 1 (Smart Header Loading)** is recommended because it:
- ✅ Fixes the immediate problem
- ✅ Maintains architectural integrity (tags remain separate from changes)
- ✅ Requires minimal code changes
- ✅ Is fully backward compatible
- ✅ Follows Atomic's design patterns
- ✅ Enables future optimizations

The implementation is straightforward and aligns with Atomic's principles of type safety, error transparency, and performance consideration.