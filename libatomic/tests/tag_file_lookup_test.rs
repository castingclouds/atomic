//! Integration test to reproduce tag file lookup issue
//! This test creates a tag using from_channel and then verifies get_header_by_hash() can retrieve it

use libatomic::change::ChangeHeader;
use libatomic::changestore::filesystem::FileSystem as ChangeFileSystem;
use libatomic::pristine::MerkleHasher as Hasher;
use libatomic::pristine::{
    get_header_by_hash, Base32, GraphTxnT, Hash, Merkle, MutTxnT, NodeId, NodeType,
};
use tempfile::tempdir;

#[test]
fn test_tag_file_lookup_with_get_header_by_hash() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init()
        .ok();

    let tmp = tempdir().unwrap();
    let repo_path = tmp.path().to_path_buf();

    // Create pristine database
    let pristine =
        libatomic::pristine::sanakirja::Pristine::new(&repo_path.join("pristine.db")).unwrap();

    // Initialize database and create a channel
    {
        let mut txn = pristine.mut_txn_begin().unwrap();
        txn.open_or_create_channel("main").unwrap();
        txn.commit().unwrap();
    }

    // Create changes directory
    let changes_dir = repo_path.join("changes");
    std::fs::create_dir_all(&changes_dir).unwrap();

    // Create changestore
    let changes = ChangeFileSystem::from_changes(changes_dir.clone(), 10);

    println!("\n=== Step 1: Creating a unique tag ID ===");

    // DON'T use Merkle::zero() - that's a special sentinel value already registered!
    // Instead, create a unique merkle hash for this test
    let unique_tag_id = {
        let mut hasher = Hasher::default();
        hasher.update(b"unique_test_tag_");
        hasher.update(
            &std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_le_bytes(),
        );
        hasher.finish()
    };

    println!("   Created unique tag ID: {}", unique_tag_id.to_base32());

    // Verify this ID is NOT already in the database
    {
        let txn = pristine.txn_begin().unwrap();
        let existing = txn.get_internal(&unique_tag_id.into()).unwrap();
        println!("   Existing mapping in database: {:?}", existing);
        assert!(existing.is_none(), "Tag ID should not be pre-registered");
    }

    println!("\n=== Step 2: Creating tag file using from_channel ===");

    let tag_header = ChangeHeader {
        message: "Test tag for file lookup".to_string(),
        authors: vec![],
        description: None,
        timestamp: chrono::Utc::now(),
    };

    // Create tag file using from_channel
    // Note: from_channel computes the tag's merkle state from the channel
    let tag_merkle = {
        let txn = pristine.txn_begin().unwrap();
        let mut tag_path = changes_dir.clone();

        // Create a temporary file to write to
        let temp_path = changes_dir.join("temp_tag");
        let mut temp_file = std::fs::File::create(&temp_path).unwrap();

        let state =
            libatomic::tag::from_channel(&txn, "main", &tag_header, &mut temp_file).unwrap();
        drop(temp_file);

        // Now move it to the correct location
        libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &state);

        // Create parent directory
        if let Some(parent) = tag_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }

        // Move the temp file to the correct location
        std::fs::rename(&temp_path, &tag_path).unwrap();

        println!("✅ Tag file created at: {}", tag_path.display());
        println!("   Tag state: {}", state.to_base32());
        println!("   File exists: {}", tag_path.exists());
        println!("   File size: {} bytes", tag_path.metadata().unwrap().len());

        state
    };

    let tag_hash: Hash = tag_merkle.into(); // Hash and Merkle are the same type

    println!("\n=== Step 3: Allocating internal ID for tag ===");

    // The key insight: We need to allocate a NEW internal ID that doesn't conflict
    // with any existing IDs in the database
    let tag_id = {
        let txn = pristine.txn_begin().unwrap();

        // Check if the tag merkle is already registered
        let existing_internal = txn.get_internal(&tag_hash.into()).unwrap().copied();

        if let Some(existing_id) = existing_internal {
            println!("   Tag merkle already has internal ID: {:?}", existing_id);
            println!("   Using existing ID for registration");
            existing_id
        } else {
            // Allocate a new ID
            let new_id = NodeId(::sanakirja::L64(100));
            println!("   Allocating new internal ID: {:?}", new_id);
            new_id
        }
    };

    println!("\n=== Step 4: Registering tag in database ===");

    {
        let mut txn = pristine.mut_txn_begin().unwrap();

        // Register the tag node
        libatomic::pristine::register_node(
            &mut txn,
            &tag_id,
            &tag_hash,
            NodeType::Tag,
            &[], // No dependencies
        )
        .unwrap();

        println!("   ✅ Tag registered with ID {:?}", tag_id);

        txn.commit().unwrap();
    }

    println!("\n=== Step 5: Verifying tag file can be read directly ===");

    let mut direct_tag_file = libatomic::tag::OpenTagFile::open(
        {
            let mut tag_path = changes_dir.clone();
            libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &tag_merkle);
            tag_path
        },
        &tag_merkle,
    )
    .unwrap();
    let direct_header = direct_tag_file.header().unwrap();
    println!("✅ Successfully read tag file directly");
    println!("   Message: {}", direct_header.message);

    println!("\n=== Step 6: Verifying database state ===");

    {
        let txn = pristine.txn_begin().unwrap();

        // Check node type
        let node_type = txn.get_node_type(&tag_id).unwrap();
        println!("   Node type for ID {:?}: {:?}", tag_id, node_type);
        assert_eq!(node_type, Some(NodeType::Tag), "Node type should be Tag");

        // Check internal mapping (hash -> internal ID)
        let internal = txn.get_internal(&tag_hash.into()).unwrap();
        println!("   Internal ID returned by get_internal(): {:?}", internal);
        assert_eq!(
            internal,
            Some(&tag_id),
            "Internal mapping should point to tag_id"
        );

        // Check external mapping (internal ID -> hash)
        let external = txn.get_external(&tag_id).unwrap();
        if let Some(ext_hash) = external {
            let ext_merkle: Merkle = ext_hash.into();
            println!("   External hash: {}", ext_merkle.to_base32());
            assert_eq!(
                ext_merkle, tag_merkle,
                "External mapping should match tag_merkle"
            );
        } else {
            panic!("External mapping not found!");
        }
    }

    println!("✅ Database state is correct");

    println!("\n=== Step 7: Testing get_header_by_hash() ===");
    println!("   Looking up hash: {}", tag_hash.to_base32());

    let txn = pristine.txn_begin().unwrap();
    let result = get_header_by_hash(&txn, &changes, &tag_hash);

    match result {
        Ok(header) => {
            println!("✅ Successfully retrieved tag header via get_header_by_hash()");
            println!("   Message: {}", header.message);
            assert_eq!(
                header.message, "Test tag for file lookup",
                "Header message should match"
            );
        }
        Err(e) => {
            println!("❌ Failed to retrieve tag header via get_header_by_hash()");
            println!("   Error: {}", e);
            println!("\n=== Debugging Info ===");
            let mut tag_path = changes_dir.clone();
            libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &tag_merkle);
            println!("   Tag path: {}", tag_path.display());
            println!("   Tag path exists: {}", tag_path.exists());
            println!("   Changes dir: {}", changes_dir.display());

            // Print database state
            let txn = pristine.txn_begin().unwrap();
            let internal = txn.get_internal(&tag_hash.into()).unwrap();
            let node_type = internal.and_then(|id| txn.get_node_type(id).unwrap());
            println!("   Internal ID: {:?}", internal);
            println!("   Node type: {:?}", node_type);

            panic!("get_header_by_hash() failed: {}", e);
        }
    }

    println!("\n✅ Test completed successfully! Tag file lookup works correctly.");
}

#[test]
fn test_tag_file_path_construction() {
    // Test that tag file path construction works correctly
    let tmp = tempdir().unwrap();
    let changes_dir = tmp.path().join("changes");
    std::fs::create_dir_all(&changes_dir).unwrap();

    let tag_merkle = Merkle::zero();
    let tag_base32 = tag_merkle.to_base32();

    println!("Tag base32: {}", tag_base32);
    println!("Tag base32 length: {}", tag_base32.len());

    let mut tag_path = changes_dir.clone();
    libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &tag_merkle);

    println!("Expected tag path: {}", tag_path.display());

    // Extract first 2 chars
    let (first_two, rest) = tag_base32.split_at(2);
    println!("First 2 chars: {}", first_two);
    println!("Rest: {}", rest);

    // Verify path structure
    let path_string = tag_path.to_string_lossy();
    assert!(
        path_string.contains(&format!("{}/{}.tag", first_two, rest)),
        "Path should contain {}/{}.tag but got {}",
        first_two,
        rest,
        path_string
    );
}
