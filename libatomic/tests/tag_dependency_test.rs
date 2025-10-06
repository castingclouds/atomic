//! Integration tests for changes depending on tags (Phase 4)

use libatomic::change::{Change, ChangeHeader};
use libatomic::pristine::MerkleHasher as Hasher;
use libatomic::pristine::{
    DepsTxnT, GraphMutTxnT, GraphTxnT, Hash, Merkle, MutTxnT, NodeId, NodeType, SerializedTag, Tag,
    TagMetadataMutTxnT,
};
use tempfile::tempdir;

#[test]
fn test_change_can_depend_on_tag() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    // Initialize database version
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.commit().unwrap();
    }

    // Step 1: Register a tag
    let tag_id = NodeId(::sanakirja::L64(100));
    let tag_merkle = Merkle::zero();

    let mut h = Hasher::default();
    h.update(b"tag_hash_1");
    let tag_hash = h.finish();

    let tag = Tag {
        tag_hash,
        change_file_hash: None,
        state: tag_merkle,
        channel: "main".to_string(),
        consolidation_timestamp: 1000,
        previous_consolidation: None,
        dependency_count_before: 10,
        consolidated_change_count: 5,
        consolidates_since: None,
        consolidated_changes: vec![],
        version: Some("v1.0.0".to_string()),
        message: Some("Test tag".to_string()),
        created_by: Some("test".to_string()),
        metadata: std::collections::HashMap::new(),
    };

    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        // Store tag metadata
        let serialized = SerializedTag::from_tag(&tag).unwrap();
        txn.put_tag(&tag_hash, &serialized).unwrap();
        // Register node with no dependencies
        libatomic::pristine::register_node(&mut txn, &tag_id, &tag_hash, NodeType::Tag, &[])
            .unwrap();
        txn.commit().unwrap();
    }

    // Step 2: Verify tag was registered with correct node type
    {
        let txn = pristine.txn_begin().unwrap();
        assert_eq!(txn.get_node_type(&tag_id).unwrap(), Some(NodeType::Tag));

        // Verify internal/external mappings
        let tag_hash: Hash = tag_merkle.into();
        let shash = (&tag_hash).into();
        assert_eq!(txn.get_internal(&shash).unwrap(), Some(&tag_id));
        assert!(txn.get_external(&tag_id).unwrap().is_some());
    }

    // Step 3: Create a change that depends on the tag
    let change_id = NodeId(::sanakirja::L64(101));

    let mut h = Hasher::default();
    h.update(b"change_hash_1");
    let change_hash = h.finish();

    let tag_hash: Hash = tag_merkle.into();

    let change = Change {
        offsets: libatomic::change::Offsets::default(),
        hashed: libatomic::change::Hashed {
            version: 1,
            header: ChangeHeader {
                message: "Change depending on tag".to_string(),
                authors: vec![],
                timestamp: chrono::Utc::now(),
                description: None,
            },
            dependencies: vec![tag_hash], // Depends on the tag!
            extra_known: vec![],
            metadata: vec![],
            changes: vec![],
            contents_hash: Merkle::zero(),
            tag: None,
        },
        unhashed: None,
        contents: vec![],
    };

    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        libatomic::pristine::register_change(&mut txn, &change_id, &change_hash, &change).unwrap();
        txn.commit().unwrap();
    }

    // Step 4: Verify the dependency was recorded
    {
        let txn = pristine.txn_begin().unwrap();

        // Verify change node type
        assert_eq!(
            txn.get_node_type(&change_id).unwrap(),
            Some(NodeType::Change)
        );

        // Verify dep table: change_id -> tag_id
        let mut deps_found = false;
        for dep_result in txn.iter_dep(&change_id).unwrap() {
            let (_key, dep_id) = dep_result.unwrap();
            if *dep_id == tag_id {
                deps_found = true;
            }
        }
        assert!(deps_found, "Change should have tag as dependency");

        // Verify revdep table: tag_id -> change_id
        let mut revdeps_found = false;
        for revdep_result in txn.iter_revdep(&tag_id).unwrap() {
            let (_key, revdep_id) = revdep_result.unwrap();
            if *revdep_id == change_id {
                revdeps_found = true;
            }
        }
        assert!(
            revdeps_found,
            "Tag should have change as reverse dependency"
        );
    }
}

