//! Node-Type-Aware System Tests
//!
//! Following AGENTS.md testing strategy patterns for comprehensive coverage
//! of the node-type-aware refactoring.

use atomic_remote::Node;
use libatomic::pristine::{Base32, Hash, Hasher, Merkle, NodeType};

// Helper to create test hashes
fn test_hash(data: &[u8]) -> Hash {
    let mut hasher = Hasher::default();
    hasher.update(data);
    hasher.finish()
}

// Helper to create test merkle from hash
fn test_merkle(hash: &Hash) -> Merkle {
    Merkle::from(*hash)
}

#[test]
fn test_node_creation_change() {
    let hash = test_hash(b"test_change");
    let state = test_merkle(&hash);

    let node = Node::change(hash.clone(), state.clone());

    assert_eq!(node.hash, hash);
    assert_eq!(node.state, state);
    assert_eq!(node.node_type, NodeType::Change);
    assert!(node.is_change());
    assert!(!node.is_tag());
}

#[test]
fn test_node_creation_tag() {
    let hash = test_hash(b"test_tag");
    let state = test_merkle(&hash);

    let node = Node::tag(hash.clone(), state.clone());

    assert_eq!(node.hash, hash);
    assert_eq!(node.state, state);
    assert_eq!(node.node_type, NodeType::Tag);
    assert!(node.is_tag());
    assert!(!node.is_change());
}

#[test]
fn test_node_type_marker() {
    let hash = test_hash(b"test_marker");
    let state = test_merkle(&hash);

    let change_node = Node::change(hash.clone(), state.clone());
    assert_eq!(change_node.type_marker(), "C");

    let tag_node = Node::tag(hash.clone(), state.clone());
    assert_eq!(tag_node.type_marker(), "T");
}

#[test]
fn test_node_from_type_marker_change() {
    let hash = test_hash(b"test_from_marker_change");
    let state = test_merkle(&hash);

    let node = Node::from_type_marker(hash.clone(), state.clone(), "C").expect("Valid type marker");

    assert_eq!(node.node_type, NodeType::Change);
    assert!(node.is_change());
}

#[test]
fn test_node_from_type_marker_tag() {
    let hash = test_hash(b"test_from_marker_tag");
    let state = test_merkle(&hash);

    let node = Node::from_type_marker(hash.clone(), state.clone(), "T").expect("Valid type marker");

    assert_eq!(node.node_type, NodeType::Tag);
    assert!(node.is_tag());
}

#[test]
fn test_node_from_type_marker_invalid() {
    let hash = test_hash(b"test_invalid_marker");
    let state = test_merkle(&hash);

    let result = Node::from_type_marker(hash, state, "X");
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid node type marker"));
}

#[test]
fn test_node_equality() {
    let hash1 = test_hash(b"test_equality");
    let hash2 = test_hash(b"test_equality");
    let state = test_merkle(&hash1);

    let node1 = Node::change(hash1, state.clone());
    let node2 = Node::change(hash2, state);

    assert_eq!(node1, node2);
}

#[test]
fn test_node_inequality_different_types() {
    let hash = test_hash(b"test_inequality_types");
    let state = test_merkle(&hash);

    let change_node = Node::change(hash.clone(), state.clone());
    let tag_node = Node::tag(hash, state);

    assert_ne!(change_node, tag_node);
}

#[test]
fn test_node_inequality_different_hashes() {
    let hash1 = test_hash(b"test_hash_1");
    let hash2 = test_hash(b"test_hash_2");
    let state = test_merkle(&hash1);

    let node1 = Node::change(hash1, state.clone());
    let node2 = Node::change(hash2, state);

    assert_ne!(node1, node2);
}

#[test]
fn test_node_clone() {
    let hash = test_hash(b"test_clone");
    let state = test_merkle(&hash);

    let node1 = Node::tag(hash, state);
    let node2 = node1.clone();

    assert_eq!(node1, node2);
    assert_eq!(node1.hash, node2.hash);
    assert_eq!(node1.state, node2.state);
    assert_eq!(node1.node_type, node2.node_type);
}

