//! Test for unified header loading (get_header_by_hash)
//!
//! This test verifies that the get_header_by_hash function correctly:
//! 1. Loads headers for regular changes
//! 2. Compiles and has the correct signature
//! 3. Integrates with the existing changestore trait

use libatomic::change::{Change, ChangeHeader};
use libatomic::changestore::memory::Memory;
use libatomic::changestore::ChangeStore;
use libatomic::pristine::{
    get_header_by_hash, Base32, NodeId, GraphTxnT, Hash, Merkle, MutTxnT, NodeType,
};
use tempfile::tempdir;

#[test]
fn test_get_header_by_hash_for_change() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    // Initialize database
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.commit().unwrap();
    }

    // Create a minimal change
    let mut change = Change {
        offsets: libatomic::change::Offsets::default(),
        hashed: libatomic::change::Hashed {
            version: 1,
            header: ChangeHeader {
                message: "Test change message".to_string(),
                authors: vec![],
                timestamp: chrono::Utc::now(),
                description: None,
            },
            dependencies: vec![],
            extra_known: vec![],
            metadata: vec![],
            changes: vec![],
            contents_hash: Merkle::zero(),
            tag: None,
        },
        unhashed: None,
        contents: vec![],
    };

    // Store change in changestore using save_change - this computes the hash
    let changes = Memory::new();
    let hash = changes
        .save_change(&mut change, |_, _| {
            Ok::<(), libatomic::changestore::memory::Error>(())
        })
        .unwrap();

    let change_id = NodeId(::sanakirja::L64(1));

    // Register the change
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        libatomic::pristine::register_change(&mut txn, &change_id, &hash, &change).unwrap();
        txn.commit().unwrap();
    }

    // Verify node type was set
    {
        let txn = pristine.txn_begin().unwrap();
        assert_eq!(
            txn.get_node_type(&change_id).unwrap(),
            Some(NodeType::Change)
        );
    }

    // Test get_header_by_hash
    {
        let txn = pristine.txn_begin().unwrap();
        let header = get_header_by_hash(&txn, &changes, &hash).unwrap();

        assert_eq!(header.message, "Test change message");
        println!("✅ Successfully loaded change header via get_header_by_hash");
    }
}

#[test]
fn test_get_header_by_hash_detects_change_node_type() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    // Initialize database
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.commit().unwrap();
    }

    // Create a change
    let mut change = Change {
        offsets: libatomic::change::Offsets::default(),
        hashed: libatomic::change::Hashed {
            version: 1,
            header: ChangeHeader {
                message: "Change message".to_string(),
                authors: vec![],
                timestamp: chrono::Utc::now(),
                description: None,
            },
            dependencies: vec![],
            extra_known: vec![],
            metadata: vec![],
            changes: vec![],
            contents_hash: Merkle::zero(),
            tag: None,
        },
        unhashed: None,
        contents: vec![],
    };

    // Store in changestore - this computes the hash
    let changes = Memory::new();
    let change_hash = changes
        .save_change(&mut change, |_, _| {
            Ok::<(), libatomic::changestore::memory::Error>(())
        })
        .unwrap();

    let change_id = NodeId(::sanakirja::L64(1));

    // Register change
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        libatomic::pristine::register_change(&mut txn, &change_id, &change_hash, &change).unwrap();
        txn.commit().unwrap();
    }

    // Verify node type was set correctly
    {
        let txn = pristine.txn_begin().unwrap();
        assert_eq!(
            txn.get_node_type(&change_id).unwrap(),
            Some(NodeType::Change)
        );
    }

    // Test that get_header_by_hash correctly identifies and loads the change
    {
        let txn = pristine.txn_begin().unwrap();

        let change_header = get_header_by_hash(&txn, &changes, &change_hash).unwrap();
        assert_eq!(change_header.message, "Change message");

        println!("✅ Successfully detected and loaded change header");
    }
}

#[test]
fn test_get_header_by_hash_unknown_hash() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    // Initialize database
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.commit().unwrap();
    }

    let changes = Memory::new();
    // Create a hash that won't be in the database by using a different point
    let unknown_hash = Merkle::zero().next(1u64);

    // Test that unknown hash is handled gracefully
    {
        let txn = pristine.txn_begin().unwrap();
        let result = get_header_by_hash(&txn, &changes, &unknown_hash);

        assert!(
            result.is_err(),
            "Expected error for unknown hash, got: {:?}",
            result
        );
        println!("✅ Correctly handled unknown hash with error");
    }
}
