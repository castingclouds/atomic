//! Phase 2 Integration Tests: Remote Table Node Type Tracking
//!
//! Tests that verify the remote table correctly stores and retrieves node types
//! following AGENTS.md testing patterns.

use atomic_remote::{Node, RemoteRepo};
use atomic_repository::Repository;
use libatomic::pristine::{
    Base32, GraphMutTxnT, GraphTxnT, Hash, Hasher, Merkle, MutTxnT, NodeType, RemoteId, TxnT,
};
use libatomic::ChannelMutTxnT;
use std::path::PathBuf;
use tempfile::TempDir;

// Helper to create a test repository
fn create_test_repo() -> (TempDir, Repository) {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("test_repo");

    // Initialize repository in subdirectory to avoid "already in repo" error
    std::fs::create_dir_all(&repo_path).expect("Failed to create repo dir");

    // Change to the temp directory to avoid being inside the atomic repo
    let original_dir = std::env::current_dir().expect("Failed to get current dir");
    std::env::set_current_dir(&repo_path).expect("Failed to change dir");

    let repo = Repository::init(None, None, Some(".")).expect("Failed to init repo");

    // Change back to original directory
    std::env::set_current_dir(original_dir).expect("Failed to restore dir");

    (tmp, repo)
}

// Helper to create test hash
fn test_hash(data: &[u8]) -> Hash {
    let mut hasher = Hasher::default();
    hasher.update(data);
    hasher.finish()
}

// Helper to create test merkle
fn test_merkle(hash: &Hash) -> Merkle {
    Merkle::from(*hash)
}

#[test]
fn test_put_remote_with_change_node_type() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

    // Create a change hash and register it as a change
    let hash = test_hash(b"test_change_1");
    let state = test_merkle(&hash);
    let internal = txn
        .get_internal(&(&hash).into())
        .expect("Failed to get internal")
        .cloned();

    // Register as change node (this would normally be done during record)
    if let Some(internal_id) = internal {
        txn.put_node_type(&internal_id, NodeType::Change)
            .expect("Failed to register node type");
    }

    // Create remote
    let remote_id = RemoteId::from_bytes(b"test_remote_id_1").unwrap();
    let mut remote = txn
        .open_or_create_remote(remote_id, "/tmp/test")
        .expect("Failed to create remote");

    // Put to remote table
    let position = 1u64;
    txn.put_remote(&mut remote, position, (hash.clone(), state.clone()))
        .expect("Failed to put remote");

    // Verify node type is accessible
    if let Some(node_type) = txn.get_node_type_by_hash(&hash) {
        assert_eq!(node_type, NodeType::Change);
    } else {
        panic!("Node type should be registered");
    }

    txn.commit().expect("Failed to commit");
}

#[test]
fn test_put_remote_with_tag_node_type() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

    // Create a tag hash and register it as a tag
    let hash = test_hash(b"test_tag_1");
    let state = test_merkle(&hash);
    let internal = txn
        .get_internal(&(&hash).into())
        .expect("Failed to get internal")
        .cloned();

    // Register as tag node (this would normally be done during tag creation)
    if let Some(internal_id) = internal {
        txn.put_node_type(&internal_id, NodeType::Tag)
            .expect("Failed to register node type");
    }

    // Create remote
    let remote_id = RemoteId::from_bytes(b"test_remote_id_2").unwrap();
    let mut remote = txn
        .open_or_create_remote(remote_id, "/tmp/test")
        .expect("Failed to create remote");

    // Put to remote table
    let position = 1u64;
    txn.put_remote(&mut remote, position, (hash.clone(), state.clone()))
        .expect("Failed to put remote");

    // Also put to tags table (as the protocol does)
    txn.put_tags(&mut remote.lock().tags, position, &state)
        .expect("Failed to put tags");

    // Verify node type is accessible
    if let Some(node_type) = txn.get_node_type_by_hash(&hash) {
        assert_eq!(node_type, NodeType::Tag);
    } else {
        panic!("Node type should be registered");
    }

    txn.commit().expect("Failed to commit");
}

