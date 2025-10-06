# Consolidating Tags - Quick Reference

**Version**: 1.0  
**Date**: 2025-01-15  
**Applies to**: Atomic VCS Increment 5+

---

## What Are Consolidating Tags?

Consolidating tags are **immutable snapshots** that represent a point in the dependency DAG. They provide a single reference point instead of accumulated dependencies.

**Key Benefits**:
- 🎯 Reduce dependency count from O(n) to O(1)
- 📦 Create clean reference points for releases
- 🔄 Enable flexible insertion workflows
- 📊 Track AI contribution across changes

---

## Basic Commands

### Create a Consolidating Tag

```bash
# Create tag consolidating all changes up to current state
atomic tag create v1.0 --consolidate -m "Release 1.0"

# Output: MNYNGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC (consolidating: 25 changes)
```

### List Consolidating Tags

```bash
# List all consolidating tags
atomic tag list --consolidating

# List with AI attribution info
atomic tag list --consolidating --attribution
```

### List on Specific Channel

```bash
atomic tag list --consolidating --channel feature-branch
```

---

## Common Workflows

### 1. Standard Release Workflow

```bash
# Develop normally
atomic record -m "Feature 1"
atomic record -m "Feature 2"
atomic record -m "Feature 3"

# Create release tag
atomic tag create v1.0 --consolidate -m "Release 1.0"

# Continue development
atomic record -m "Feature 4"
# (Feature 4 will depend on Tag v1.0)
```

### 2. Inserting a Hotfix

**Scenario**: Need to insert a fix between existing changes

```bash
# Create the fix
echo "security patch" > fix.txt
atomic add fix.txt

# Use edit mode to control dependencies
atomic record -e -m "Security fix CVE-2025-1234"
```

**In editor**:
```toml
# Dependencies
[0] BBBB... # C2 (change to depend on earlier change)
```

**Save and close** - fix is inserted at specified position

### 3. Merging Multiple Paths

**Scenario**: Combine parallel development lines

```bash
atomic record -e -m "Merge feature and bugfix"
```

**In editor**:
```toml
# Dependencies
[0] AAAA... # Feature change
[1] BBBB... # Bugfix change
```

**Save and close** - creates a merge point

### 4. Incremental Consolidation

```bash
# First release
atomic tag create v1.0 --consolidate -m "Release 1.0"

# More development...
atomic record -m "Change A"
atomic record -m "Change B"

# Second release (automatically includes v1.0)
atomic tag create v1.1 --consolidate -m "Release 1.1"
```

### 5. Backporting

```bash
# Create change based on old tag
atomic record -e -m "Backport to v1.0"
```

**In editor**:
```toml
# Dependencies
[0] <v1.0_tag_hash> # Depend on old tag
```

---

## Understanding the DAG

### Before Consolidation

```
C1 → C2 → C3 → C4 → C5
Dependencies:
  C1: [] (0 deps)
  C2: [C1] (1 dep)
  C3: [C1, C2] (2 deps)
  C4: [C1, C2, C3] (3 deps)
  C5: [C1, C2, C3, C4] (4 deps)
Total: 10 dependencies
```

### After Consolidation

```
C1 → C2 → C3 → C4 → C5 → Tag v1.0 → C6
Dependencies:
  C1-C5: (same as before)
  Tag v1.0: consolidates [C1, C2, C3, C4, C5]
  C6: [Tag v1.0] (1 dep, equivalent to 5 deps)
  C7: [C6] (1 dep, equivalent to 6 deps via tag)
```

### After Insertion

```
C1 → C2 → C2.5 (inserted) ↘
     └─→ C3 → C4 → C5 → Tag v1.0 → C6 → Merge

Tag v1.0 is IMMUTABLE (still represents C1-C5)
Create Tag v1.1 to consolidate the new path
```

---

## Key Principles

### 1. Tags Are Immutable
- Once created, a tag never changes
- Represents specific point in time
- Inserting changes doesn't modify existing tags

### 2. No Branches
- Atomic doesn't have Git-style branches
- Just a DAG with explicit dependencies
- Channels filter/view the DAG

### 3. Tags Consolidate via Traversal
- Tag creation traverses DAG from tip
- Expands any tag references encountered
- Automatically includes all reachable changes

### 4. Full Control via `-e`
- Edit dependencies manually
- Insert changes anywhere
- Merge multiple paths
- No restrictions

---

## Tag Output Format

