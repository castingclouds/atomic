# Inserting Changes with Consolidating Tags

**Status**: Workflow Documentation  
**Version**: 1.0  
**Date**: 2025-01-15  
**Applies to**: Increment 5+  

---

## Overview

This document describes how to insert changes at arbitrary positions in the dependency DAG using `atomic record -e` (edit mode) and how consolidating tags automatically incorporate these changes.

**Key Principle**: Atomic does not have branches. It has a DAG (directed acyclic graph) of changes, and consolidating tags provide snapshot reference points. Users can manually edit dependencies to insert changes anywhere in the DAG.

---

## The Mechanism: `atomic record -e`

When you run `atomic record -e`, Atomic opens your editor with the change file, including a **Dependencies** section that you can manually edit.

### Example Change File

```toml
message = 'Security fix'
timestamp = '2025-01-15T11:00:00.000000Z'

[[authors]]
key = '3rAPLeTJdKPwzE3eFHGEeeH7iW9fPTXGotmUPeWAekph'

# Dependencies
[0] BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB # C2

# Hunks
1. Edit in "security-fix.txt":1 1.0 "UTF-8"
  up 1.1, new 1:5, down
+ security fix content
```

**You can edit the Dependencies section to:**
- Add dependencies on older changes
- Remove default dependencies
- Depend on multiple changes (merging paths)
- Insert the change anywhere in the DAG

---

## Complete Workflow: Inserting a Security Fix

### Initial Setup

```bash
# Initialize repository
atomic init myproject
cd myproject

# Create initial changes
echo "v1" > file.txt
atomic add file.txt
atomic record -m "Change 1"  # Hash: AAAA...

echo "v2" > file.txt
atomic record -m "Change 2"  # Hash: BBBB...

echo "v3" > file.txt
atomic record -m "Change 3"  # Hash: CCCC...

# Create first consolidating tag
atomic tag create v1.0 --consolidate -m "Release 1.0"
# Output: MNYNGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC (consolidating: 3 changes)
```

**DAG State:**
```
C1 → C2 → C3
          ↓
      Tag v1.0 [consolidates: C1, C2, C3]
```

### Continue Development

```bash
# Continue with more changes
echo "v4" > file.txt
atomic record -m "Change 4"  # Hash: DDDD...

# C4 automatically depends on Tag v1.0
```

**DAG State:**
```
C1 → C2 → C3 → Tag v1.0 → C4
```

### Insert Security Fix Between C2 and C3

**Scenario**: You discover a security issue that needs to be fixed, and you want to insert it logically between C2 and C3 in the dependency chain.

```bash
# Create the security fix
echo "security patch" > security-fix.txt
atomic add security-fix.txt

# Use edit mode to control dependencies
atomic record -e -m "Security fix for vulnerability CVE-2025-1234"
```

**Editor opens with:**
```toml
message = 'Security fix for vulnerability CVE-2025-1234'
timestamp = '2025-01-15T11:00:00.000000Z'

[[authors]]
key = '3rAPLeTJdKPwzE3eFHGEeeH7iW9fPTXGotmUPeWAekph'

# Dependencies
[0] DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD # Change 4 (default: latest)

# Hunks
1. File addition: "security-fix.txt" in "/" 1.0
...
```

**Manual Edit - Set dependency to C2:**
```toml
message = 'Security fix for vulnerability CVE-2025-1234'
timestamp = '2025-01-15T11:00:00.000000Z'

[[authors]]
key = '3rAPLeTJdKPwzE3eFHGEeeH7iW9fPTXGotmUPeWAekph'

# Dependencies
[0] BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB # Change 2

# Hunks
1. File addition: "security-fix.txt" in "/" 1.0
...
```

**Save and close the editor.**

**DAG State After Insertion:**
```
C1 → C2 → C2.5 (security fix - new!)
     └─→ C3 → Tag v1.0 → C4

# Note: C2.5 depends on C2
# C3 still depends on C2 (original path preserved)
# Tag v1.0 is immutable (still represents original C1→C2→C3)
```