#[test]
fn test_change_depends_on_multiple_tags() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    // Initialize database
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.commit().unwrap();
    }

    // Register two tags
    let tag_id_1 = NodeId(::sanakirja::L64(100));
    let tag_merkle_1 = Merkle::zero();

    let mut h = Hasher::default();
    h.update(b"tag_hash_2");
    let tag_hash_1 = h.finish();

    let tag_1 = Tag {
        tag_hash: tag_hash_1,
        change_file_hash: None,
        state: tag_merkle_1,
        channel: "main".to_string(),
        consolidation_timestamp: 1000,
        previous_consolidation: None,
        dependency_count_before: 5,
        consolidated_change_count: 3,
        consolidates_since: None,
        consolidated_changes: vec![],
        version: Some("v1.0.0".to_string()),
        message: Some("First tag".to_string()),
        created_by: Some("test".to_string()),
        metadata: std::collections::HashMap::new(),
    };

    let tag_id_2 = NodeId(::sanakirja::L64(200));
    let tag_merkle_2 = Merkle::zero().next(1u64);

    let mut h = Hasher::default();
    h.update(b"tag_hash_3");
    let tag_hash_2 = h.finish();

    let tag_2 = Tag {
        tag_hash: tag_hash_2,
        change_file_hash: None,
        state: tag_merkle_2,
        channel: "main".to_string(),
        consolidation_timestamp: 2000,
        previous_consolidation: None,
        dependency_count_before: 8,
        consolidated_change_count: 4,
        consolidates_since: None,
        consolidated_changes: vec![],
        version: Some("v2.0.0".to_string()),
        message: Some("Second tag".to_string()),
        created_by: Some("test".to_string()),
        metadata: std::collections::HashMap::new(),
    };

    // Register tags
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        // Store tag1 metadata
        let serialized1 = SerializedTag::from_tag(&tag_1).unwrap();
        txn.put_tag(&tag_hash_1, &serialized1).unwrap();
        libatomic::pristine::register_node(&mut txn, &tag_id_1, &tag_hash_1, NodeType::Tag, &[])
            .unwrap();
        // Store tag2 metadata
        let serialized2 = SerializedTag::from_tag(&tag_2).unwrap();
        txn.put_tag(&tag_hash_2, &serialized2).unwrap();
        libatomic::pristine::register_node(&mut txn, &tag_id_2, &tag_hash_2, NodeType::Tag, &[])
            .unwrap();
        txn.commit().unwrap();
    }

    // Create a change that depends on both tags
    let change_id = NodeId(::sanakirja::L64(300));

    let mut h = Hasher::default();
    h.update(b"change_hash_2");
    let change_hash = h.finish();

    let tag_hash_1: Hash = tag_merkle_1.into();
    let tag_hash_2: Hash = tag_merkle_2.into();

    let change = Change {
        offsets: libatomic::change::Offsets::default(),
        hashed: libatomic::change::Hashed {
            version: 1,
            header: ChangeHeader {
                message: "Change depending on two tags".to_string(),
                authors: vec![],
                timestamp: chrono::Utc::now(),
                description: None,
            },
            dependencies: vec![tag_hash_1, tag_hash_2], // Depends on both tags!
            extra_known: vec![],
            metadata: vec![],
            changes: vec![],
            contents_hash: Merkle::zero(),
            tag: None,
        },
        unhashed: None,
        contents: vec![],
    };

    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        libatomic::pristine::register_change(&mut txn, &change_id, &change_hash, &change).unwrap();
        txn.commit().unwrap();
    }

    // Verify both dependencies were recorded
    {
        let txn = pristine.txn_begin().unwrap();

        let deps: Vec<_> = txn
            .iter_dep(&change_id)
            .unwrap()
            .map(|r| *r.unwrap().1)
            .collect();
        assert_eq!(deps.len(), 2, "Change should have 2 dependencies");
        assert!(deps.contains(&tag_id_1));
        assert!(deps.contains(&tag_id_2));

        // Verify reverse dependencies
        let revdeps_1: Vec<_> = txn
            .iter_revdep(&tag_id_1)
            .unwrap()
            .map(|r| *r.unwrap().1)
            .collect();
        assert!(revdeps_1.contains(&change_id));

        let revdeps_2: Vec<_> = txn
            .iter_revdep(&tag_id_2)
            .unwrap()
            .map(|r| *r.unwrap().1)
            .collect();
        assert!(revdeps_2.contains(&change_id));
    }
}

