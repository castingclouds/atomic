//! Integration tests for change registration with node types

use libatomic::change::{Change, ChangeHeader};
use libatomic::pristine::{Base32, NodeId, GraphMutTxnT, GraphTxnT, Hash, MutTxnT, NodeType};
use tempfile::tempdir;

#[test]
fn test_register_change_sets_node_type() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    // Initialize database version
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.commit().unwrap();
    }

    let change_id = NodeId(::sanakirja::L64(1));
    let hash = Hash::from_base32(b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap();

    // Create a minimal change
    let change = Change {
        offsets: libatomic::change::Offsets::default(),
        hashed: libatomic::change::Hashed {
            version: 1,
            header: ChangeHeader {
                message: "Test change".to_string(),
                authors: vec![],
                timestamp: chrono::Utc::now(),
                description: None,
            },
            dependencies: vec![],
            extra_known: vec![],
            metadata: vec![],
            changes: vec![],
            contents_hash: libatomic::pristine::Merkle::zero(),
            tag: None,
        },
        unhashed: None,
        contents: vec![],
    };

    // Register the change
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        libatomic::pristine::register_change(&mut txn, &change_id, &hash, &change).unwrap();
        txn.commit().unwrap();
    }

    // Verify node type was set to Change
    {
        let txn = pristine.txn_begin().unwrap();
        let node_type = txn.get_node_type(&change_id).unwrap();
        assert_eq!(node_type, Some(NodeType::Change));
    }
}

#[test]
fn test_register_multiple_changes() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    // Initialize database version
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.commit().unwrap();
    }

    let change_id_1 = NodeId(::sanakirja::L64(1));
    let change_id_2 = NodeId(::sanakirja::L64(2));
    let hash_1 =
        Hash::from_base32(b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap();
    let hash_2 = Hash::from_base32(b"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB").unwrap();

    let change_1 = Change {
        offsets: libatomic::change::Offsets::default(),
        hashed: libatomic::change::Hashed {
            version: 1,
            header: ChangeHeader {
                message: "First change".to_string(),
                authors: vec![],
                timestamp: chrono::Utc::now(),
                description: None,
            },
            dependencies: vec![],
            extra_known: vec![],
            metadata: vec![],
            changes: vec![],
            contents_hash: libatomic::pristine::Merkle::zero(),
            tag: None,
        },
        unhashed: None,
        contents: vec![],
    };

    let change_2 = Change {
        offsets: libatomic::change::Offsets::default(),
        hashed: libatomic::change::Hashed {
            version: 1,
            header: ChangeHeader {
                message: "Second change".to_string(),
                authors: vec![],
                timestamp: chrono::Utc::now(),
                description: None,
            },
            dependencies: vec![hash_1], // Depends on first change
            extra_known: vec![],
            metadata: vec![],
            changes: vec![],
            contents_hash: libatomic::pristine::Merkle::zero(),
            tag: None,
        },
        unhashed: None,
        contents: vec![],
    };

    // Register both changes
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        libatomic::pristine::register_change(&mut txn, &change_id_1, &hash_1, &change_1).unwrap();
        libatomic::pristine::register_change(&mut txn, &change_id_2, &hash_2, &change_2).unwrap();
        txn.commit().unwrap();
    }

    // Verify both node types are Change
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
    }
}

#[test]
fn test_register_change_with_internal_external_mapping() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    // Initialize database version
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.commit().unwrap();
    }

    let change_id = NodeId(::sanakirja::L64(42));
    let hash = Hash::from_base32(b"CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC").unwrap();

    let change = Change {
        offsets: libatomic::change::Offsets::default(),
        hashed: libatomic::change::Hashed {
            version: 1,
            header: ChangeHeader {
                message: "Test change with mapping".to_string(),
                authors: vec![],
                timestamp: chrono::Utc::now(),
                description: None,
            },
            dependencies: vec![],
            extra_known: vec![],
            metadata: vec![],
            changes: vec![],
            contents_hash: libatomic::pristine::Merkle::zero(),
            tag: None,
        },
        unhashed: None,
        contents: vec![],
    };

    // Register the change
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        libatomic::pristine::register_change(&mut txn, &change_id, &hash, &change).unwrap();
        txn.commit().unwrap();
    }

    // Verify all three things are set: external, internal, and node_type
    {
        let txn = pristine.txn_begin().unwrap();

        // Check external mapping (internal -> hash)
        let external = txn.get_external(&change_id).unwrap();
        assert!(external.is_some());

        // Check internal mapping (hash -> internal)
        let shash = (&hash).into();
        let internal = txn.get_internal(&shash).unwrap();
        assert_eq!(internal, Some(&change_id));

        // Check node type
        let node_type = txn.get_node_type(&change_id).unwrap();
        assert_eq!(node_type, Some(NodeType::Change));
    }
}