**Output:**
```
Recorded change: EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE
```

---

## Merging the Paths

Now you have two paths from C2:
1. **Original path**: C2 → C3 → Tag v1.0 → C4
2. **Security fix path**: C2 → C2.5

To merge these paths, create a new change that depends on both:

```bash
# Create a change that merges both paths
echo "v5 - includes security fix" > file.txt
atomic record -e -m "Change 5 - merge security fix"
```

**Editor opens:**
```toml
message = 'Change 5 - merge security fix'
timestamp = '2025-01-15T11:30:00.000000Z'

[[authors]]
key = '3rAPLeTJdKPwzE3eFHGEeeH7iW9fPTXGotmUPeWAekph'

# Dependencies
[0] DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD # Change 4 (default)

# Hunks
...
```

**Manual Edit - Add both paths:**
```toml
message = 'Change 5 - merge security fix'
timestamp = '2025-01-15T11:30:00.000000Z'

[[authors]]
key = '3rAPLeTJdKPwzE3eFHGEeeH7iW9fPTXGotmUPeWAekph'

# Dependencies
[0] EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE # C2.5 (security fix)
[1] DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD # C4 (main line)

# Hunks
...
```

**Save and close.**

**DAG State After Merge:**
```
C1 → C2 → C2.5 ↘
     └─→ C3 → Tag v1.0 → C4 → C5
                            ↗
```

---

## Creating a New Consolidating Tag

Now create a new consolidating tag that represents the current state including the security fix:

```bash
atomic tag create v1.0.1 --consolidate -m "Release 1.0.1 with security fix"
```

**What Happens:**
1. Tag v1.0.1 starts from the channel tip (C5)
2. Traverses the DAG backwards:
   - C5 → [C2.5, C4]
   - C4 → Tag v1.0 → **expands to [C3, C2, C1]**
   - C2.5 → C2 (already visited via Tag v1.0)
3. Collects all reachable changes: **[C1, C2, C2.5, C3, C4, C5]**
4. ✅ **C2.5 is automatically included!**

**Output:**
```
XYZHGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC (consolidating: 6 changes)
```

**Final DAG State:**
```
C1 → C2 → C2.5 ↘
     └─→ C3 → Tag v1.0 → C4 → C5
                            ↗   ↓
                         Tag v1.0.1 [consolidates: C1, C2, C2.5, C3, C4, C5]
```

---

## Verifying the Result

```bash
# List all consolidating tags
atomic tag list --consolidating

# Output:
Tag: MNYNGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC (channel: main)
  Consolidated changes: 3
  Dependencies before: 6
  Effective dependencies: 1
  Dependency reduction: 5

Tag: XYZHGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC (channel: main)
  Consolidated changes: 6
  Dependencies before: 21
  Effective dependencies: 1
  Dependency reduction: 20
```

---

## Key Principles

### 1. No Branches, Just DAG Topology

Atomic does NOT have Git-style branches. Instead:
- ✅ You have a DAG of changes
- ✅ Changes can have multiple parents (merges)
- ✅ You control dependencies explicitly via `-e`
- ✅ Channels filter/view the DAG (not branches)

### 2. Tags Are Immutable Snapshots

When you create a consolidating tag:
- ✅ It represents a **specific point in time**
- ✅ It consolidates all changes reachable at that moment
- ✅ It **never changes** after creation
- ✅ Inserting changes later doesn't modify old tags

### 3. Tags Consolidate via DAG Traversal

When creating a new tag:
- ✅ Starts from the channel tip
- ✅ Traverses backwards through ALL dependencies
- ✅ Expands any tag references it encounters
- ✅ Automatically includes all reachable changes
- ✅ **No manual specification needed**

### 4. Full Control via `-e` Flag

The `-e` (edit mode) gives you complete control:
- ✅ See all dependencies for your change
- ✅ Add/remove/modify dependencies manually
- ✅ Insert changes anywhere in the DAG
- ✅ Merge multiple paths
- ✅ No restrictions on DAG topology

---

## Common Patterns

### Pattern 1: Hotfix Insertion

