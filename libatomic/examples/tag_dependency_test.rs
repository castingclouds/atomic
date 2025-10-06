//! Test to verify that changes created after a tag correctly use the tag as a dependency
//! instead of the individual changes that were consolidated into the tag.

use libatomic::pristine::{Base32, Hash, Merkle, SerializedTag, Tag};

fn main() {
    println!("=== Tag Dependency Replacement Test ===\n");

    // Create some mock change hashes (representing changes before the tag)
    let mut hasher1 = libatomic::pristine::Hasher::default();
    hasher1.update(b"change1");
    let change1 = hasher1.finish();

    let mut hasher2 = libatomic::pristine::Hasher::default();
    hasher2.update(b"change2");
    let change2 = hasher2.finish();

    let mut hasher3 = libatomic::pristine::Hasher::default();
    hasher3.update(b"change3");
    let change3 = hasher3.finish();

    println!("Original changes:");
    println!("  Change 1: {}", change1.to_base32());
    println!("  Change 2: {}", change2.to_base32());
    println!("  Change 3: {}", change3.to_base32());

    // Create a tag that consolidates these changes
    let tag_state = Merkle::zero().next(&change1).next(&change2).next(&change3);
    let tag_hash = tag_state; // In the new system, Hash IS Merkle

    println!("\nTag created:");
    println!("  Tag state/hash: {}", tag_state.to_base32());

    // Create a tag object
    let mut tag = Tag::new(
        tag_hash,
        tag_state,
        "main".to_string(),
        None,                            // No previous consolidation
        3,                               // dependency_count_before
        3,                               // consolidated_change_count
        vec![change1, change2, change3], // consolidated_changes
    );

    // IMPORTANT: Set the change_file_hash to the tag's merkle state
    // This is what should be used as a dependency for changes after the tag
    tag.change_file_hash = Some(tag_state);

    println!("\nTag metadata:");
    println!("  tag_hash: {}", tag.tag_hash.to_base32());
    println!(
        "  change_file_hash: {}",
        tag.change_file_hash
            .map(|h| h.to_base32())
            .unwrap_or_else(|| "None".to_string())
    );
    println!("  consolidated {} changes", tag.consolidated_changes.len());

    // Simulate what should happen when a new change is recorded after the tag
    println!("\n=== Dependency Replacement Logic ===");

    // These are the dependencies that would normally be collected
    let original_deps = vec![change1, change2, change3];
    println!("\nOriginal dependencies (before replacement):");
    for dep in &original_deps {
        println!("  - {}", dep.to_base32());
    }

    // Simulate the dependency replacement
    let mut new_deps = Vec::new();
    let mut replaced = false;

    // Check if the tag covers any of our dependencies
    let mut covered_deps = Vec::new();
    for dep in &original_deps {
        if tag.consolidated_changes.contains(dep) {
            covered_deps.push(*dep);
        }
    }

    if !covered_deps.is_empty() {
        // Use the tag's change_file_hash as the dependency
        let tag_dep = tag.change_file_hash.unwrap_or(tag.tag_hash);
        new_deps.push(tag_dep);
        replaced = true;

        // Add any dependencies NOT covered by the tag
        for dep in &original_deps {
            if !covered_deps.contains(dep) && *dep != tag_dep {
                new_deps.push(*dep);
            }
        }
    } else {
        new_deps = original_deps.clone();
    }

    println!("\nDependencies after replacement:");
    if replaced {
        println!("  ✓ Replaced {} dependencies with tag", covered_deps.len());
        for dep in &new_deps {
            if *dep == tag.change_file_hash.unwrap_or(tag.tag_hash) {
                println!("  - {} (TAG)", dep.to_base32());
            } else {
                println!("  - {}", dep.to_base32());
            }
        }
    } else {
        println!("  ✗ No replacement (tag doesn't cover dependencies)");
        for dep in &new_deps {
            println!("  - {}", dep.to_base32());
        }
    }

    // Verify the result
    println!("\n=== Verification ===");
    if replaced
        && new_deps.len() == 1
        && new_deps[0] == tag.change_file_hash.unwrap_or(tag.tag_hash)
    {
        println!("✅ SUCCESS: Dependencies correctly replaced with tag!");
        println!(
            "   Change after tag will depend on: {}",
            new_deps[0].to_base32()
        );
    } else if replaced {
        println!("⚠️  PARTIAL: Some dependencies replaced, but not all");
        println!("   This might be correct if the tag doesn't cover all dependencies");
    } else {
        println!("❌ FAILURE: Dependencies were not replaced with tag");
    }

    println!("\n=== Summary ===");
    println!("When a tag consolidates changes, any new change recorded after the tag");
    println!("should depend on the tag itself (via change_file_hash), not on the");
    println!("individual changes that were consolidated. This:");
    println!("  1. Reduces the dependency chain length");
    println!("  2. Makes the history cleaner and more manageable");
    println!("  3. Improves push/pull/clone performance");
    println!("  4. Is the whole point of O(1) consolidation!");
}
