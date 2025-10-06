use std::io::Write;
use std::path::PathBuf;

use crate::commands::record::parse_datetime_rfc2822;
use anyhow::bail;
use atomic_repository::Repository;
use clap::{Parser, ValueHint};
use libatomic::change::ChangeHeader;
use libatomic::pristine::TagMetadataTxnT;
use libatomic::{ArcTxn, Base32, ChannelMutTxnT, ChannelTxnT, MutTxnT, TxnT, TxnTExt};
use log::*;

#[derive(Parser, Debug)]
pub struct Tag {
    #[clap(subcommand)]
    subcmd: Option<SubCommand>,
    #[clap(long = "repository", value_hint = ValueHint::DirPath)]
    repo_path: Option<PathBuf>,
    #[clap(long = "channel")]
    channel: Option<String>,
}

#[derive(Parser, Debug)]
pub enum SubCommand {
    /// Create a consolidating tag. Tags serve as dependency boundaries,
    /// enabling clean dependency trees by providing a single reference point
    /// instead of accumulated dependencies.
    #[clap(name = "create")]
    Create {
        /// Set the repository where this command should run. Defaults to
        /// the first ancestor of the current directory that contains a
        /// `.atomic` directory.
        #[clap(long = "repository", value_hint = ValueHint::DirPath)]
        repo_path: Option<PathBuf>,
        #[clap(short = 'm', long = "message")]
        message: Option<String>,
        /// Set the author field
        #[clap(long = "author")]
        author: Option<String>,
        /// Tag the current state of this channel instead of the
        /// current channel.
        #[clap(long = "channel")]
        channel: Option<String>,
        #[clap(long = "timestamp", value_parser = parse_datetime_rfc2822)]
        timestamp: Option<i64>,
        /// Specify which previous tag to consolidate from. If not specified,
        /// consolidates from the current state. This enables flexible consolidation
        /// strategies for production hotfix workflows.
        #[clap(long = "since")]
        since: Option<String>,
        /// Semantic version for this tag (e.g., "1.0.0", "0.0.1")
        #[clap(long = "version")]
        version: Option<String>,
        /// Increment major version (X.0.0)
        #[clap(long = "major", conflicts_with_all = &["minor", "patch", "version"])]
        major: bool,
        /// Increment minor version (x.Y.0)
        #[clap(long = "minor", conflicts_with_all = &["major", "patch", "version"])]
        minor: bool,
        /// Increment patch version (x.y.Z)
        #[clap(long = "patch", conflicts_with_all = &["major", "minor", "version"])]
        patch: bool,
    },
    /// Restore a tag into a new channel.
    #[clap(name = "checkout")]
    Checkout {
        /// Set the repository where this command should run. Defaults to
        /// the first ancestor of the current directory that contains a
        /// `.atomic` directory.
        #[clap(long = "repository", value_hint = ValueHint::DirPath)]
        repo_path: Option<PathBuf>,
        tag: String,
        /// Optional new channel name. If not given, the base32
        /// representation of the tag hash is used.
        #[clap(long = "to-channel")]
        to_channel: Option<String>,
    },
    /// Reset the working copy to a tag.
    #[clap(name = "reset")]
    Reset {
        /// Set the repository where this command should run. Defaults to
        /// the first ancestor of the current directory that contains a
        /// `.atomic` directory.
        #[clap(long = "repository", value_hint = ValueHint::DirPath)]
        repo_path: Option<PathBuf>,
        tag: String,
    },
    /// Delete a tag from a channel. If the same state isn't tagged in
    /// other channels, delete the tag file.
    #[clap(name = "delete")]
    Delete {
        /// Set the repository where this command should run. Defaults to
        /// the first ancestor of the current directory that contains a
        /// `.atomic` directory.
        #[clap(long = "repository", value_hint = ValueHint::DirPath)]
        repo_path: Option<PathBuf>,
        /// Delete the tag in this channel instead of the current channel
        #[clap(long = "channel")]
        channel: Option<String>,
        tag: String,
    },
    /// List tags
    #[clap(name = "list")]
    List {
        /// Set the repository where this command should run. Defaults to
        /// the first ancestor of the current directory that contains a
        /// `.atomic` directory.
        #[clap(long = "repository", value_hint = ValueHint::DirPath)]
        repo_path: Option<PathBuf>,
        /// List tags on this channel instead of the current channel
        #[clap(long = "channel")]
        channel: Option<String>,
        /// Show attribution summaries
        #[clap(long = "attribution")]
        attribution: bool,
    },
}