#[test]
fn test_node_debug_format() {
    let hash = test_hash(b"test_debug");
    let state = test_merkle(&hash);

    let node = Node::change(hash, state);
    let debug_str = format!("{:?}", node);

    // Verify debug output contains key information
    assert!(debug_str.contains("Node"));
    assert!(debug_str.contains("hash"));
    assert!(debug_str.contains("node_type"));
    assert!(debug_str.contains("state"));
}

#[test]
fn test_node_hash_trait() {
    use std::collections::HashSet;

    let hash = test_hash(b"test_hash_trait");
    let state = test_merkle(&hash);

    let mut set = HashSet::new();
    let node1 = Node::change(hash.clone(), state.clone());
    let node2 = Node::change(hash, state);

    set.insert(node1);
    assert!(set.contains(&node2));
}

// CS enum has been removed in Phase 4 - Node is now the unified type
// These tests verified conversion from Node to CS, which is no longer needed

#[test]
fn test_node_change_properties() {
    let hash = test_hash(b"test_change");
    let state = test_merkle(&hash);

    let node = Node::change(hash.clone(), state.clone());

    assert!(node.is_change());
    assert!(!node.is_tag());
    assert_eq!(node.hash, hash);
    assert_eq!(node.state, state);
}

#[test]
fn test_node_tag_properties() {
    let hash = test_hash(b"test_tag");
    let state = test_merkle(&hash);

    let node = Node::tag(hash.clone(), state.clone());

    assert!(node.is_tag());
    assert!(!node.is_change());
    assert_eq!(node.hash, hash);
    assert_eq!(node.state, state);
}

#[test]
fn test_changelist_entry_format() {
    let hash = test_hash(b"test_changelist");
    let state = test_merkle(&hash);

    let change_node = Node::change(hash.clone(), state.clone());
    let tag_node = Node::tag(hash.clone(), state.clone());

    // Format as changelist entry
    let change_entry = format!(
        "42.{}.{}.{}",
        hash.to_base32(),
        state.to_base32(),
        change_node.type_marker()
    );

    let tag_entry = format!(
        "43.{}.{}.{}",
        hash.to_base32(),
        state.to_base32(),
        tag_node.type_marker()
    );

    assert!(change_entry.contains(".C"));
    assert!(tag_entry.contains(".T"));
    assert!(change_entry.starts_with("42."));
    assert!(tag_entry.starts_with("43."));
}

#[test]
fn test_protocol_version_updated() {
    use atomic_remote::PROTOCOL_VERSION;

    // Following AGENTS.md: Protocol version should be 4 for node-type-aware system
    assert_eq!(PROTOCOL_VERSION, 4);
}

// Note: Integration tests that require database access should be in separate
// integration test files with full repository setup. These unit tests focus
// on the Node structure itself without database dependencies.

#[cfg(test)]
mod property_tests {
    //! Property-based testing following AGENTS.md patterns
    use super::*;

    #[test]
    fn test_node_type_roundtrip() {
        // Property: Node type marker -> Node -> type marker should be identity
        let hash = test_hash(b"test_roundtrip");
        let state = test_merkle(&hash);

        for marker in &["C", "T"] {
            let node =
                Node::from_type_marker(hash.clone(), state.clone(), marker).expect("Valid marker");
            assert_eq!(node.type_marker(), *marker);
        }
    }

    #[test]
    fn test_node_type_exclusivity() {
        // Property: A node is either a change OR a tag, never both
        let hash = test_hash(b"test_exclusivity");
        let state = test_merkle(&hash);

        let change_node = Node::change(hash.clone(), state.clone());
        assert!(change_node.is_change() != change_node.is_tag());

        let tag_node = Node::tag(hash, state);
        assert!(tag_node.is_tag() != tag_node.is_change());
    }

    #[test]
    fn test_node_equality_reflexive() {
        // Property: node == node (reflexive)
        let hash = test_hash(b"test_reflexive");
        let state = test_merkle(&hash);

        let node = Node::change(hash, state);
        assert_eq!(node, node);
    }

    #[test]
    fn test_node_equality_symmetric() {
        // Property: if node1 == node2, then node2 == node1 (symmetric)
        let hash = test_hash(b"test_symmetric");
        let state = test_merkle(&hash);

        let node1 = Node::tag(hash.clone(), state.clone());
        let node2 = Node::tag(hash, state);

        if node1 == node2 {
            assert_eq!(node2, node1);
        }
    }
}
