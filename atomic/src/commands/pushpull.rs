use std::collections::{BTreeSet, HashSet};
use std::io::Write;
use std::path::PathBuf;

use super::{make_changelist, parse_changelist};
use anyhow::bail;
use clap::{Parser, ValueHint};
use lazy_static::lazy_static;
use libatomic::changestore::ChangeStore;
use libatomic::pristine::sanakirja::MutTxn;
use libatomic::pristine::TagMetadataMutTxnT;
use libatomic::*;
use log::debug;
use regex::Regex;

use atomic_interaction::{ProgressBar, Spinner, APPLY_MESSAGE, OUTPUT_MESSAGE};
use atomic_remote::{self as remote, Node, PushDelta, RemoteDelta, RemoteRepo};
use atomic_repository::Repository;

#[derive(Parser, Debug)]
pub struct Remote {
    #[clap(subcommand)]
    subcmd: Option<SubRemote>,
    /// Set the repository where this command should run. Defaults to the first ancestor of the current directory that contains a `.atomic` directory.
    #[clap(long = "repository", value_hint = ValueHint::DirPath)]
    repo_path: Option<PathBuf>,
}

#[derive(Parser, Debug)]
pub enum SubRemote {
    /// Set the default remote
    #[clap(name = "default")]
    Default { remote: String },
    /// Deletes the remote
    #[clap(name = "delete")]
    Delete { remote: String },
}