#[test]
fn test_get_remote_node_returns_correct_type() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

    // Create and register a change
    let change_hash = test_hash(b"change_for_remote");
    let change_state = test_merkle(&change_hash);

    if let Some(internal) = txn.get_internal(&(&change_hash).into()).unwrap().cloned() {
        txn.put_node_type(&internal, NodeType::Change).unwrap();
    }

    // Create remote and put change
    let remote_id = RemoteId::from_bytes(b"test_remote_id_3").unwrap();
    let mut remote = txn.open_or_create_remote(remote_id, "/tmp/test").unwrap();

    txn.put_remote(&mut remote, 1, (change_hash.clone(), change_state.clone()))
        .unwrap();

    // Get node using helper method
    let node = RemoteRepo::get_remote_node(&txn, &remote, 1)
        .expect("Failed to get remote node")
        .expect("Node should exist");

    assert_eq!(node.hash, change_hash);
    assert_eq!(node.state, change_state);
    assert_eq!(node.node_type, NodeType::Change);
    assert!(node.is_change());
    assert!(!node.is_tag());

    txn.commit().unwrap();
}

#[test]
fn test_get_remote_node_with_tag_type() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

    // Create and register a tag
    let tag_hash = test_hash(b"tag_for_remote");
    let tag_state = test_merkle(&tag_hash);

    if let Some(internal) = txn.get_internal(&(&tag_hash).into()).unwrap().cloned() {
        txn.put_node_type(&internal, NodeType::Tag).unwrap();
    }

    // Create remote and put tag
    let remote_id = RemoteId::from_bytes(b"test_remote_id_4").unwrap();
    let mut remote = txn.open_or_create_remote(remote_id, "/tmp/test").unwrap();

    txn.put_remote(&mut remote, 1, (tag_hash.clone(), tag_state.clone()))
        .unwrap();

    txn.put_tags(&mut remote.lock().tags, 1, &tag_state)
        .unwrap();

    // Get node using helper method
    let node = RemoteRepo::get_remote_node(&txn, &remote, 1)
        .expect("Failed to get remote node")
        .expect("Node should exist");

    assert_eq!(node.hash, tag_hash);
    assert_eq!(node.state, tag_state);
    assert_eq!(node.node_type, NodeType::Tag);
    assert!(node.is_tag());
    assert!(!node.is_change());

    txn.commit().unwrap();
}

#[test]
fn test_is_remote_tag_helper() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

    // Create a change and a tag
    let change_hash = test_hash(b"change_node");
    let change_state = test_merkle(&change_hash);
    let tag_hash = test_hash(b"tag_node");
    let tag_state = test_merkle(&tag_hash);

    // Register types
    if let Some(internal) = txn.get_internal(&(&change_hash).into()).unwrap().cloned() {
        txn.put_node_type(&internal, NodeType::Change).unwrap();
    }
    if let Some(internal) = txn.get_internal(&(&tag_hash).into()).unwrap().cloned() {
        txn.put_node_type(&internal, NodeType::Tag).unwrap();
    }

    // Create remote
    let remote_id = RemoteId::from_bytes(b"test_remote_id_5").unwrap();
    let mut remote = txn.open_or_create_remote(remote_id, "/tmp/test").unwrap();

    // Put both to remote
    txn.put_remote(&mut remote, 1, (change_hash, change_state))
        .unwrap();
    txn.put_remote(&mut remote, 2, (tag_hash, tag_state.clone()))
        .unwrap();
    txn.put_tags(&mut remote.lock().tags, 2, &tag_state)
        .unwrap();

    // Test helper
    assert!(!RemoteRepo::is_remote_tag(&txn, &remote, 1).unwrap());
    assert!(RemoteRepo::is_remote_tag(&txn, &remote, 2).unwrap());

    txn.commit().unwrap();
}