impl Tag {
    pub async fn run(self) -> Result<(), anyhow::Error> {
        let mut stdout = std::io::stdout();
        match self.subcmd {
            Some(SubCommand::Create {
                repo_path,
                message,
                author,
                channel,
                timestamp,
                since,
                version,
                major,
                minor,
                patch,
            }) => {
                let mut repo = Repository::find_root(repo_path)?;
                let txn = repo.pristine.arc_txn_begin()?;
                let channel_name = if let Some(c) = channel {
                    c
                } else {
                    txn.read()
                        .current_channel()
                        .unwrap_or(libatomic::DEFAULT_CHANNEL)
                        .to_string()
                };

                // Determine the version for this tag
                let tag_version = if let Some(v) = version {
                    v
                } else if major || minor || patch {
                    // Get the last tag to increment from
                    let channel = txn.read().load_channel(&channel_name)?.unwrap();
                    let last_tag_version = find_last_tag_version(&*txn.read(), &channel)?;
                    increment_semver(&last_tag_version, major, minor, patch)?
                } else {
                    // No version specified - default to 0.0.1
                    "0.0.1".to_string()
                };
                debug!("channel_name = {:?}", channel_name);
                try_record(&mut repo, txn.clone(), &channel_name)?;
                let channel = txn.read().load_channel(&channel_name)?.unwrap();
                let last_t = if let Some(n) = txn.read().reverse_log(&*channel.read(), None)?.next()
                {
                    n?.0.into()
                } else {
                    bail!("Channel {} is empty", channel_name);
                };
                log::debug!("last_t = {:?}", last_t);
                if txn.read().is_tagged(&channel.read().tags, last_t)? {
                    bail!("Current state is already tagged")
                }
                let mut tag_path = repo.changes_dir.clone();
                std::fs::create_dir_all(&tag_path)?;

                let mut temp_path = tag_path.clone();
                temp_path.push("tmp");

                let mut w = std::fs::File::create(&temp_path)?;
                // Use version as the message if no message provided
                let tag_message = message.or(Some(tag_version.clone()));
                let header = header(author.as_deref(), tag_message, timestamp).await?;
                let h: libatomic::Merkle =
                    libatomic::tag::from_channel(&*txn.read(), &channel_name, &header, &mut w)?;
                libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &h);
                std::fs::create_dir_all(tag_path.parent().unwrap())?;
                std::fs::rename(&temp_path, &tag_path)?;

                // Store consolidating tag metadata in database
                // Tags ARE consolidating tags in Atomic - that's their purpose
                {
                    use libatomic::pristine::{
                        Hash as PristineHash, SerializedTag, Tag, TagMetadataMutTxnT,
                    };

                    // Convert Merkle tag hash to Hash for database keying
                    let tag_hash = h;

                    // Find the most recent tag in the channel to determine where to start consolidating
                    // IMPORTANT: Do this BEFORE adding the new tag to the tags table
                    let start_position = {
                        let mut last_tag_pos = None;
                        let txn_read = txn.read();
                        let channel_read = channel.read();
                        for entry in txn_read.rev_iter_tags(txn_read.tags(&*channel_read), None)? {
                            let (pos, _merkle_pair) = entry?;
                            debug!("Found previous tag at position: {:?}", pos);
                            last_tag_pos = Some(pos);
                            break; // Get the most recent tag
                        }
                        // Start from the position after the last tag, or from 0 if no tags exist
                        let start = last_tag_pos.map(|p| p.0 + 1).unwrap_or(0);
                        debug!("Starting consolidation from position: {}", start);
                        start
                    };

                    // Collect changes from the last tag onwards to populate consolidated_changes
                    let mut consolidated_changes = Vec::new();
                    let mut change_count = 0u64;

                    for entry in txn.read().log(&*channel.read(), start_position)? {
                        let (pos, (hash, _)) = entry?;
                        // Convert SerializedHash to Hash
                        let hash: PristineHash = hash.into();
                        debug!("  Position {}: including change {}", pos, hash.to_base32());
                        consolidated_changes.push(hash);
                        change_count += 1;
                    }

                    info!(
                        "Tag consolidation: {} changes since position {}",
                        change_count, start_position
                    );

                    // For now, dependency_count_before equals change_count
                    // A future increment will implement proper dependency graph analysis
                    let dependency_count_before = change_count;
                    let consolidated_change_count = change_count;

                    // Handle --since flag if provided (restore functionality)
                    let previous_consolidation = if let Some(since_tag) = since {
                        // Look up the previous consolidating tag
                        match resolve_tag_to_hash(&since_tag, &*txn.read(), &channel_name)? {
                            Some(since_hash) => {
                                let since_key = since_hash;
                                // Verify the tag exists as a consolidating tag
                                if txn.read().get_tag(&since_key)?.is_some() {
                                    Some(since_key)
                                } else {
                                    return Err(anyhow::anyhow!(
                                        "Tag '{}' is not a consolidating tag",
                                        since_tag
                                    ));
                                }
                            }
                            None => {
                                return Err(anyhow::anyhow!("Tag '{}' not found", since_tag));
                            }
                        }
                    } else {
                        None
                    };

                    // Create the consolidating tag with the collected changes
                    let mut tag = if let Some(since_hash) = previous_consolidation {
                        Tag::new_with_since(
                            tag_hash,
                            h,
                            channel_name.clone(),
                            since_hash,
                            dependency_count_before,
                            consolidated_change_count,
                            consolidated_changes,
                        )
                    } else {
                        Tag::new(
                            tag_hash,
                            h,
                            channel_name.clone(),
                            None,
                            dependency_count_before,
                            consolidated_change_count,
                            consolidated_changes,
                        )
                    };

                    // Set the change_file_hash to the merkle state
                    // This is what should be used as a dependency when recording changes after the tag
                    tag.change_file_hash = Some(h);

                    // Note: We don't set change_file_hash because tags are referenced by their
                    // merkle hash directly (the hash used for the .tag filename), not a derived hash.
                    // The merkle hash IS the tag's identifier for dependencies.

                    // Serialize and store in database
                    let serialized = SerializedTag::from_tag(&tag).map_err(|e| {
                        anyhow::anyhow!("Failed to serialize consolidating tag: {}", e)
                    })?;

                    txn.write().put_tag(&tag_hash, &serialized)?;
                }

                // Update tags table
                txn.write()
                    .put_tags(&mut channel.write().tags, last_t.into(), &h)?;

                txn.commit()?;

                // Output just the tag hash (ONE tag, not two!)
                writeln!(stdout, "{}", h.to_base32())?;
            }
            Some(SubCommand::Checkout {
                repo_path,
                mut tag,
                to_channel,
            }) => {
                let repo = Repository::find_root(repo_path)?;
                let mut tag_path = repo.changes_dir.clone();
                let h = if let Some(h) = libatomic::Merkle::from_base32(tag.as_bytes()) {
                    libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &h);
                    h
                } else {
                    super::find_hash(&mut tag_path, &tag)?
                };

                let mut txn = repo.pristine.mut_txn_begin()?;
                tag = h.to_base32();
                let channel_name = if let Some(ref channel) = to_channel {
                    channel.as_str()
                } else {
                    tag.as_str()
                };
                if txn.load_channel(channel_name)?.is_some() {
                    bail!("Channel {:?} already exists", channel_name)
                }
                let f = libatomic::tag::OpenTagFile::open(&tag_path, &h)?;
                libatomic::tag::restore_channel(f, &mut txn, &channel_name)?;
                txn.commit()?;
                writeln!(stdout, "Tag {} restored as channel {}", tag, channel_name)?;
            }
            Some(SubCommand::Reset { repo_path, tag }) => {
                let repo = Repository::find_root(repo_path)?;
                let mut tag_path = repo.changes_dir.clone();
                let h = if let Some(h) = libatomic::Merkle::from_base32(tag.as_bytes()) {
                    libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &h);
                    h
                } else {
                    super::find_hash(&mut tag_path, &tag)?
                };