impl Remote {
    pub fn run(self) -> Result<(), anyhow::Error> {
        let repo = Repository::find_root(self.repo_path)?;
        debug!("{:?}", repo.config);
        let mut stdout = std::io::stdout();
        match self.subcmd {
            None => {
                let txn = repo.pristine.txn_begin()?;
                for r in txn.iter_remotes(&libatomic::pristine::RemoteId::nil())? {
                    let r = r?;
                    writeln!(stdout, "  {}: {}", r.id(), r.lock().path.as_str())?;
                }
            }
            Some(SubRemote::Default { remote }) => {
                let mut repo = repo;
                repo.config.default_remote = Some(remote);
                repo.update_config()?;
            }
            Some(SubRemote::Delete { remote }) => {
                let remote = if let Some(r) =
                    libatomic::pristine::RemoteId::from_base32(remote.as_bytes())
                {
                    r
                } else {
                    bail!("Could not parse identifier: {:?}", remote)
                };
                let mut txn = repo.pristine.mut_txn_begin()?;
                if !txn.drop_named_remote(remote)? {
                    bail!("Remote not found: {:?}", remote)
                } else {
                    txn.commit()?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Parser, Debug)]
pub struct Push {
    /// Path to the repository. Uses the current repository if the argument is omitted
    #[clap(long = "repository", value_hint = ValueHint::DirPath)]
    repo_path: Option<PathBuf>,
    /// Push from this channel instead of the default channel
    #[clap(long = "from-channel")]
    from_channel: Option<String>,
    /// Push all changes
    #[clap(long = "all", short = 'a', conflicts_with = "changes")]
    all: bool,
    /// Force an update of the local remote cache. May effect some
    /// reporting of unrecords/concurrent changes in the remote.
    #[clap(long = "force-cache", short = 'f')]
    force_cache: bool,
    /// Do not check certificates (HTTPS remotes only, this option might be dangerous)
    #[clap(short = 'k')]
    no_cert_check: bool,
    /// Push changes only relating to these paths
    #[clap(long = "path", value_hint = ValueHint::AnyPath)]
    path: Vec<String>,
    /// Push to this remote
    to: Option<String>,
    /// Push to this remote channel instead of the remote's default channel
    #[clap(long = "to-channel")]
    to_channel: Option<String>,
    /// Push only these changes
    #[clap(last = true)]
    changes: Vec<String>,
    /// Push attribution metadata along with changes
    #[clap(long = "with-attribution")]
    with_attribution: bool,
    /// Skip attribution sync even if configured
    #[clap(long = "skip-attribution", conflicts_with = "with_attribution")]
    skip_attribution: bool,
}

#[derive(Parser, Debug)]
pub struct Pull {
    /// Set the repository where this command should run. Defaults to the first ancestor of the current directory that contains a `.atomic` directory.
    #[clap(long = "repository", value_hint = ValueHint::DirPath)]
    repo_path: Option<PathBuf>,
    /// Pull into this channel instead of the current channel
    #[clap(long = "to-channel")]
    to_channel: Option<String>,
    /// Pull all changes
    #[clap(long = "all", short = 'a', conflicts_with = "changes")]
    all: bool,
    /// Force an update of the local remote cache. May effect some
    /// reporting of unrecords/concurrent changes in the remote.
    #[clap(long = "force-cache", short = 'f')]
    force_cache: bool,
    /// Do not check certificates (HTTPS remotes only, this option might be dangerous)
    #[clap(short = 'k')]
    no_cert_check: bool,
    /// Download full changes, even when not necessary
    #[clap(long = "full")]
    full: bool, // This can't be symmetric with push
    /// Only pull to these paths
    #[clap(long = "path", value_hint = ValueHint::AnyPath)]
    path: Vec<String>,
    /// Pull from this remote
    from: Option<String>,
    /// Pull from this remote channel
    #[clap(long = "from-channel")]
    from_channel: Option<String>,
    /// Pull changes from the local repository, not necessarily from a channel
    #[clap(last = true)]
    changes: Vec<String>, // For local changes only, can't be symmetric.
    /// Pull attribution metadata along with changes
    #[clap(long = "with-attribution")]
    with_attribution: bool,
    /// Skip attribution sync even if configured
    #[clap(long = "skip-attribution", conflicts_with = "with_attribution")]
    skip_attribution: bool,
}

lazy_static! {
    static ref CHANNEL: Regex = Regex::new(r#"([^:]*)(:(.*))?"#).unwrap();
}

impl Push {
    /// Gets the `to_upload` vector while trying to auto-update
    /// the local cache if possible. Also calculates whether the remote
    /// has any changes we don't know about.
    async fn to_upload(
        &self,
        txn: &mut MutTxn<()>,
        channel: &mut ChannelRef<MutTxn<()>>,
        repo: &Repository,
        remote: &mut RemoteRepo,
    ) -> Result<PushDelta, anyhow::Error> {
        let remote_delta = remote
            .update_changelist_pushpull(
                txn,
                &self.path,
                channel,
                Some(self.force_cache),
                repo,
                self.changes.as_slice(),
                false,
            )
            .await?;
        if let RemoteRepo::LocalChannel(ref remote_channel) = remote {
            remote_delta.to_local_channel_push(
                remote_channel,
                txn,
                self.path.as_slice(),
                channel,
                repo,
            )
        } else {
            remote_delta.to_remote_push(txn, self.path.as_slice(), channel, repo)
        }
    }

    pub async fn run(self) -> Result<(), anyhow::Error> {
        let mut stderr = std::io::stderr();
        let repo = Repository::find_root(self.repo_path.clone())?;
        debug!("{:?}", repo.config);
        let txn = repo.pristine.arc_txn_begin()?;
        let cur = txn
            .read()
            .current_channel()
            .unwrap_or(libatomic::DEFAULT_CHANNEL)
            .to_string();
        let channel_name = if let Some(ref c) = self.from_channel {
            c
        } else {
            cur.as_str()
        };
        let remote_name = if let Some(ref rem) = self.to {
            rem
        } else if let Some(ref def) = repo.config.default_remote {
            def
        } else {
            bail!("Missing remote");
        };
        let mut push_channel = None;
        let remote_channel = if let Some(ref c) = self.to_channel {
            let c = CHANNEL.captures(c).unwrap();
            push_channel = c.get(3).map(|x| x.as_str());
            let c = c.get(1).unwrap().as_str();
            if c.is_empty() {
                channel_name
            } else {
                c
            }
        } else {
            channel_name
        };
        debug!("remote_channel = {:?} {:?}", remote_channel, push_channel);
        let mut remote = remote::repository(
            &repo,
            Some(&repo.path),
            None,
            &remote_name,
            remote_channel,
            self.no_cert_check,
            true,
        )
        .await?;

        let mut channel = txn.write().open_or_create_channel(&channel_name)?;

        let PushDelta {
            to_upload,
            remote_unrecs,
            unknown_changes,
            ..
        } = self
            .to_upload(&mut *txn.write(), &mut channel, &repo, &mut remote)
            .await?;

        debug!("to_upload = {:?}", to_upload);

        if to_upload.is_empty() {
            writeln!(stderr, "Nothing to push")?;
            txn.commit()?;
            return Ok(());
        }

        notify_remote_unrecords(&repo, remote_unrecs.as_slice());
        notify_unknown_changes(unknown_changes.as_slice());

        // Handle attribution sync following AGENTS.md environment variable injection pattern
        if self.with_attribution {
            std::env::set_var("ATOMIC_ATTRIBUTION_SYNC_PUSH", "true");
        }
        if self.skip_attribution {
            std::env::set_var("ATOMIC_ATTRIBUTION_SYNC_PUSH", "false");
        }

        let to_upload = if !self.changes.is_empty() {
            let mut u: Vec<Node> = Vec::new();
            let mut not_found = Vec::new();
            let txn = txn.read();
            for change in self.changes.iter() {
                match txn.hash_from_prefix(change) {
                    Ok((hash, _)) => {
                        if to_upload.contains(&Node::change(hash, libatomic::Merkle::zero())) {
                            u.push(Node::change(hash, libatomic::Merkle::zero()));
                        }
                    }
                    Err(_) => {
                        if !not_found.contains(change) {
                            not_found.push(change.to_string());
                        }
                    }
                }
            }

            u.sort_by(|a, b| {
                if a.is_change() && b.is_change() {
                    let na = txn.get_revchanges(&channel, &a.hash).unwrap().unwrap();
                    let nb = txn.get_revchanges(&channel, &b.hash).unwrap().unwrap();
                    na.cmp(&nb)
                } else if a.is_tag() && b.is_tag() {
                    let na = txn
                        .channel_has_state(txn.states(&*channel.read()), &a.state.into())
                        .unwrap()
                        .unwrap();
                    let nb = txn
                        .channel_has_state(txn.states(&*channel.read()), &b.state.into())
                        .unwrap()
                        .unwrap();
                    na.cmp(&nb)
                } else {
                    std::cmp::Ordering::Equal
                }
            });

            if !not_found.is_empty() {
                bail!("Changes not found: {:?}", not_found)
            }

            check_deps(&repo.changes, &to_upload, &u)?;
            u
        } else if self.all {
            to_upload
        } else {
            let mut o = make_changelist(&repo.changes, &to_upload, "push")?;
            loop {
                let d = parse_changelist(&edit::edit_bytes(&o[..])?, &to_upload);
                let comp = complete_deps(&repo.changes, Some(&to_upload), &d)?;
                if comp.len() == d.len() {
                    break comp;
                }
                o = make_changelist(&repo.changes, &comp, "push")?
            }
        };
        debug!("to_upload = {:?}", to_upload);

        if to_upload.is_empty() {
            writeln!(stderr, "Nothing to push")?;
            txn.commit()?;
            return Ok(());
        }

        remote
            .upload_nodes(
                &mut *txn.write(),
                repo.changes_dir.clone(),
                push_channel,
                &to_upload,
            )
            .await?;

        debug!("Upload changes completed, committing local transaction");
        txn.commit()?;
        debug!("Local transaction committed successfully");

        debug!("Calling remote.finish()");
        remote.finish().await?;
        debug!("remote.finish() completed");
        Ok(())
    }
}

impl Pull {
    /// Gets the `to_download` vec and calculates any remote unrecords.
    /// If the local remote cache can be auto-updated, it will be.
    async fn to_download(
        &self,
        txn: &mut MutTxn<()>,
        channel: &mut ChannelRef<MutTxn<()>>,
        repo: &mut Repository,
        remote: &mut RemoteRepo,
    ) -> Result<RemoteDelta<MutTxn<()>>, anyhow::Error> {
        let force_cache = if self.force_cache {
            Some(self.force_cache)
        } else {
            None
        };
        let delta = remote
            .update_changelist_pushpull(
                txn,
                &self.path,
                channel,
                force_cache,
                repo,
                self.changes.as_slice(),
                true,
            )
            .await?;
        let to_download = remote
            .pull(
                repo,
                txn,
                channel,
                delta.to_download.as_slice(),
                &delta.inodes,
                false,
            )
            .await?;

        Ok(RemoteDelta {
            to_download,
            ..delta
        })
    }

    pub async fn run(self) -> Result<(), anyhow::Error> {
        let mut repo = Repository::find_root(self.repo_path.clone())?;
        let txn = repo.pristine.arc_txn_begin()?;
        let cur = txn
            .read()
            .current_channel()
            .unwrap_or(libatomic::DEFAULT_CHANNEL)
            .to_string();
        let channel_name = if let Some(ref c) = self.to_channel {
            c
        } else {
            cur.as_str()
        };
        let is_current_channel = channel_name == cur;
        let mut channel = txn.write().open_or_create_channel(&channel_name)?;
        debug!("{:?}", repo.config);
        let remote_name = if let Some(ref rem) = self.from {
            rem
        } else if let Some(ref def) = repo.config.default_remote {
            def
        } else {
            bail!("Missing remote")
        };
        let from_channel = if let Some(ref c) = self.from_channel {
            c
        } else {
            libatomic::DEFAULT_CHANNEL
        };
        let mut remote = remote::repository(
            &repo,
            Some(&repo.path),
            None,
            &remote_name,
            from_channel,
            self.no_cert_check,
            true,
        )
        .await?;
        debug!("downloading");

        let RemoteDelta {
            inodes,
            remote_ref,
            mut to_download,
            remote_unrecs,
            ..
        } = self
            .to_download(&mut *txn.write(), &mut channel, &mut repo, &mut remote)
            .await?;

        let hash = super::pending(txn.clone(), &mut channel, &mut repo)?;

        if let Some(ref r) = remote_ref {
            remote.update_identities(&mut repo, r).await?;
        }

        notify_remote_unrecords(&repo, remote_unrecs.as_slice());

        if to_download.is_empty() {
            let mut stderr = std::io::stderr();
            writeln!(stderr, "Nothing to pull")?;
            if let Some(ref h) = hash {
                txn.write().unrecord(&repo.changes, &mut channel, h, 0)?;
            }
            txn.commit()?;
            return Ok(());
        }

        if self.changes.is_empty() {
            if !self.all {
                let mut o = make_changelist(&repo.changes, &to_download, "pull")?;
                to_download = loop {
                    let d = parse_changelist(&edit::edit_bytes(&o[..])?, &to_download);
                    let comp = complete_deps(&repo.changes, Some(&to_download), &d)?;
                    if comp.len() == d.len() {
                        break comp;
                    }
                    o = make_changelist(&repo.changes, &comp, "pull")?
                };
            }
        } else {
            to_download = complete_deps(&repo.changes, None, &to_download)?;
        }

        // Regenerate tag files from short version after download
        // Following SSH protocol pattern: client receives SHORT tag, regenerates FULL
        for h in to_download.iter() {
            if h.is_tag() {
                let merkle = h.state;
                let mut tag_path = repo.changes_dir.clone();
                libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &merkle);

                if tag_path.exists() {
                    // Read the short tag data that was downloaded
                    let short_data = std::fs::read(&tag_path)?;

                    // Check if this is already a full tag (> 1KB) or short tag (< 500 bytes)
                    if short_data.len() < 500 {
                        debug!(
                            "Regenerating full tag from short version for {} ({} bytes -> full)",
                            merkle.to_base32(),
                            short_data.len()
                        );

                        // HTTP client already stripped the 8-byte length prefix
                        // Parse header from short version directly
                        let header =
                            libatomic::tag::read_short(std::io::Cursor::new(&short_data), &merkle)?;

                        // Regenerate full tag from our channel state
                        let temp_path = tag_path.with_extension("tmp");
                        let mut w = std::fs::File::create(&temp_path)?;
                        libatomic::tag::from_channel(&*txn.read(), &channel_name, &header, &mut w)?;

                        // Atomically replace with full tag
                        std::fs::rename(&temp_path, &tag_path)?;

                        debug!("Regenerated full tag file for {}", merkle.to_base32());
                    }
                }
            }
        }

        {
            // Now that .pull is always given `false` for `do_apply`...
            let mut ws = libatomic::ApplyWorkspace::new();
            debug!("to_download = {:#?}", to_download);
            let apply_bar = ProgressBar::new(to_download.len() as u64, APPLY_MESSAGE)?;

            let mut channel = channel.write();
            let mut txn = txn.write();

            // Unified single pass: Apply all nodes (changes and tags) in order
            for node in to_download.iter().rev() {
                debug!(
                    "Applying node {} (type: {:?})",
                    node.hash.to_base32(),
                    node.node_type
                );

                // Use unified apply for both changes and tags
                txn.apply_node_rec_ws(
                    &repo.changes,
                    &mut channel,
                    &node.hash,
                    node.node_type,
                    &mut ws,
                )?;
                apply_bar.inc(1);

                // If it's a tag, store consolidating metadata
                if node.is_tag() {
                    let s = node.state;
                    if let Some(_n) = txn.channel_has_state(&channel.states, &s.into())? {
                        // Read tag file header to get original timestamp
                        let mut tag_path = repo.changes_dir.clone();
                        libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &s);
                        let mut tag_file = libatomic::tag::OpenTagFile::open(&tag_path, &s)?;
                        let header = tag_file.header()?;
                        let original_timestamp = header.timestamp.timestamp() as u64;

                        // Calculate consolidating tag metadata
                        let start_position = {
                            let mut last_tag_pos = None;
                            for entry in txn.rev_iter_tags(txn.tags(&*channel), None)? {
                                let (pos, _merkle_pair) = entry?;
                                debug!("Found previous tag at position: {:?}", pos);
                                last_tag_pos = Some(pos);
                                break;
                            }
                            last_tag_pos.map(|p| p.0 + 1).unwrap_or(0)
                        };

                        // Collect changes from last tag onwards
                        let mut consolidated_changes = Vec::new();
                        let mut change_count = 0u64;

                        for entry in txn.log(&*channel, start_position)? {
                            let (pos, (hash, _)) = entry?;
                            let hash: libatomic::pristine::Hash = hash.into();
                            debug!("  Position {}: including change {}", pos, hash.to_base32());
                            consolidated_changes.push(hash);
                            change_count += 1;
                        }

                        debug!(
                            "Tag consolidation: {} changes since position {}",
                            change_count, start_position
                        );

                        let dependency_count_before = change_count;
                        let consolidated_change_count = change_count;

                        // Get channel name
                        let channel_name = txn.name(&*channel).to_string();

                        // Create consolidating tag metadata with original timestamp
                        let tag_hash = s;
                        let mut tag = libatomic::pristine::Tag::new(
                            tag_hash,
                            s,
                            channel_name,
                            None,
                            dependency_count_before,
                            consolidated_change_count,
                            consolidated_changes,
                        );
                        tag.consolidation_timestamp = original_timestamp;
                        // Set the change_file_hash to the merkle state
                        // This is what should be used as a dependency when recording changes after the tag
                        tag.change_file_hash = Some(s);

                        // Serialize and store consolidating tag metadata
                        let serialized = libatomic::pristine::SerializedTag::from_tag(&tag)?;

                        debug!("Storing consolidating tag metadata");
                        txn.put_tag(&tag_hash, &serialized)?;
                        debug!("Stored consolidating metadata for tag {}", s.to_base32());
                    } else {
                        debug!(
                            "Warning: Cannot add tag metadata {}: channel does not have that state",
                            s.to_base32()
                        );
                    }
                }
            }
        }

        debug!("completing changes");
        remote
            .complete_changes(&repo, &*txn.read(), &mut channel, &to_download, self.full)
            .await?;
        remote.finish().await?;

        debug!("inodes = {:?}", inodes);
        debug!("to_download: {:?}", to_download.len());
        let mut touched = HashSet::new();
        let txn_ = txn.read();
        for d in to_download.iter() {
            debug!("to_download {:?}", d);
            if d.is_change() {
                if let Some(int) = txn_.get_internal(&d.hash.into())? {
                    for inode in txn_.iter_rev_touched(int)? {
                        let (int_, inode) = inode?;
                        if int_ < int {
                            continue;
                        } else if int_ > int {
                            break;
                        }
                        let ext = libatomic::pristine::Position {
                            change: txn_.get_external(&inode.change)?.unwrap().into(),
                            pos: inode.pos,
                        };
                        if inodes.is_empty() || inodes.contains(&ext) {
                            touched.insert(*inode);
                        }
                    }
                }
            }
        }
        std::mem::drop(txn_);
        if is_current_channel {
            let mut touched_paths = BTreeSet::new();
            {
                let txn_ = txn.read();
                for &i in touched.iter() {
                    if let Some((path, _)) =
                        libatomic::fs::find_path(&repo.changes, &*txn_, &*channel.read(), false, i)?
                    {
                        touched_paths.insert(path);
                    } else {
                        touched_paths.clear();
                        break;
                    }
                }
            }
            if touched_paths.is_empty() {
                touched_paths.insert(String::from(""));
            }
            let mut last: Option<&str> = None;
            let mut conflicts = Vec::new();
            let _output_spinner = Spinner::new(OUTPUT_MESSAGE);

            for path in touched_paths.iter() {
                match last {
                    Some(last_path) => {
                        // If `last_path` is a prefix (in the path sense) of `path`, skip.
                        if last_path.len() < path.len() {
                            let (pre_last, post_last) = path.split_at(last_path.len());
                            if pre_last == last_path && post_last.starts_with("/") {
                                continue;
                            }
                        }
                    }
                    _ => (),
                }
                debug!("path = {:?}", path);
                conflicts.extend(
                    libatomic::output::output_repository_no_pending(
                        &repo.working_copy,
                        &repo.changes,
                        &txn,
                        &channel,
                        path,
                        true,
                        None,
                        std::thread::available_parallelism()?.get(),
                        0,
                    )?
                    .into_iter(),
                );
                last = Some(path)
            }

            super::print_conflicts(&conflicts)?;
        }
        if let Some(h) = hash {
            txn.write().unrecord(&repo.changes, &mut channel, &h, 0)?;
            repo.changes.del_change(&h)?;
        }

        // Handle attribution sync following AGENTS.md environment variable injection pattern
        if self.with_attribution {
            std::env::set_var("ATOMIC_ATTRIBUTION_SYNC_PULL", "true");
        }
        if self.skip_attribution {
            std::env::set_var("ATOMIC_ATTRIBUTION_SYNC_PULL", "false");
        }

        txn.commit()?;
        Ok(())
    }
}

fn complete_deps<C: ChangeStore>(
    c: &C,
    original: Option<&[Node]>,
    now: &[Node],
) -> Result<Vec<Node>, anyhow::Error> {
    debug!("complete deps {:?} {:?}", original, now);
    let original_: Option<HashSet<_>> = original.map(|original| original.iter().collect());
    let now_: HashSet<_> = now.iter().cloned().collect();
    let mut result = Vec::with_capacity(original.unwrap_or(now).len());
    let mut result_h = HashSet::with_capacity(original.unwrap_or(now).len());
    let mut stack: Vec<_> = now.iter().rev().cloned().collect();
    while let Some(h) = stack.pop() {
        stack.push(h);
        let l0 = stack.len();
        let hh = if h.is_change() {
            h.hash
        } else {
            stack.pop();
            result.push(h);
            continue;
        };
        for d in c.get_dependencies(&hh)? {
            let is_missing = now_
                .get(&Node::change(d, libatomic::Merkle::zero()))
                .is_none()
                && result_h
                    .get(&Node::change(d, libatomic::Merkle::zero()))
                    .is_none();

            debug!("complete_deps {:?} {:?}", d, is_missing);
            let is_missing = if let Some(ref original) = original_ {
                // If this is a list we submitted to the user for editing
                original
                    .get(&Node::change(d, libatomic::Merkle::zero()))
                    .is_some()
                    && is_missing
            } else {
                // Else, we were given an explicit list of patches to pull/push
                is_missing
            };
            if is_missing {
                // The user missed a dep.
                stack.push(Node::change(d, libatomic::Merkle::zero()));
            }
        }
        if stack.len() == l0 {
            // We have all dependencies.
            stack.pop();
            debug!("all deps, push");
            if result_h.insert(h) {
                result.push(h);
            }
        }
    }
    debug!("result {:?}", result);
    Ok(result)
}

fn check_deps<C: ChangeStore>(c: &C, original: &[Node], now: &[Node]) -> Result<(), anyhow::Error> {
    let original_: HashSet<_> = original.iter().collect();
    let now_: HashSet<_> = now.iter().collect();
    for n in now {
        // check that all of `now`'s deps are in now or not in original
        let n = if n.is_change() { n.hash } else { continue };
        for d in c.get_dependencies(&n)? {
            if original_
                .get(&Node::change(d, libatomic::Merkle::zero()))
                .is_some()
                && now_
                    .get(&Node::change(d, libatomic::Merkle::zero()))
                    .is_none()
            {
                bail!("Missing dependency: {:?}", n)
            }
        }
    }
    Ok(())
}

fn notify_remote_unrecords(repo: &Repository, remote_unrecs: &[(u64, Node)]) {
    use std::fmt::Write;
    if !remote_unrecs.is_empty() {
        let mut s = format!(
            "# The following changes have been unrecorded in the remote.\n\
            # This buffer is only being used to inform you of the remote change;\n\
            # your push will continue when it is closed.\n"
        );
        for (_, hash) in remote_unrecs {
            let header = if hash.is_change() {
                repo.changes.get_header(&hash.hash).unwrap()
            } else {
                repo.changes.get_tag_header(&hash.state).unwrap()
            };
            s.push_str("#\n");
            writeln!(&mut s, "#    {}", header.message).unwrap();
            writeln!(&mut s, "#    {}", header.timestamp).unwrap();
            match hash {
                _ if hash.is_change() => {
                    writeln!(&mut s, "#    {}", hash.hash.to_base32()).unwrap();
                }
                _ => {
                    writeln!(&mut s, "#    {}", hash.state.to_base32()).unwrap();
                }
            }
        }
        if let Err(e) = edit::edit(s.as_str()) {
            log::error!(
                "Notification of remote unrecords experienced an error: {}",
                e
            );
        }
    }
}

fn notify_unknown_changes(unknown_changes: &[Node]) {
    use std::fmt::Write;
    if unknown_changes.is_empty() {
        return;
    } else {
        let mut s = format!(
            "# The following changes are new in the remote\n# (and are not yet known to your local copy):\n#\n"
        );
        let rest_len = unknown_changes.len().saturating_sub(5);
        for node in unknown_changes.iter().take(5) {
            let hash = if node.is_change() {
                node.hash.to_base32()
            } else {
                node.state.to_base32()
            };
            writeln!(&mut s, "#     {}", hash).expect("Infallible write to String");
        }
        if rest_len > 0 {
            let plural = if rest_len == 1 { "" } else { "s" };
            writeln!(&mut s, "#     ... plus {} more change{}", rest_len, plural)
                .expect("Infallible write to String");
        }
        if let Err(e) = edit::edit(s.as_str()) {
            log::error!(
                "Notification of unknown changes experienced an error: {}",
                e
            );
        }
    }
}
