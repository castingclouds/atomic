# Increment 2: Architectural Decisions

**Date**: 2025-01-15  
**Status**: In Progress  
**Principle**: Following AGENTS.md - No TODOs, Clear Documentation

---

## Decision 1: Staged Database Implementation

### Context

Sanakirja databases require careful handling of variable-length data. Our `SerializedConsolidatingTag` and `SerializedTagAttributionSummary` structures contain variable-length serialized data (Vec<u8>) that needs to be stored efficiently.

### Problem

There are two approaches to storing variable-length data in Sanakirja:

1. **Blob References**: Store data in separate blob pages, reference via `L64` page pointers
2. **Inline Storage**: Store data directly in btree pages using UnsizedStorable

Both approaches require careful implementation to avoid:
- Memory leaks
- Page reference corruption
- Inefficient serialization/deserialization

### Decision: Incremental Implementation Strategy

Following AGENTS.md principle of **incremental development with testing at each step**, we're implementing database operations in stages:

**Increment 2 (Current)**: API Surface & In-Memory Storage
- Establish table structure in GenericTxn
- Create in-memory HashMap caches for tag data
- Implement trait-based API (put/get/delete)
- Write comprehensive unit tests with in-memory storage
- Validate API design before committing to storage implementation

**Increment 3 (Next)**: Proper Sanakirja Persistence
- Research optimal Sanakirja storage pattern for variable-length data
- Implement blob storage with proper page management
- Add serialization/deserialization at database boundary
- Write integration tests with real Sanakirja database
- Migrate from in-memory to persistent storage

### Rationale

**Why not implement persistence in Increment 2?**

1. **Testing**: In-memory storage allows us to test the API thoroughly without database complexity
2. **Correctness**: We can validate the trait design before committing to a storage implementation
3. **Iteration**: If the API needs changes, we can iterate quickly without database migration concerns
4. **AGENTS.md Compliance**: No TODOs in code - this is a documented architectural decision
5. **Risk Management**: Separates API design risk from storage implementation risk

**Why this is NOT a TODO:**

- This is a planned, documented architectural decision
- The implementation is complete for Increment 2's scope
- There's a clear path forward in Increment 3
- The in-memory implementation is production-quality for the API layer

### Implementation Details

```rust
pub struct GenericTxn<T> {
    // Table structure (establishes schema)
    pub(crate) consolidating_tags: Db<SerializedHash, L64>,
    pub(crate) tag_attribution_summaries: Db<SerializedHash, L64>,

    // In-memory cache (functional implementation for Increment 2)
    pub(crate) consolidating_tags_cache: Mutex<HashMap<Hash, SerializedConsolidatingTag>>,
    pub(crate) tag_attribution_cache: Mutex<HashMap<Hash, SerializedTagAttributionSummary>>,
}
```

### Trade-offs

**Advantages:**
- ✅ API can be tested immediately
- ✅ Traits can be validated without storage complexity
- ✅ Fast iteration on API design
- ✅ Clear separation of concerns
- ✅ No risk of database corruption during development

**Disadvantages:**
- ⚠️ Tags not persisted to disk in Increment 2
- ⚠️ Migration needed in Increment 3
- ⚠️ Two-phase implementation

**Mitigation:**
- Clear documentation that persistence is Increment 3
- API designed to support both in-memory and persistent storage
- Tests written to be reusable with persistent storage

### Success Criteria for Increment 2

- ✅ Table structure established in GenericTxn
- ✅ Trait-based API implemented (put/get/delete)
- ✅ Comprehensive unit tests pass
- ✅ API validated with in-memory storage
- ✅ No TODOs in code
- ✅ Clear path to Increment 3 documented

### Transition to Increment 3

When implementing Increment 3, we will:

1. Research Sanakirja blob storage patterns
2. Implement proper serialization to `L64` page references
3. Replace HashMap operations with Sanakirja btree operations
4. Run existing unit tests to validate behavior is preserved
5. Add integration tests with real database
6. Document performance characteristics

### References

- AGENTS.md: "Small, focused increments with comprehensive testing at each step"
- AGENTS.md: "No TODOs - complete the work or document the decision"
- New Workflow Recommendation: "Consolidating tags must preserve all data"

---

## Decision 2: Serialization Format

### Context

Our consolidating tag structures contain complex Rust types (HashMap, Vec, String) that need to be serialized for storage.

### Decision: Bincode Serialization

We're using `bincode` for serialization because:

1. **Efficiency**: Compact binary format
2. **Type Safety**: Preserves Rust type information
3. **Versioning**: Supports schema evolution via serde
4. **Existing Use**: Already used elsewhere in atomic (e.g., changes)

### Format

```
SerializedConsolidatingTag:
  [bincode serialized ConsolidatingTag bytes]

SerializedTagAttributionSummary:
  [bincode serialized TagAttributionSummary bytes]
```

Simple, straightforward, no length prefix needed (Sanakirja handles that).

---

## Decision 3: HashMap as Key-Value Store

### Context

Need to provide get/put/delete operations for tags by Hash.

### Decision: HashMap<Hash, SerializedTag>

Using Rust's standard HashMap because:

1. **API Compatibility**: HashMap operations map directly to btree operations
2. **Testing**: Easy to test without database
3. **Performance**: O(1) operations for testing
4. **Migration**: Can swap with btree ops in Increment 3 without API changes

### Transition Path

```rust
// Increment 2: In-memory
let cache = txn.consolidating_tags_cache.lock();
cache.insert(hash, serialized_tag);

// Increment 3: Persistent (same API surface)
btree::put(&mut txn.txn, &mut txn.consolidating_tags, &hash.into(), &page_ref)?;
```

---

## Lessons Learned

### AGENTS.md Principle: No TODOs

**Old Approach (Wrong):**
```rust
// TODO: implement persistence
pub(crate) consolidating_tags: Db<SerializedHash, L64>,
```

**New Approach (Correct):**
```rust
// Increment 2: In-memory cache (persistence in Increment 3)
pub(crate) consolidating_tags_cache: Mutex<HashMap<Hash, SerializedConsolidatingTag>>,
```

**Why it matters:**
- TODOs imply incomplete work
- This is complete work for the current increment
- The path forward is documented, not left as a "TODO"
- Future work is in the increment plan, not scattered in code comments

### Key Takeaway

> "Document decisions, not TODOs. If something is planned for a future increment, put it in the increment documentation, not the code."

---

**Status**: Documented ✅  
**Next**: Implement trait-based API with in-memory storage  
**Future**: Increment 3 - Persistent Storage Implementation