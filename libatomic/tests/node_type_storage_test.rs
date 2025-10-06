//! Integration tests for NodeType storage in Sanakirja database

use libatomic::pristine::{NodeId, GraphMutTxnT, GraphTxnT, MutTxnT, NodeType};
use tempfile::tempdir;

#[test]
fn test_db_with_node_types_table() {
    // Verify database initialization doesn't crash with node_types table
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    // Initialize database version
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.commit().unwrap();
    }

    // Open a transaction
    let txn = pristine.txn_begin().unwrap();

    // If we got here, the database initialized successfully
    drop(txn);
}

#[test]
fn test_store_and_retrieve_node_type_change() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    let change_id = NodeId(::sanakirja::L64(42));

    // Store a Change node type
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.put_node_type(&change_id, NodeType::Change).unwrap();
        txn.commit().unwrap();
    }

    // Retrieve and verify
    {
        let txn = pristine.txn_begin().unwrap();
        let node_type = txn.get_node_type(&change_id).unwrap();
        assert_eq!(node_type, Some(NodeType::Change));
    }
}

#[test]
fn test_store_and_retrieve_node_type_tag() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    let tag_id = NodeId(::sanakirja::L64(100));

    // Store a Tag node type
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.put_node_type(&tag_id, NodeType::Tag).unwrap();
        txn.commit().unwrap();
    }

    // Retrieve and verify
    {
        let txn = pristine.txn_begin().unwrap();
        let node_type = txn.get_node_type(&tag_id).unwrap();
        assert_eq!(node_type, Some(NodeType::Tag));
    }
}

#[test]
fn test_node_type_not_found() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    // Initialize database version
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.commit().unwrap();
    }

    let txn = pristine.txn_begin().unwrap();
    let nonexistent_id = NodeId(::sanakirja::L64(999));

    // Query for non-existent ID should return None
    let node_type = txn.get_node_type(&nonexistent_id).unwrap();
    assert_eq!(node_type, None);
}

#[test]
fn test_multiple_node_types() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    let change_id_1 = NodeId(::sanakirja::L64(1));
    let change_id_2 = NodeId(::sanakirja::L64(2));
    let tag_id_1 = NodeId(::sanakirja::L64(100));
    let tag_id_2 = NodeId(::sanakirja::L64(101));

    // Store multiple different node types
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.put_node_type(&change_id_1, NodeType::Change).unwrap();
        txn.put_node_type(&change_id_2, NodeType::Change).unwrap();
        txn.put_node_type(&tag_id_1, NodeType::Tag).unwrap();
        txn.put_node_type(&tag_id_2, NodeType::Tag).unwrap();
        txn.commit().unwrap();
    }

    // Verify all are stored correctly
    {
        let txn = pristine.txn_begin().unwrap();
        assert_eq!(
            txn.get_node_type(&change_id_1).unwrap(),
            Some(NodeType::Change)
        );
        assert_eq!(
            txn.get_node_type(&change_id_2).unwrap(),
            Some(NodeType::Change)
        );
        assert_eq!(txn.get_node_type(&tag_id_1).unwrap(), Some(NodeType::Tag));
        assert_eq!(txn.get_node_type(&tag_id_2).unwrap(), Some(NodeType::Tag));
    }
}

#[test]
fn test_update_node_type() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    let node_id = NodeId(::sanakirja::L64(50));

    // Store as Change
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.put_node_type(&node_id, NodeType::Change).unwrap();
        txn.commit().unwrap();
    }

    // Verify it's a Change
    {
        let txn = pristine.txn_begin().unwrap();
        assert_eq!(txn.get_node_type(&node_id).unwrap(), Some(NodeType::Change));
    }

    // Update to Tag
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.put_node_type(&node_id, NodeType::Tag).unwrap();
        txn.commit().unwrap();
    }

    // Verify it's now a Tag
    {
        let txn = pristine.txn_begin().unwrap();
        assert_eq!(txn.get_node_type(&node_id).unwrap(), Some(NodeType::Tag));
    }
}

#[test]
fn test_delete_node_type() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    let node_id = NodeId(::sanakirja::L64(75));

    // Store a node type
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.put_node_type(&node_id, NodeType::Change).unwrap();
        txn.commit().unwrap();
    }

    // Verify it exists
    {
        let txn = pristine.txn_begin().unwrap();
        assert_eq!(txn.get_node_type(&node_id).unwrap(), Some(NodeType::Change));
    }

    // Delete it
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        let deleted = txn.del_node_type(&node_id).unwrap();
        assert!(deleted, "Should return true when deleting existing entry");
        txn.commit().unwrap();
    }

    // Verify it's gone
    {
        let txn = pristine.txn_begin().unwrap();
        assert_eq!(txn.get_node_type(&node_id).unwrap(), None);
    }
}

#[test]
fn test_delete_nonexistent_node_type() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    let mut txn = pristine.mut_txn_begin().unwrap();
    let nonexistent_id = NodeId(::sanakirja::L64(999));

    // Deleting non-existent entry should return false
    let deleted = txn.del_node_type(&nonexistent_id).unwrap();
    assert!(
        !deleted,
        "Should return false when deleting non-existent entry"
    );
}

#[test]
fn test_node_type_persistence_across_transactions() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    let change_id = NodeId(::sanakirja::L64(10));
    let tag_id = NodeId(::sanakirja::L64(20));

    // Transaction 1: Store node types
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.put_node_type(&change_id, NodeType::Change).unwrap();
        txn.put_node_type(&tag_id, NodeType::Tag).unwrap();
        txn.commit().unwrap();
    }

    // Transaction 2: Read them back
    {
        let txn = pristine.txn_begin().unwrap();
        assert_eq!(
            txn.get_node_type(&change_id).unwrap(),
            Some(NodeType::Change)
        );
        assert_eq!(txn.get_node_type(&tag_id).unwrap(), Some(NodeType::Tag));
    }

    // Transaction 3: Modify one
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.put_node_type(&change_id, NodeType::Tag).unwrap();
        txn.commit().unwrap();
    }

    // Transaction 4: Verify changes persisted
    {
        let txn = pristine.txn_begin().unwrap();
        assert_eq!(txn.get_node_type(&change_id).unwrap(), Some(NodeType::Tag));
        assert_eq!(txn.get_node_type(&tag_id).unwrap(), Some(NodeType::Tag));
    }
}
