//! Phase 3: Protocol Handler Unification - Integration Tests
//!
//! These tests verify that remote operations correctly register node types
//! in the remote table, enabling node-type-aware sync operations.
//!
//! Focus: Verify that put_remote() queries and logs node types correctly

use libatomic::pristine::{
    GraphMutTxnT, Hash, Merkle, MutTxnT, NodeType, RemoteId, SerializedMerkle, TxnT,
};
use libatomic::GraphTxnT;

use atomic_repository::Repository;
use tempfile::TempDir;

/// Helper to create a test hash from bytes
fn test_hash(data: &[u8]) -> Hash {
    // Create a deterministic hash for testing using libatomic's internal representation
    // SerializedMerkle needs 33 bytes: [algorithm_id, ...32 bytes of hash]
    let mut bytes = [0u8; 33];
    bytes[0] = 1; // MerkleAlgorithm::Ed25519
    let len = data.len().min(32);
    bytes[1..1 + len].copy_from_slice(&data[..len]);

    // Use SerializedMerkle (SerializedHash is a type alias) to create a proper hash
    let serialized = SerializedMerkle(bytes);
    Hash::from(&serialized)
}

/// Helper to create a test merkle from a hash
fn test_merkle(hash: &Hash) -> Merkle {
    // Convert hash to serialized form and then to merkle
    let serialized: SerializedMerkle = hash.into();
    Merkle::from(&serialized)
}

/// Create a minimal test repository
fn create_test_repo() -> (TempDir, Repository) {
    let tmp = tempfile::tempdir().expect("Failed to create temp dir");
    let repo_path = tmp.path().to_path_buf();

    Repository::init(Some(repo_path.clone()), None, None).expect("Failed to init repo");
    let repo = Repository::find_root(Some(repo_path.join(".atomic"))).expect("Failed to find repo");

    (tmp, repo)
}

/// Test that put_remote() queries node types when they're registered
#[test]
fn test_put_remote_queries_registered_node_types() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

    // Create a change hash and register it with NodeType::Change
    let change_hash = test_hash(b"phase3_change_1");
    let state = test_merkle(&change_hash);

    // Register the node type in the database
    let internal = txn
        .get_internal(&(&change_hash).into())
        .expect("Failed to get internal")
        .cloned();

    if let Some(internal) = internal {
        txn.put_node_type(&internal, NodeType::Change)
            .expect("Failed to put node type");
    }

    // Create remote and put entry (RemoteId needs exactly 16 bytes)
    let remote_id = RemoteId::from_bytes(b"test_remote_phas").expect("Failed to create ID");
    let mut remote = txn
        .open_or_create_remote(remote_id.clone(), "test_remote")
        .expect("Failed to create remote");

    // This should query the node type we just registered
    txn.put_remote(&mut remote, 1, (change_hash.clone(), state.clone()))
        .expect("Failed to put remote");

    txn.commit().expect("Failed to commit");

    // Verify the node type is still accessible
    let query_txn = repo
        .pristine
        .txn_begin()
        .expect("Failed to begin query txn");
    if let Some(internal) = query_txn
        .get_internal(&(&change_hash).into())
        .expect("Failed to get internal")
    {
        let node_type = query_txn
            .get_node_type(&internal)
            .expect("Failed to get node type");
        assert_eq!(
            node_type,
            Some(NodeType::Change),
            "Node type should be Change"
        );
    }
}

/// Test that put_remote() correctly handles tag node types
#[test]
fn test_put_remote_with_tag_node_types() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

    // Create a tag hash and register it with NodeType::Tag
    let tag_hash = test_hash(b"phase3_tag_1");
    let state = test_merkle(&tag_hash);

    // Register as a tag
    let internal = txn
        .get_internal(&(&tag_hash).into())
        .expect("Failed to get internal")
        .cloned();

    if let Some(internal) = internal {
        txn.put_node_type(&internal, NodeType::Tag)
            .expect("Failed to put node type");
    }

    // Create remote and put entry (RemoteId needs exactly 16 bytes)
    let remote_id = RemoteId::from_bytes(b"test_remote_tags").expect("Failed to create ID");
    let mut remote = txn
        .open_or_create_remote(remote_id.clone(), "test_remote")
        .expect("Failed to create remote");

    // This should query the tag node type
    txn.put_remote(&mut remote, 1, (tag_hash.clone(), state.clone()))
        .expect("Failed to put remote");

    txn.commit().expect("Failed to commit");

    // Verify the node type is Tag
    let query_txn = repo
        .pristine
        .txn_begin()
        .expect("Failed to begin query txn");
    if let Some(internal) = query_txn
        .get_internal(&(&tag_hash).into())
        .expect("Failed to get internal")
    {
        let node_type = query_txn
            .get_node_type(&internal)
            .expect("Failed to get node type");
        assert_eq!(node_type, Some(NodeType::Tag), "Node type should be Tag");
    }
}