#[test]
fn test_change_depends_on_change_and_tag() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    // Initialize database
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.commit().unwrap();
    }

    // Register a regular change
    let change_id_1 = NodeId(::sanakirja::L64(1));

    let mut h = Hasher::default();
    h.update(b"change_hash_3");
    let change_hash_1 = h.finish();

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
            contents_hash: Merkle::zero(),
            tag: None,
        },
        unhashed: None,
        contents: vec![],
    };

    // Register a tag
    let tag_id = NodeId(::sanakirja::L64(100));
    let tag_merkle = Merkle::zero();

    let mut h = Hasher::default();
    h.update(b"tag_hash_4");
    let tag_hash = h.finish();

    let tag = Tag {
        tag_hash,
        change_file_hash: None,
        state: tag_merkle,
        channel: "main".to_string(),
        consolidation_timestamp: 1000,
        previous_consolidation: None,
        dependency_count_before: 5,
        consolidated_change_count: 3,
        consolidates_since: None,
        consolidated_changes: vec![],
        version: Some("v1.0.0".to_string()),
        message: Some("Test tag".to_string()),
        created_by: Some("test".to_string()),
        metadata: std::collections::HashMap::new(),
    };

    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        // Register change with no dependencies
        libatomic::pristine::register_node(
            &mut txn,
            &change_id_1,
            &change_hash_1,
            NodeType::Change,
            &[],
        )
        .unwrap();
        // Store tag metadata
        let serialized = SerializedTag::from_tag(&tag).unwrap();
        txn.put_tag(&tag_hash, &serialized).unwrap();
        // Register tag with no dependencies
        libatomic::pristine::register_node(&mut txn, &tag_id, &tag_hash, NodeType::Tag, &[])
            .unwrap();
        txn.commit().unwrap();
    }

    // Create a change that depends on both a change and a tag
    let change_id_2 = NodeId(::sanakirja::L64(2));

    let mut h = Hasher::default();
    h.update(b"change_hash_4");
    let change_hash_2 = h.finish();

    let tag_hash: Hash = tag_merkle.into();

    let change_2 = Change {
        offsets: libatomic::change::Offsets::default(),
        hashed: libatomic::change::Hashed {
            version: 1,
            header: ChangeHeader {
                message: "Change depending on change and tag".to_string(),
                authors: vec![],
                timestamp: chrono::Utc::now(),
                description: None,
            },
            dependencies: vec![change_hash_1, tag_hash], // Depends on both!
            extra_known: vec![],
            metadata: vec![],
            changes: vec![],
            contents_hash: Merkle::zero(),
            tag: None,
        },
        unhashed: None,
        contents: vec![],
    };

    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        libatomic::pristine::register_change(&mut txn, &change_id_2, &change_hash_2, &change_2)
            .unwrap();
        txn.commit().unwrap();
    }

    // Verify mixed dependencies work correctly
    {
        let txn = pristine.txn_begin().unwrap();

        let deps: Vec<_> = txn
            .iter_dep(&change_id_2)
            .unwrap()
            .map(|r| *r.unwrap().1)
            .collect();
        assert_eq!(deps.len(), 2, "Change should have 2 dependencies");
        assert!(deps.contains(&change_id_1), "Should depend on change");
        assert!(deps.contains(&tag_id), "Should depend on tag");

        // Verify node types
        assert_eq!(
            txn.get_node_type(&change_id_1).unwrap(),
            Some(NodeType::Change)
        );
        assert_eq!(txn.get_node_type(&tag_id).unwrap(), Some(NodeType::Tag));
        assert_eq!(
            txn.get_node_type(&change_id_2).unwrap(),
            Some(NodeType::Change)
        );
    }
}