```
Tag: MNYNGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC (channel: main)
  Consolidated changes: 25
  Dependencies before: 300
  Effective dependencies: 1
  Dependency reduction: 299
  Attribution:
    Total changes: 25
    AI-assisted: 8
    Human-authored: 17
    AI contribution: 32.0%
```

**Fields**:
- **Tag**: Merkle hash (53 characters, Base32)
- **Consolidated changes**: Number of changes in this tag
- **Dependencies before**: Total deps before consolidation
- **Effective dependencies**: Deps after consolidation (1 = just the tag)
- **Dependency reduction**: How many deps saved

---

## Advanced Patterns

### Pattern: Multiple Independent Paths

```bash
# Main development
atomic record -m "M1"
atomic record -m "M2"

# Feature path (depend on M1)
atomic record -e -m "F1"
# Set: [0] <M1_hash>

atomic record -m "F2"  # Depends on F1

# Result:
# M1 → M2
#  └─→ F1 → F2

# Tag main path
atomic tag create v1.0-main --consolidate

# Tag feature path
atomic tag create v1.0-feature --consolidate
```

### Pattern: Hotfix Workflow

```bash
# Production at Tag v1.0
atomic tag create v1.0 --consolidate

# Development continues on v1.1
atomic record -m "New feature"

# Critical bug found in v1.0
atomic record -e -m "Hotfix for v1.0"
# Set: [0] <v1.0_tag_hash>

# Create hotfix tag
atomic tag create v1.0.1 --consolidate

# Merge hotfix into main development
atomic record -e -m "Merge hotfix"
# Set: [0] <v1.0.1_hash>, [1] <latest_feature_hash>
```

---

## Troubleshooting

### My inserted change isn't in the new tag

**Solution**: Make sure the change is reachable from the channel tip. Use `atomic record -e` to add it as a dependency of a later change.

### Can I modify an existing tag?

**No**. Tags are immutable. Create a new tag with the desired state.

### How do I see dependencies for a change?

```bash
atomic show <change_hash>
# Shows the full change file including dependencies
```

### What if I make a mistake in dependencies?

Create a new change with correct dependencies. The old change remains in the DAG but won't be in the mainline if nothing depends on it.

---

## Best Practices

### ✅ DO

- Create tags at release points
- Use meaningful tag messages
- Document why you're inserting changes
- Test thoroughly after creating tags
- Use `-e` to control complex dependency scenarios

### ❌ DON'T

- Don't try to modify existing tags
- Don't assume tags work like Git branches
- Don't forget to create new tags after insertions
- Don't use tags for every commit (only at logical boundaries)

---

## Performance Tips

- Tags reduce dependency resolution time significantly
- Create tags at regular intervals (releases, milestones)
- Use `--consolidate` for long-lived development
- Channels can have different consolidating strategies

---

## Example Session

```bash
# Initialize
atomic init myproject
cd myproject

# Create changes
atomic record -m "Initial commit"     # C1
atomic record -m "Add feature"        # C2
atomic record -m "Fix bug"            # C3
atomic tag create v1.0 --consolidate  # Tag v1.0 = [C1, C2, C3]

# Continue
atomic record -m "New feature"        # C4 (depends on Tag v1.0)

# Insert hotfix
atomic record -e -m "Security fix"
# Edit dependencies: [0] <C2_hash>
# This creates C2.5 between C2 and C3

# Merge paths
atomic record -e -m "Merge security fix"
# Edit dependencies: [0] <C2.5_hash>, [1] <C4_hash>
# This creates C5 that merges both paths

# Create new tag
atomic tag create v1.0.1 --consolidate  # Tag v1.0.1 = [C1, C2, C2.5, C3, C4, C5]

# List tags
atomic tag list --consolidating

# Output shows both tags:
# - Tag v1.0: 3 changes
# - Tag v1.0.1: 6 changes (includes C2.5!)
```

---

## Related Documentation

- **Full Workflow Guide**: `docs/workflows/inserting-changes-with-tags.md`
- **Implementation Details**: `docs/implementation/increment-05-complete.md`
- **Architecture**: `AGENTS.md`

---

## Quick Command Reference

| Command | Description |
|---------|-------------|
| `atomic tag create NAME --consolidate` | Create consolidating tag |
| `atomic tag list --consolidating` | List all consolidating tags |
| `atomic tag list --consolidating --attribution` | List with AI attribution |
| `atomic record -e` | Create change with manual dependency control |
| `atomic show <hash>` | View change details including dependencies |

---

*Last Updated: 2025-01-15*  
*Version: 1.0*  
*Atomic VCS Documentation*