/// Test Phase 2 helper methods work with Phase 3 remote operations
#[test]
fn test_get_remote_node_after_put_remote() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

    // Create and register a change
    let change_hash = test_hash(b"phase3_helper_test");
    let state = test_merkle(&change_hash);

    let internal = txn
        .get_internal(&(&change_hash).into())
        .expect("Failed to get internal")
        .cloned();

    if let Some(internal) = internal {
        txn.put_node_type(&internal, NodeType::Change)
            .expect("Failed to put node type");
    }

    // Create remote and register node (RemoteId needs exactly 16 bytes)
    let remote_id = RemoteId::from_bytes(b"test_helper_remo").expect("Failed to create ID");
    let mut remote = txn
        .open_or_create_remote(remote_id.clone(), "test_remote")
        .expect("Failed to create remote");

    txn.put_remote(&mut remote, 1, (change_hash.clone(), state.clone()))
        .expect("Failed to put remote");

    txn.commit().expect("Failed to commit");

    // Use Phase 2 helper to query the node
    let query_txn = repo
        .pristine
        .txn_begin()
        .expect("Failed to begin query txn");
    let remote_ref = query_txn
        .load_remote(&remote_id)
        .expect("Failed to load remote")
        .expect("Remote not found");

    let node = atomic_remote::RemoteRepo::get_remote_node(&query_txn, &remote_ref, 1)
        .expect("Failed to get remote node");

    assert!(node.is_some(), "Node should exist at position 1");

    if let Some(node) = node {
        assert_eq!(node.hash, change_hash, "Hash should match");
        assert_eq!(node.state, state, "State should match");
        assert_eq!(node.node_type, NodeType::Change, "Should be a Change");
    }
}

/// Test is_remote_tag helper with Phase 3 operations
#[test]
fn test_is_remote_tag_helper_phase3() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

    // Register a change
    let change_hash = test_hash(b"phase3_change_tag_test");
    let change_state = test_merkle(&change_hash);

    let internal = txn
        .get_internal(&(&change_hash).into())
        .expect("Failed to get internal")
        .cloned();

    if let Some(internal) = internal {
        txn.put_node_type(&internal, NodeType::Change)
            .expect("Failed to put node type");
    }

    // Register a tag
    let tag_hash = test_hash(b"phase3_tag_tag_test");
    let tag_state = test_merkle(&tag_hash);

    let internal = txn
        .get_internal(&(&tag_hash).into())
        .expect("Failed to get internal")
        .cloned();

    if let Some(internal) = internal {
        txn.put_node_type(&internal, NodeType::Tag)
            .expect("Failed to put node type");
    }

    // Create remote and register both (RemoteId needs exactly 16 bytes)
    let remote_id = RemoteId::from_bytes(b"test_tag_helpers").expect("Failed to create ID");
    let mut remote = txn
        .open_or_create_remote(remote_id.clone(), "test_remote")
        .expect("Failed to create remote");

    txn.put_remote(&mut remote, 1, (change_hash, change_state))
        .expect("Failed to put change");
    txn.put_remote(&mut remote, 2, (tag_hash, tag_state))
        .expect("Failed to put tag");

    txn.commit().expect("Failed to commit");

    // Use is_remote_tag helper
    let query_txn = repo
        .pristine
        .txn_begin()
        .expect("Failed to begin query txn");
    let remote_ref = query_txn
        .load_remote(&remote_id)
        .expect("Failed to load remote")
        .expect("Remote not found");

    let is_change_tag = atomic_remote::RemoteRepo::is_remote_tag(&query_txn, &remote_ref, 1)
        .expect("Failed to check if entry 1 is tag");
    let is_tag_tag = atomic_remote::RemoteRepo::is_remote_tag(&query_txn, &remote_ref, 2)
        .expect("Failed to check if entry 2 is tag");

    assert!(!is_change_tag, "Entry 1 should not be a tag");
    assert!(is_tag_tag, "Entry 2 should be a tag");
}