                let tag = libatomic::tag::txn::TagTxn::new(&tag_path, &h)?;
                let txn = libatomic::tag::txn::WithTag {
                    tag,
                    txn: repo.pristine.mut_txn_begin()?,
                };
                let channel = txn.channel();
                let txn = ArcTxn::new(txn);

                libatomic::output::output_repository_no_pending_(
                    &repo.working_copy,
                    &repo.changes,
                    &txn,
                    &channel,
                    "",
                    true,
                    None,
                    std::thread::available_parallelism()?.get(),
                    0,
                )?;
                if let Ok(txn) = std::sync::Arc::try_unwrap(txn.0) {
                    txn.into_inner().txn.commit()?
                }
                writeln!(stdout, "Reset to tag {}", h.to_base32())?;
            }
            Some(SubCommand::Delete {
                repo_path,
                channel,
                tag,
            }) => {
                let repo = Repository::find_root(repo_path)?;
                let mut tag_path = repo.changes_dir.clone();
                let h = if let Some(h) = libatomic::Merkle::from_base32(tag.as_bytes()) {
                    libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &h);
                    h
                } else {
                    super::find_hash(&mut tag_path, &tag)?
                };

                let mut txn = repo.pristine.mut_txn_begin()?;
                let channel_name = channel.unwrap_or_else(|| {
                    txn.current_channel()
                        .unwrap_or(libatomic::DEFAULT_CHANNEL)
                        .to_string()
                });
                let channel = if let Some(c) = txn.load_channel(&channel_name)? {
                    c
                } else {
                    bail!("Channel {:?} not found", channel_name)
                };
                {
                    let mut ch = channel.write();
                    if let Some(n) = txn.channel_has_state(txn.states(&*ch), &h.into())? {
                        let tags = txn.tags_mut(&mut *ch);
                        txn.del_tags(tags, n.into())?;
                    }
                }
                txn.commit()?;
                writeln!(stdout, "Deleted tag {}", h.to_base32())?;
            }
            Some(SubCommand::List {
                repo_path,
                channel,
                attribution,
            }) => {
                use libatomic::pristine::TagMetadataTxnT;

                let repo = Repository::find_root(repo_path)?;
                let txn = repo.pristine.txn_begin()?;
                let channel_name = channel.unwrap_or_else(|| {
                    txn.current_channel()
                        .unwrap_or(libatomic::DEFAULT_CHANNEL)
                        .to_string()
                });

                // List all tags (all tags are consolidating tags in Atomic)
                let mut found_any = false;

                let channel = if let Some(c) = txn.load_channel(&channel_name)? {
                    c
                } else {
                    bail!("Channel {:?} not found", channel_name)
                };

                // Iterate through tags on this channel
                let channel_read = channel.read();
                for tag_entry in txn.iter_tags(txn.tags(&*channel_read), 0)? {
                    let (_, tag_bytes) = tag_entry?;

                    // Convert TagBytes to get the merkle (minimal tag from channel table)
                    let serialized =
                        libatomic::pristine::SerializedTag::from_bytes_wrapper(tag_bytes);
                    if let Ok(minimal_tag) = serialized.to_tag() {
                        let merkle_hash = minimal_tag.state;
                        let tag_hash = merkle_hash;

                        // Look up full tag metadata from global table
                        if let Some(full_tag_serialized) = txn.get_tag(&tag_hash)? {
                            if let Ok(tag) = full_tag_serialized.to_tag() {
                                found_any = true;

                                writeln!(
                                    stdout,
                                    "\nTag: {} (channel: {})",
                                    merkle_hash.to_base32(),
                                    tag.channel
                                )?;
                                writeln!(
                                    stdout,
                                    "  Consolidated changes: {}",
                                    tag.consolidated_change_count
                                )?;
                                writeln!(
                                    stdout,
                                    "  Dependencies before: {}",
                                    tag.dependency_count_before
                                )?;
                                writeln!(
                                    stdout,
                                    "  Effective dependencies: {}",
                                    tag.effective_dependency_count()
                                )?;
                                writeln!(
                                    stdout,
                                    "  Dependency reduction: {}",
                                    tag.dependency_reduction()
                                )?;

                                if attribution {
                                    if let Some(summary_serialized) =
                                        txn.get_tag_attribution_summary(&tag_hash)?
                                    {
                                        let summary = summary_serialized.to_summary()?;
                                        writeln!(stdout, "  Attribution:")?;
                                        writeln!(
                                            stdout,
                                            "    Total changes: {}",
                                            summary.total_changes
                                        )?;
                                        writeln!(
                                            stdout,
                                            "    AI-assisted: {}",
                                            summary.ai_assisted_changes
                                        )?;
                                        writeln!(
                                            stdout,
                                            "    Human-authored: {}",
                                            summary.human_authored_changes
                                        )?;
                                        writeln!(
                                            stdout,
                                            "    AI percentage: {:.1}%",
                                            summary.ai_percentage()
                                        )?;
                                    }
                                }
                            }
                        }
                    }
                }

                if !found_any {
                    writeln!(stdout, "No tags found")?;
                }
            }
            None => {
                let repo = Repository::find_root(self.repo_path)?;
                let txn = repo.pristine.txn_begin()?;
                let channel_name = self.channel.unwrap_or_else(|| {
                    txn.current_channel()
                        .unwrap_or(libatomic::DEFAULT_CHANNEL)
                        .to_string()
                });
                let channel = if let Some(c) = txn.load_channel(&channel_name)? {
                    c
                } else {
                    bail!("Channel {:?} not found", channel_name)
                };
                let mut tag_path = repo.changes_dir.clone();
                super::pager(repo.config.pager.as_ref());
                for t in txn.rev_iter_tags(txn.tags(&*channel.read()), None)? {
                    let (t, _) = t?;
                    let (_, m) = txn.get_changes(&channel, (*t).into())?.unwrap();
                    libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &m);
                    debug!("tag path {:?}", tag_path);
                    let mut f = libatomic::tag::OpenTagFile::open(&tag_path, &m)?;
                    let header = f.header()?;
                    writeln!(stdout, "State {}", m.to_base32())?;
                    writeln!(stdout, "Author: {:?}", header.authors)?;
                    writeln!(stdout, "Date: {}", header.timestamp)?;
                    writeln!(stdout, "\n    {}\n", header.message)?;
                    libatomic::changestore::filesystem::pop_filename(&mut tag_path);
                }
            }
        }
        Ok(())
    }
}

