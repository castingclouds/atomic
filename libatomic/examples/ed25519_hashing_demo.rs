//! Demonstration of pure Ed25519 hashing in Atomic VCS
//!
//! This example shows how Atomic now uses Ed25519 elliptic curve points
//! as hashes throughout the system, enabling cryptographic proofs for
//! AI attestations and other advanced features.

use libatomic::pristine::{Base32, Hash, Hasher, Merkle};

fn main() {
    println!("=== Atomic VCS: Pure Ed25519 Hashing Demo ===\n");

    // 1. Basic hashing
    println!("1. Creating Ed25519 hashes from data:");
    let mut hasher = Hasher::default();
    hasher.update(b"Hello, Atomic!");
    let hash1 = hasher.finish();
    println!("   Hash of 'Hello, Atomic!': {}", hash1.to_base32());

    // 2. Deterministic hashing
    println!("\n2. Demonstrating deterministic hashing:");
    let mut hasher2 = Hasher::default();
    hasher2.update(b"Hello, Atomic!");
    let hash2 = hasher2.finish();
    println!("   Same input produces same hash: {}", hash1 == hash2);

    // 3. Different inputs produce different hashes
    println!("\n3. Different inputs produce different Ed25519 points:");
    let mut hasher3 = Hasher::default();
    hasher3.update(b"Different data");
    let hash3 = hasher3.finish();
    println!("   Hash of 'Different data': {}", hash3.to_base32());
    println!("   Hashes are different: {}", hash1 != hash3);

    // 4. Hash IS Merkle - no conversion needed
    println!("\n4. Hash and Merkle are unified:");
    let merkle: Merkle = hash1; // Direct assignment - they're the same type!
    println!("   Hash as Merkle: {}", merkle.to_base32());

    // 5. The special NONE hash (Ed25519 base point)
    println!("\n5. The NONE hash (Ed25519 base point):");
    let none_hash = Hash::NONE;
    println!("   Hash::NONE: {}", none_hash.to_base32());
    println!("   Is none: {}", none_hash.is_none());

    // 6. Merkle operations (next)
    println!("\n6. Merkle chain operations:");
    let base = Merkle::zero();
    println!("   Base merkle: {}", base.to_base32());

    let next1 = base.next(&hash1);
    println!("   After applying hash1: {}", next1.to_base32());

    let next2 = next1.next(&hash2);
    println!("   After applying hash2: {}", next2.to_base32());

    // 7. Round-trip through Base32
    println!("\n7. Base32 serialization round-trip:");
    let base32_str = hash1.to_base32();
    let decoded = Hash::from_base32(base32_str.as_bytes()).unwrap();
    println!("   Original == Decoded: {}", hash1 == decoded);

    // 8. AI Attribution use case
    println!("\n8. AI Attribution Example:");
    println!("   With Ed25519 hashes, we can now:");
    println!("   - Cryptographically sign AI-generated changes");
    println!("   - Create verifiable chains of attribution");
    println!("   - Enable zero-knowledge proofs of AI involvement");
    println!("   - Build trust through mathematical guarantees");

    // 9. Performance note
    println!("\n9. Performance:");
    println!("   Ed25519 operations are slower than BLAKE3, but:");
    println!("   - Changes are created at human timescales");
    println!("   - Cryptographic properties are worth the trade-off");
    println!("   - Enables unique features for AI attribution");

    println!("\n=== Summary ===");
    println!("Atomic VCS now uses pure Ed25519 for all hashing:");
    println!("- No more BLAKE3 dependency");
    println!("- Single, unified hashing mechanism");
    println!("- Enables cryptographic proofs for AI attestations");
    println!("- Simplifies push/pull/clone synchronization");
    println!("- Future-proof for advanced cryptographic features");
}