#[test]
fn test_tag_can_be_depended_on_by_multiple_changes() {
    let tmp = tempdir().unwrap();
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&tmp.path().join("pristine.db")).unwrap();

    // Initialize database
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.commit().unwrap();
    }

    // Register one tag
    let tag_id = NodeId(::sanakirja::L64(100));
    let tag_merkle = Merkle::zero();

    let mut h = Hasher::default();
    h.update(b"tag_hash_5");
    let tag_hash = h.finish();

    let tag = Tag {
        tag_hash,
        change_file_hash: None,
        state: tag_merkle,
        channel: "main".to_string(),
        consolidation_timestamp: 1000,
        previous_consolidation: None,
        dependency_count_before: 10,
        consolidated_change_count: 5,
        consolidates_since: None,
        consolidated_changes: vec![],
        version: Some("v1.0.0".to_string()),
        message: Some("Shared tag".to_string()),
        created_by: Some("test".to_string()),
        metadata: std::collections::HashMap::new(),
    };

    // Register tag
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        let serialized = SerializedTag::from_tag(&tag).unwrap();
        txn.put_tag(&tag_hash, &serialized).unwrap();
        libatomic::pristine::register_node(&mut txn, &tag_id, &tag_hash, NodeType::Tag, &[])
            .unwrap();
        txn.commit().unwrap();
    }

    // Create multiple changes that all depend on the same tag
    let tag_hash: Hash = tag_merkle.into();
    let change_ids: Vec<NodeId> = (1..=5).map(|i| NodeId(::sanakirja::L64(i))).collect();

    for (i, &change_id) in change_ids.iter().enumerate() {
        let mut h = Hasher::default();
        h.update(format!("change_hash_{}", i + 5).as_bytes());
        let change_hash = h.finish();

        let change = Change {
            offsets: libatomic::change::Offsets::default(),
            hashed: libatomic::change::Hashed {
                version: 1,
                header: ChangeHeader {
                    message: format!("Change {} depending on tag", i + 1),
                    authors: vec![],
                    timestamp: chrono::Utc::now(),
                    description: None,
                },
                dependencies: vec![tag_hash],
                extra_known: vec![],
                metadata: vec![],
                changes: vec![],
                contents_hash: Merkle::zero(),
                tag: None,
            },
            unhashed: None,
            contents: vec![],
        };

        let mut txn = pristine.mut_txn_begin().unwrap();
        libatomic::pristine::register_change(&mut txn, &change_id, &change_hash, &change).unwrap();
        txn.commit().unwrap();
    }

    // Verify the tag has all 5 changes as reverse dependencies
    {
        let txn = pristine.txn_begin().unwrap();

        let revdeps: Vec<_> = txn
            .iter_revdep(&tag_id)
            .unwrap()
            .map(|r| *r.unwrap().1)
            .collect();
        assert_eq!(revdeps.len(), 5, "Tag should have 5 reverse dependencies");

        for change_id in &change_ids {
            assert!(
                revdeps.contains(change_id),
                "Tag should have change {:?} as revdep",
                change_id
            );
        }
    }
}