/// Writes a consolidating tag as a change file.
///
/// This function creates a Change structure containing the consolidating tag metadata
/// and writes it to the change store. The resulting change file can be synced across
/// repositories using the standard change protocol.
///
/// # Arguments
///
/// * `change_store` - The change store to write to
/// * `tag` - The consolidating tag to serialize
/// * `message` - The commit message for the tag
/// * `author` - The author of the tag
/// * `timestamp` - The timestamp for the tag
///
/// Resolves a tag name (base32 string or prefix) to its Merkle hash.
///
/// This function searches through tags on the specified channel to find
/// a matching tag by its base32 representation.
///
/// # Arguments
///
/// * `tag_name` - The tag name to resolve (full base32 or prefix)
/// * `txn` - The transaction to use for lookups
/// * `channel_name` - The channel to search for tags
///
/// # Returns
///
/// * `Ok(Some(merkle))` - If a unique tag is found
/// * `Ok(None)` - If no tag is found
/// * `Err(_)` - If the tag name is ambiguous or lookup fails
fn resolve_tag_to_hash<T: TxnT + ChannelTxnT>(
    tag_name: &str,
    txn: &T,
    channel_name: &str,
) -> Result<Option<libatomic::Merkle>, anyhow::Error> {
    let channel = if let Some(c) = txn.load_channel(channel_name)? {
        c
    } else {
        bail!("Channel '{}' not found", channel_name);
    };

    // Try exact match first
    if let Ok(merkle) = tag_name.parse::<libatomic::Merkle>() {
        // Verify this tag exists on the channel
        let channel_read = channel.read();
        for tag_entry in txn.iter_tags(txn.tags(&*channel_read), 0)? {
            let (_, tag_bytes) = tag_entry?;
            let serialized = libatomic::pristine::SerializedTag::from_bytes_wrapper(tag_bytes);
            if let Ok(tag) = serialized.to_tag() {
                let tag_merkle = tag.state;
                if tag_merkle == merkle {
                    return Ok(Some(merkle));
                }
            }
        }
    }

    // Try prefix match
    let mut matches = Vec::new();
    let channel_read = channel.read();
    for tag_entry in txn.iter_tags(txn.tags(&*channel_read), 0)? {
        let (_, tag_bytes) = tag_entry?;
        let serialized = libatomic::pristine::SerializedTag::from_bytes_wrapper(tag_bytes);
        if let Ok(tag) = serialized.to_tag() {
            let tag_merkle = tag.state;
            let tag_str = tag_merkle.to_base32();
            if tag_str.starts_with(tag_name) {
                matches.push(tag_merkle);
            }
        }
    }

    match matches.len() {
        0 => Ok(None),
        1 => Ok(Some(matches[0])),
        _ => bail!(
            "Ambiguous tag prefix '{}': matches {} tags",
            tag_name,
            matches.len()
        ),
    }
}