#[test]
fn test_multiple_remote_entries_with_mixed_types() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

    let remote_id = RemoteId::from_bytes(b"test_remote_id_6").unwrap();
    let mut remote = txn.open_or_create_remote(remote_id, "/tmp/test").unwrap();

    // Create multiple nodes with different types
    let nodes = vec![
        (test_hash(b"change_1"), NodeType::Change),
        (test_hash(b"change_2"), NodeType::Change),
        (test_hash(b"tag_1"), NodeType::Tag),
        (test_hash(b"change_3"), NodeType::Change),
        (test_hash(b"tag_2"), NodeType::Tag),
    ];

    // Register and put to remote
    for (pos, (hash, node_type)) in nodes.iter().enumerate() {
        let state = test_merkle(hash);

        if let Some(internal) = txn.get_internal(&(hash).into()).unwrap().cloned() {
            txn.put_node_type(&internal, *node_type).unwrap();
        }

        let position = (pos + 1) as u64;
        txn.put_remote(&mut remote, position, (hash.clone(), state.clone()))
            .unwrap();

        if *node_type == NodeType::Tag {
            txn.put_tags(&mut remote.lock().tags, position, &state)
                .unwrap();
        }
    }

    // Verify all nodes have correct types
    for (pos, (hash, expected_type)) in nodes.iter().enumerate() {
        let position = (pos + 1) as u64;
        let node = RemoteRepo::get_remote_node(&txn, &remote, position)
            .unwrap()
            .expect("Node should exist");

        assert_eq!(node.hash, *hash);
        assert_eq!(node.node_type, *expected_type);
    }

    txn.commit().unwrap();
}

#[test]
fn test_get_remote_node_nonexistent_position() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().expect("Failed to begin txn");

    let remote_id = RemoteId::from_bytes(b"test_remote_id_x").unwrap();
    let remote = txn
        .open_or_create_remote(remote_id, "/tmp/test")
        .expect("Failed to create remote");

    // Try to get node from nonexistent position
    let result = RemoteRepo::get_remote_node(&txn, &remote, 999);

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());

    txn.commit().unwrap();
}

#[test]
fn test_remote_node_type_persistence_across_transactions() {
    let (_tmp, repo) = create_test_repo();

    let hash = test_hash(b"persistent_change");
    let state = test_merkle(&hash);

    // First transaction: register and put
    {
        let mut txn = repo.pristine.mut_txn_begin().unwrap();

        if let Some(internal) = txn.get_internal(&(&hash).into()).unwrap().cloned() {
            txn.put_node_type(&internal, NodeType::Change).unwrap();
        }

        let remote_id = RemoteId::from_bytes(b"test_remote_id_7").unwrap();
        let mut remote = txn.open_or_create_remote(remote_id, "/tmp/test").unwrap();

        txn.put_remote(&mut remote, 1, (hash.clone(), state.clone()))
            .unwrap();

        txn.commit().unwrap();
    }

    // Second transaction: verify persistence
    {
        let mut txn = repo.pristine.mut_txn_begin().unwrap();
        let remote_id = RemoteId::from_bytes(b"test_remote_id_7").unwrap();
        let remote = txn.get_remote(remote_id).unwrap().unwrap();

        let node = RemoteRepo::get_remote_node(&txn, &remote, 1)
            .unwrap()
            .expect("Node should exist");

        assert_eq!(node.hash, hash);
        assert_eq!(node.state, state);
        assert_eq!(node.node_type, NodeType::Change);
    }
}

#[test]
fn test_node_type_helpers_in_txn_trait() {
    let (_tmp, repo) = create_test_repo();
    let mut txn = repo.pristine.mut_txn_begin().unwrap();

    // Create test nodes
    let change_hash = test_hash(b"test_change_node");
    let tag_hash = test_hash(b"test_tag_node");

    // Register types using register_node helper
    let change_internal = txn.get_internal(&(&change_hash).into()).unwrap().cloned();
    if let Some(internal) = change_internal {
        libatomic::pristine::register_node(
            &mut txn,
            &internal,
            &change_hash,
            NodeType::Change,
            &[],
        )
        .unwrap();
    }

    let tag_internal = txn.get_internal(&(&tag_hash).into()).unwrap().cloned();
    if let Some(internal) = tag_internal {
        libatomic::pristine::register_node(&mut txn, &internal, &tag_hash, NodeType::Tag, &[])
            .unwrap();
    }

    // Test helper methods
    assert!(txn.is_change_node(&change_hash));
    assert!(!txn.is_tag_node(&change_hash));

    assert!(txn.is_tag_node(&tag_hash));
    assert!(!txn.is_change_node(&tag_hash));

    assert_eq!(
        txn.get_node_type_by_hash(&change_hash),
        Some(NodeType::Change)
    );
    assert_eq!(txn.get_node_type_by_hash(&tag_hash), Some(NodeType::Tag));

    txn.commit().unwrap();
}
