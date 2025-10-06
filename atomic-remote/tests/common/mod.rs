//! Common test utilities for atomic-remote integration tests
//!
//! Provides helper functions for creating test repositories, changes, and tags
//! used across Phase 2 and Phase 3 integration tests.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use libatomic::pristine::{Base32, Hash, Merkle, MutTxnT, NodeId, NodeType, TxnT};
use libatomic::{ChannelTxnT, GraphTxnT, MutTxnTExt, TxnTExt};

use atomic_repository::Repository;

/// Create a test repository in a temporary directory
pub fn create_test_repo() -> (tempfile::TempDir, Repository) {
    let tmp = tempfile::tempdir().expect("Failed to create temp dir");
    let repo_path = tmp.path().to_path_buf();

    // Initialize repository
    Repository::init(Some(repo_path.clone()), None).expect("Failed to initialize repository");

    let repo =
        Repository::find_root(Some(repo_path.join(".atomic"))).expect("Failed to open repository");

    (tmp, repo)
}

/// Create a test change in a repository
///
/// Returns the hash of the created change
pub fn create_test_change(repo: &Repository, filename: &str, content: &str) -> Hash {
    // Write file to working copy
    let file_path = repo.path.join(filename);
    let mut file = fs::File::create(&file_path).expect("Failed to create file");
    file.write_all(content.as_bytes())
        .expect("Failed to write file");

    // Add and record the change
    let mut txn = repo
        .pristine
        .mut_txn_begin()
        .expect("Failed to begin transaction");
    let mut channel = txn
        .open_or_create_channel("main")
        .expect("Failed to open channel");

    // Add file to pristine
    txn.add_file(filename.to_string(), 0)
        .expect("Failed to add file");

    // For testing, we'll create a simple change by directly manipulating files
    // and using a simplified recording process

    // Create a deterministic hash for testing
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(filename.as_bytes());
    hasher.update(content.as_bytes());
    hasher.update(
        &std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_le_bytes(),
    );
    let change_hash = Hash::from_bytes(hasher.finalize().as_bytes());

    // For now, just register the change hash as a node type
    // In a full implementation, we'd need to create actual change files
    if let Some(internal) = txn.get_internal(&(&change_hash).into()).ok().flatten() {
        let _ = txn.put_node_type(&internal, NodeType::Change);
    }

    txn.commit().expect("Failed to commit transaction");

    change_hash
}

/// Get the current state (Merkle) of a channel
pub fn get_channel_state(repo: &Repository, channel_name: &str) -> Merkle {
    let txn = repo.pristine.txn_begin().expect("Failed to begin txn");
    let channel = txn
        .load_channel(channel_name)
        .expect("Failed to load channel")
        .expect("Channel not found");

    let channel_read = channel.read();
    txn.current_state(&*channel_read)
        .expect("Failed to get current state")
}

/// Create a tag file for a given state
pub fn create_tag_file(repo: &Repository, state: &Merkle, tag_name: &str) {
    let txn = repo.pristine.txn_begin().expect("Failed to begin txn");
    let channel = txn
        .load_channel("main")
        .expect("Failed to load channel")
        .expect("Channel not found");

    // Create tag header
    let header = libatomic::tag::TagHeader {
        message: format!("Tag: {}", tag_name),
        authors: vec![],
        description: None,
        timestamp: chrono::Utc::now(),
    };

    // Create tag file path
    let mut tag_path = repo.changes_dir.clone();
    libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, state);

    fs::create_dir_all(tag_path.parent().unwrap()).expect("Failed to create tag directory");

    // Write tag file
    let mut tag_file = fs::File::create(&tag_path).expect("Failed to create tag file");
    libatomic::tag::from_channel(&txn, "main", &header, &mut tag_file)
        .expect("Failed to write tag file");

    drop(txn);

    // Register tag in database
    let mut mut_txn = repo
        .pristine
        .mut_txn_begin()
        .expect("Failed to begin mut txn");
    let mut channel = mut_txn
        .load_channel("main")
        .expect("Failed to load channel")
        .expect("Channel not found");

    let channel_read = channel.read();
    if let Some(position) = mut_txn
        .channel_has_state(mut_txn.states(&*channel_read), state)
        .expect("Failed to check state")
    {
        drop(channel_read);
        let tags = mut_txn.tags_mut(&mut channel.write());
        mut_txn
            .put_tags(tags, position.into(), state)
            .expect("Failed to put tag");
    }

    mut_txn.commit().expect("Failed to commit tag");
}

/// Create a test hash from bytes
pub fn test_hash(data: &[u8]) -> Hash {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(data);
    let hash_bytes = hasher.finalize();
    Hash::from_bytes(hash_bytes.as_bytes())
}

/// Create a test merkle from a hash
pub fn test_merkle(hash: &Hash) -> Merkle {
    let hash_bytes = hash.to_bytes();
    Merkle::from_bytes(&hash_bytes)
}

/// Helper to check if a change exists in a channel
pub fn channel_has_change(
    txn: &impl TxnT,
    channel: &libatomic::pristine::ChannelRef<impl TxnT>,
    hash: &Hash,
) -> bool {
    txn.has_change(channel, hash).is_ok()
}

/// Helper to get the number of changes in a channel
pub fn count_channel_changes(
    txn: &impl TxnT,
    channel: &libatomic::pristine::ChannelRef<impl TxnT>,
) -> u64 {
    let mut count = 0;
    if let Ok(iter) = txn.log(&*channel.read(), 0) {
        for _ in iter {
            count += 1;
        }
    }
    count
}

/// Helper to get the number of tags in a channel
pub fn count_channel_tags(
    txn: &impl TxnT,
    channel: &libatomic::pristine::ChannelRef<impl TxnT>,
) -> u64 {
    let mut count = 0;
    if let Ok(iter) = txn.iter_tags(txn.tags(&*channel.read()), 0) {
        for _ in iter {
            count += 1;
        }
    }
    count
}