async fn header(
    author: Option<&str>,
    message: Option<String>,
    timestamp: Option<i64>,
) -> Result<ChangeHeader, anyhow::Error> {
    let mut authors = Vec::new();
    use libatomic::change::Author;
    let mut b = std::collections::BTreeMap::new();
    if let Some(ref a) = author {
        b.insert("name".to_string(), a.to_string());
    } else if let Some(_dir) = atomic_config::global_config_dir() {
        let k = atomic_identity::public_key(&atomic_identity::choose_identity_name().await?)?;
        b.insert("key".to_string(), k.key);
    }
    authors.push(Author(b));
    let header = ChangeHeader {
        message: message.clone().unwrap_or_else(String::new),
        authors,
        description: None,
        timestamp: if let Some(t) = timestamp {
            chrono::DateTime::from_timestamp(t, 0).unwrap()
        } else {
            chrono::Utc::now()
        },
    };
    if header.message.is_empty() {
        let toml = toml::to_string_pretty(&header)?;
        loop {
            let bytes = edit::edit_bytes(toml.as_bytes())?;
            if let Ok(header) = toml::from_slice(&bytes) {
                return Ok(header);
            }
        }
    } else {
        Ok(header)
    }
}

/// Find the last tag version in the channel
/// Currently returns default version - future enhancement will parse from tag metadata
fn find_last_tag_version<T: libatomic::pristine::TxnT>(
    _txn: &T,
    _channel: &libatomic::pristine::ChannelRef<T>,
) -> Result<String, anyhow::Error> {
    Ok("0.0.1".to_string())
}