Insert a critical fix into an earlier point in history:

```bash
atomic record -e -m "Critical security fix"
# Set dependencies to earlier change
# Save and close
```

### Pattern 2: Merging Parallel Paths

Combine two independent lines of development:

```bash
atomic record -e -m "Merge feature and bugfix"
# Set dependencies:
# [0] <feature_hash>
# [1] <bugfix_hash>
# Save and close
```

### Pattern 3: Backporting to Old Versions

Create a change based on an old version:

```bash
atomic record -e -m "Backport to v1.0"
# Set dependency to old tag:
# [0] <v1.0_tag_hash>
# Save and close
```

### Pattern 4: Incremental Consolidation

Build on previous consolidating tags:

```bash
# v1.0 consolidates C1-C10
atomic tag create v1.0 --consolidate

# Work continues...
# v1.1 consolidates C1-C20 (includes everything from v1.0 + new changes)
atomic tag create v1.1 --consolidate
# Automatically includes all changes from v1.0 via DAG traversal
```

---

## Advanced: Multiple Independent Paths

You can have multiple independent paths in the DAG:

```bash
# Main path
atomic record -m "Main 1"  # M1
atomic record -m "Main 2"  # M2

# Feature path (depends on M1)
atomic record -e -m "Feature 1"
# Set dependency: [0] <M1_hash>
atomic record -m "Feature 2"  # F2 (depends on F1)

# DAG:
# M1 → M2
#  └─→ F1 → F2

# Consolidate main path
atomic tag create v1.0-main --consolidate  # Consolidates M1, M2

# Consolidate feature path
atomic tag create v1.0-feature --consolidate  # Consolidates M1, F1, F2

# Note: Both tags include M1 (shared ancestor)
```

---

## Troubleshooting

### Q: My inserted change isn't in the new tag

**A**: Make sure your latest change has the inserted change in its ancestry. Use `atomic record -e` to add it as a dependency.

### Q: Can I modify an existing tag?

**A**: No. Tags are immutable. Create a new tag with the desired changes.

### Q: How do I see the full dependency graph?

**A**: Use `atomic log --graph` (future feature) or examine change files with `atomic show <hash>`.

### Q: What if I make a mistake in dependencies?

**A**: Create a new change with the correct dependencies. The old change remains in the DAG but won't be in the mainline if nothing depends on it.

---

## Best Practices

### 1. Use Meaningful Commit Messages
```bash
atomic record -e -m "Security: Fix CVE-2025-1234 in auth module"
```

### 2. Document Dependency Choices
```toml
# Dependencies
[0] BBBB... # C2 - inserting before C3 to maintain compatibility
```

### 3. Tag After Insertions
After inserting changes and merging paths, create a new consolidating tag to capture the updated state.

### 4. Verify DAG Structure
Before creating a tag, verify your DAG structure is correct by examining recent changes.

### 5. Use Comments in Dependencies
```toml
# Dependencies
[0] AAAA... # Base change
[1] BBBB... # Security fix (inserted)
[2] CCCC... # Feature work (merge point)
```

---

## Future Enhancements

These features are planned for future increments:

- **Visual DAG editor**: GUI for manipulating dependencies
- **Automatic merge detection**: Suggest merge points
- **Conflict resolution**: Handle conflicting edits
- **Tag queries**: Query which changes are in which tags
- **Dependency validation**: Warn about cycles or invalid structures

---

## Summary

**Inserting changes with consolidating tags is simple:**

1. Use `atomic record -e` to create a change with custom dependencies
2. Edit the Dependencies section to place the change anywhere in the DAG
3. Create a new consolidating tag - it automatically includes all reachable changes
4. Tags are immutable snapshots - they never change after creation
5. No branches needed - just explicit dependency control

**The beauty of this design:**
- ✅ Full control via `-e` flag
- ✅ No magic - explicit is better than implicit
- ✅ Tags "just work" by traversing the DAG
- ✅ Simple and powerful

---

*Document Version: 1.0*  
*Author: Atomic VCS Team*  
*Date: 2025-01-15*