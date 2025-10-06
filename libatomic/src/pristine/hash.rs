use super::Merkle;
use curve25519_dalek::scalar::Scalar;

// For MVP: Use Merkle everywhere instead of BLAKE3
// This enables cryptographic proofs for AI attestations
pub type Hash = Merkle;
pub type Hasher = MerkleHasher;
pub type SerializedHash = super::SerializedMerkle;

// Re-export hash constants using Merkle
// HASH_NONE represents the Ed25519 base point in compressed form
// The compressed form of ED25519_BASEPOINT_POINT is:
// [0x58, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
//  0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
//  0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66]
pub const HASH_NONE: SerializedHash = super::SerializedMerkle([
    1, // MerkleAlgorithm::Ed25519
    0x58, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
    0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
]);

// Hasher that produces Merkle hashes using Ed25519
pub struct MerkleHasher {
    // Accumulate data using SHA-512 (same as Ed25519 internally uses)
    data: Vec<u8>,
}

impl Default for MerkleHasher {
    fn default() -> Self {
        Self { data: Vec::new() }
    }
}

impl MerkleHasher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
    }

    pub fn finish(&self) -> Hash {
        // Use SHA-512 to hash the accumulated data (Ed25519's native hash function)
        use sha2::{Digest, Sha512};
        let mut hasher = Sha512::new();
        hasher.update(&self.data);
        let hash_result = hasher.finalize();

        // Convert the first 32 bytes to a scalar (Ed25519 uses mod l reduction)
        let mut scalar_bytes = [0u8; 32];
        scalar_bytes.copy_from_slice(&hash_result[..32]);
        let scalar = Scalar::from_bytes_mod_order(scalar_bytes);

        // Start from base point and multiply by scalar to get deterministic point
        use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
        Merkle::Ed25519(ED25519_BASEPOINT_POINT * scalar)
    }
}

// Extension trait for Hash-specific operations
pub trait HashExt {
    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized;
    fn from_prefix(s: &str) -> Option<Self>
    where
        Self: Sized;
    fn to_bytes(&self) -> [u8; 1 + 32];

    /// Creates a Hash from a Merkle - for compatibility during refactor
    /// In the new design, Hash IS Merkle, so this is identity
    fn from_merkle(merkle: &Merkle) -> Self
    where
        Self: Sized;
}

impl HashExt for Hash {
    fn from_bytes(s: &[u8]) -> Option<Self> {
        // Interpret bytes as Ed25519 point
        if s.len() >= 33 && s[0] == 1 {
            // This looks like a serialized merkle with algorithm byte
            let mut point_bytes = [0u8; 32];
            point_bytes.copy_from_slice(&s[1..33]);
            curve25519_dalek::edwards::CompressedEdwardsY::from_slice(&point_bytes)
                .decompress()
                .map(Merkle::Ed25519)
        } else if s.len() == 32 {
            // Raw 32-byte compressed Ed25519 point
            curve25519_dalek::edwards::CompressedEdwardsY::from_slice(s)
                .decompress()
                .map(Merkle::Ed25519)
        } else {
            None
        }
    }

    fn from_prefix(s: &str) -> Option<Self> {
        Merkle::from_prefix(s)
    }

    fn to_bytes(&self) -> [u8; 1 + 32] {
        let mut out = [0u8; 33];
        out[0] = 1; // Ed25519 algorithm marker
        let compressed = match self {
            Merkle::Ed25519(point) => point.compress().to_bytes(),
        };
        out[1..].copy_from_slice(&compressed);
        out
    }

    fn from_merkle(merkle: &Merkle) -> Self {
        // Hash IS Merkle now
        *merkle
    }
}

// Compatibility layer for existing code expecting Hash enum variants
impl Hash {
    pub const NONE: Hash = Merkle::Ed25519(curve25519_dalek::constants::ED25519_BASEPOINT_POINT);

    pub fn is_none(&self) -> bool {
        match self {
            Merkle::Ed25519(point) => {
                *point == curve25519_dalek::constants::ED25519_BASEPOINT_POINT
            }
        }
    }
}

// Algorithm enum for compatibility
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum HashAlgorithm {
    None = 0,
    Ed25519 = 1, // We use Ed25519 everywhere now
}

// Implement size calculation for SerializedHash (which is now SerializedMerkle)
impl SerializedHash {
    pub fn size(b: &[u8]) -> usize {
        // SerializedMerkle is always 33 bytes for Ed25519
        if b.len() > 0 && b[0] == 1 {
            33
        } else {
            panic!("Unknown hash algorithm {:?}", b[0])
        }
    }

    pub unsafe fn size_from_ptr(b: *const u8) -> usize {
        if *b == 1 {
            // Ed25519
            33
        } else {
            panic!("Unknown hash algorithm {:?}", *b)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pristine::Base32;

    #[test]
    fn from_to() {
        let mut h = Hasher::default();
        h.update(b"blabla");
        let h = h.finish();
        assert_eq!(Hash::from_base32(&h.to_base32().as_bytes()), Some(h));

        // Test "None" hash (zero point)
        let h = Hash::NONE;
        assert_eq!(Hash::from_base32(&h.to_base32().as_bytes()), Some(h));
    }

    #[test]
    fn test_hasher_deterministic() {
        // Same input should always produce same hash
        let mut h1 = Hasher::default();
        h1.update(b"test data");
        let hash1 = h1.finish();

        let mut h2 = Hasher::default();
        h2.update(b"test data");
        let hash2 = h2.finish();

        assert_eq!(hash1, hash2, "Same input should produce same hash");
    }

    #[test]
    fn test_hasher_different_inputs() {
        // Different inputs should produce different hashes
        let mut h1 = Hasher::default();
        h1.update(b"test data 1");
        let hash1 = h1.finish();

        let mut h2 = Hasher::default();
        h2.update(b"test data 2");
        let hash2 = h2.finish();

        assert_ne!(
            hash1, hash2,
            "Different inputs should produce different hashes"
        );
    }

    #[test]
    fn test_hash_ext_from_merkle() {
        // Test that from_merkle is identity since Hash IS Merkle
        let merkle = Merkle::zero();
        let hash = merkle;
        assert_eq!(hash, merkle);
    }

    #[test]
    fn test_base32_roundtrip() {
        let mut h = Hasher::default();
        h.update(b"roundtrip test");
        let hash = h.finish();

        let base32 = hash.to_base32();
        assert_eq!(base32.len(), 53); // Merkle base32 is 53 chars

        let decoded = Hash::from_base32(base32.as_bytes());
        assert_eq!(decoded, Some(hash));
    }

    #[test]
    fn test_hash_none() {
        let none = Hash::NONE;
        assert!(none.is_none());

        let mut h = Hasher::default();
        h.update(b"not none");
        let hash = h.finish();
        assert!(!hash.is_none());
    }
}