/// Increment a semantic version string
fn increment_semver(
    version: &str,
    major: bool,
    minor: bool,
    patch: bool,
) -> Result<String, anyhow::Error> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 {
        return Err(anyhow::anyhow!(
            "Invalid semantic version format: {}",
            version
        ));
    }

    let mut major_v: u32 = parts[0]
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid major version: {}", parts[0]))?;
    let mut minor_v: u32 = parts[1]
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid minor version: {}", parts[1]))?;
    let mut patch_v: u32 = parts[2]
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid patch version: {}", parts[2]))?;

    if major {
        major_v += 1;
        minor_v = 0;
        patch_v = 0;
    } else if minor {
        minor_v += 1;
        patch_v = 0;
    } else if patch {
        patch_v += 1;
    }

    Ok(format!("{}.{}.{}", major_v, minor_v, patch_v))
}

fn try_record<T: ChannelMutTxnT + TxnT + Send + Sync + 'static>(
    repo: &mut Repository,
    txn: ArcTxn<T>,
    channel: &str,
) -> Result<(), anyhow::Error> {
    let channel = if let Some(channel) = txn.read().load_channel(channel)? {
        channel
    } else {
        bail!("Channel not found: {}", channel)
    };
    let mut state = libatomic::RecordBuilder::new();
    state.record(
        txn,
        libatomic::Algorithm::default(),
        false,
        &libatomic::DEFAULT_SEPARATOR,
        channel,
        &repo.working_copy,
        &repo.changes,
        "",
        std::thread::available_parallelism()?.get(),
    )?;
    let rec = state.finish();
    if !rec.actions.is_empty() {
        bail!("Cannot change channel, as there are unrecorded changes.")
    }
    Ok(())
}