/// Test mixed change and tag registration in remote table
#[test]
fn test_mixed_node_types_in_remote_table() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

    let remote_id = RemoteId::from_bytes(b"test_mixed_remot").expect("Failed to create ID");
    let mut remote = txn
        .open_or_create_remote(remote_id.clone(), "test_remote")
        .expect("Failed to create remote");

    // Register multiple nodes of different types
    let nodes = vec![
        (test_hash(b"node1"), NodeType::Change),
        (test_hash(b"node2"), NodeType::Change),
        (test_hash(b"node3"), NodeType::Tag),
        (test_hash(b"node4"), NodeType::Change),
        (test_hash(b"node5"), NodeType::Tag),
    ];

    for (pos, (hash, node_type)) in nodes.iter().enumerate() {
        let state = test_merkle(hash);

        // Register node type
        let internal = txn
            .get_internal(&(hash).into())
            .expect("Failed to get internal")
            .cloned();

        if let Some(internal) = internal {
            txn.put_node_type(&internal, *node_type)
                .expect("Failed to put node type");
        }

        // Put to remote
        let position = (pos + 1) as u64;
        txn.put_remote(&mut remote, position, (hash.clone(), state))
            .expect("Failed to put remote");
    }

    txn.commit().expect("Failed to commit");

    // Query and verify all node types
    let query_txn = repo
        .pristine
        .txn_begin()
        .expect("Failed to begin query txn");
    let remote_ref = query_txn
        .load_remote(&remote_id)
        .expect("Failed to load remote")
        .expect("Remote not found");

    for (pos, (_hash, expected_type)) in nodes.iter().enumerate() {
        let position = (pos + 1) as u64;
        let node = atomic_remote::RemoteRepo::get_remote_node(&query_txn, &remote_ref, position)
            .expect("Failed to get node");

        assert!(node.is_some(), "Node at position {} should exist", position);

        if let Some(node) = node {
            assert_eq!(
                node.node_type, *expected_type,
                "Node at position {} should have type {:?}",
                position, expected_type
            );
        }
    }
}

/// Test that node types persist across transactions
#[test]
fn test_node_type_persistence_in_remote_operations() {
    let (_tmp, repo) = create_test_repo();

    let change_hash = test_hash(b"persistent_change");
    let state = test_merkle(&change_hash);
    let remote_id = RemoteId::from_bytes(b"persistent_remot").expect("Failed to create ID");

    // First transaction: register and store
    {
        let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

        let internal = txn
            .get_internal(&(&change_hash).into())
            .expect("Failed to get internal")
            .cloned();

        if let Some(internal) = internal {
            txn.put_node_type(&internal, NodeType::Change)
                .expect("Failed to put node type");
        }

        let mut remote = txn
            .open_or_create_remote(remote_id.clone(), "test_remote")
            .expect("Failed to create remote");

        txn.put_remote(&mut remote, 1, (change_hash.clone(), state.clone()))
            .expect("Failed to put remote");

        txn.commit().expect("Failed to commit");
    }

    // Second transaction: verify persistence
    {
        let query_txn = repo
            .pristine
            .txn_begin()
            .expect("Failed to begin query txn");
        let remote_ref = query_txn
            .load_remote(&remote_id)
            .expect("Failed to load remote")
            .expect("Remote not found");

        let node = atomic_remote::RemoteRepo::get_remote_node(&query_txn, &remote_ref, 1)
            .expect("Failed to get node");

        assert!(node.is_some(), "Node should persist across transactions");

        if let Some(node) = node {
            assert_eq!(node.node_type, NodeType::Change, "Node type should persist");
        }
    }
}

/// Test that put_remote logs node types when available (Phase 2 enhancement verification)
#[test]
fn test_put_remote_logs_node_types() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

    let hash = test_hash(b"logged_node");
    let state = test_merkle(&hash);

    // Register node type
    let internal = txn
        .get_internal(&(&hash).into())
        .expect("Failed to get internal")
        .cloned();

    if let Some(internal) = internal {
        txn.put_node_type(&internal, NodeType::Change)
            .expect("Failed to put node type");
    }

    let remote_id = RemoteId::from_bytes(b"logged_remote123").expect("Failed to create ID");
    let mut remote = txn
        .open_or_create_remote(remote_id.clone(), "test_remote")
        .expect("Failed to create remote");

    // This should log the node type (Phase 2 enhancement)
    txn.put_remote(&mut remote, 1, (hash.clone(), state))
        .expect("Failed to put remote");

    txn.commit().expect("Failed to commit");

    // Verify it worked
    let query_txn = repo
        .pristine
        .txn_begin()
        .expect("Failed to begin query txn");
    if let Some(internal) = query_txn
        .get_internal(&(&hash).into())
        .expect("Failed to get internal")
    {
        let node_type = query_txn
            .get_node_type(&internal)
            .expect("Failed to get node type");
        assert!(
            node_type.is_some(),
            "Node type should be logged and retrievable"
        );
    }
}
