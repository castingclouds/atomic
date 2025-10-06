use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use anyhow::bail;
use canonical_path::CanonicalPathBuf;
use clap::{Parser, ValueHint};
use libatomic::pristine::{sanakirja::MutTxn, NodeId, ChannelMutTxnT, Position};
use libatomic::{ArcTxn, ChannelRef, ChannelTxnT, DepsTxnT, MutTxnT, TxnT, TxnTExt};
use log::*;

use atomic_interaction::{Spinner, OUTPUT_MESSAGE};
use atomic_repository::Repository;

#[derive(Parser, Debug)]
pub struct Reset {
    /// Set the repository where this command should run. Defaults to the first ancestor of the current directory that contains a `.atomic` directory.
    #[clap(long = "repository", value_hint = ValueHint::DirPath)]
    pub repo_path: Option<PathBuf>,
    /// Reset the working copy to this channel, and change the current channel to this channel.
    #[clap(long = "channel")]
    pub channel: Option<String>,
    /// Print this file to the standard output, without modifying the repository (works for a single file only).
    #[clap(long = "dry-run")]
    pub dry_run: bool,
    /// Reset even if there are unrecorded changes.
    #[clap(long = "force", short = 'f')]
    pub force: bool,
    /// Only reset these files
    #[clap(value_hint = ValueHint::FilePath)]
    pub files: Vec<PathBuf>,
}

impl Reset {
    pub fn run(self) -> Result<(), anyhow::Error> {
        let reset_overwrites_changes = atomic_config::Global::load()
            .ok()
            .and_then(|c| c.0.reset_overwrites_changes);

        let overwrite_changes = match reset_overwrites_changes.as_ref() {
            Some(atomic_config::Choice::Never) => false,
            Some(atomic_config::Choice::Auto) | Some(atomic_config::Choice::Always) | _ => true,
        };

        self.reset(overwrite_changes)
    }

    pub fn switch(self) -> Result<(), anyhow::Error> {
        self.reset(false)
    }

    fn reset(self, overwrite_changes: bool) -> Result<(), anyhow::Error> {
        use std::io::Write;
        let mut stderr = std::io::stderr();

        let has_repo_path = self.repo_path.is_some();
        let repo = Repository::find_root(self.repo_path)?;
        let txn = repo.pristine.arc_txn_begin()?;

        let cur = txn
            .read()
            .current_channel()
            .unwrap_or(libatomic::DEFAULT_CHANNEL)
            .to_string();
        let channel_name = if let Some(ref c) = self.channel {
            c
        } else {
            cur.as_str()
        };
        let repo_path = CanonicalPathBuf::canonicalize(&repo.path)?;
        let channel = if let Some(channel) = txn.read().load_channel(&channel_name)? {
            channel
        } else {
            bail!("No such channel: {:?}", channel_name)
        };

        if self.dry_run {
            if self.files.len() != 1 {
                bail!("reset --dry-run needs exactly one file");
            }
            let (pos, _ambiguous) = if has_repo_path {
                let root = std::fs::canonicalize(repo.path.join(&self.files[0]))?;
                let path = root.strip_prefix(&repo_path)?;
                use path_slash::PathExt;
                let path = path.to_slash_lossy();
                txn.read()
                    .follow_oldest_path(&repo.changes, &channel, &path)?
            } else {
                let mut root = std::env::current_dir()?;
                root.push(&self.files[0]);
                let root = std::fs::canonicalize(&root)?;
                let path = root.strip_prefix(&repo_path)?;
                use path_slash::PathExt;
                let path = path.to_slash_lossy();
                txn.read()
                    .follow_oldest_path(&repo.changes, &channel, &path)?
            };
            libatomic::output::output_file(
                &repo.changes,
                &txn,
                &channel,
                pos,
                &mut libatomic::vertex_buffer::Writer::new(std::io::stdout()),
            )?;
            return Ok(());
        }

        let current_channel = txn
            .read()
            .current_channel()
            .unwrap_or(libatomic::DEFAULT_CHANNEL)
            .to_string();
        if self.channel.as_deref() == Some(&current_channel) {
            if !overwrite_changes {
                return Ok(());
            }
        } else if self.channel.is_some() && !self.force {
            if !self.files.is_empty() {
                bail!("Cannot use --channel with individual paths. Did you mean --dry-run?")
            }
            let channel = {
                let txn = txn.read();
                txn.load_channel(&current_channel)?
            };
            if let Some(channel) = channel {
                if has_unrecorded_changes(txn.clone(), channel.clone(), &repo).unwrap() {
                    bail!("Cannot change channel, as there are unrecorded changes.")
                }
            }
        }

        let now = std::time::Instant::now();
        let mut conflicts = Vec::new();
        if self.files.is_empty() {
            if self.channel.is_none() || self.channel.as_deref() == Some(&current_channel) {
                if !overwrite_changes && !self.force {
                    writeln!(stderr, "Unrecorded changes will be lost")?;
                    writeln!(
                        stderr,
                        "Use --force to reset repository to last recorded change"
                    )?;
                    return Ok(());
                }

                let last_modified = last_modified(&*txn.read(), &*channel.read());
                if !self.force
                    && has_unrecorded_changes(txn.clone(), channel.clone(), &repo).unwrap()
                {
                    bail!("Refusing to reset, as there are unrecorded changes. Use --force")
                }
                libatomic::output::output_repository_no_pending(
                    &repo.working_copy,
                    &repo.changes,
                    &txn,
                    &channel,
                    "",
                    true,
                    Some(last_modified),
                    1, // std::thread::available_parallelism()?.get(),
                    0,
                )?;
                txn.write().touch_channel(&mut *channel.write(), None);
                txn.commit()?;

                writeln!(stderr, "Reset repository to last recorded change")?;
                return Ok(());
            }
            let mut inodes = HashSet::new();
            let mut txn_ = txn.write();
            if let Some(cur) = txn_.load_channel(&current_channel)? {
                let mut changediff = HashSet::new();
                let (a, b, s) =
                    libatomic::pristine::last_common_state(&*txn_, &*cur.read(), &*channel.read())?;
                let s: libatomic::Merkle = s.into();
                debug!("last common state {:?}", s);
                let (a, b) = if s == libatomic::Merkle::zero() {
                    (None, None)
                } else {
                    (Some(a), Some(b))
                };
                changes_after(&*txn_, &*cur.read(), a, &mut changediff, &mut inodes)?;
                changes_after(&*txn_, &*channel.read(), b, &mut changediff, &mut inodes)?;
            }

            if let Some(ref c) = self.channel {
                txn_.set_current_channel(c)?
            }
            let mut paths = BTreeSet::new();
            for pos in inodes.iter() {
                if let Some((path, _)) =
                    libatomic::fs::find_path(&repo.changes, &*txn_, &*channel.read(), false, *pos)?
                {
                    paths.insert(path);
                } else {
                    paths.clear();
                    break;
                }
            }
            if !inodes.is_empty() && paths.is_empty() {
                paths.insert(String::from(""));
            }
            let mut last = None;
            let _output_spinner = Spinner::new(OUTPUT_MESSAGE)?;
            std::mem::drop(txn_);
            for path in paths.iter() {
                match last {
                    Some(last_path) if path.starts_with(last_path) => continue,
                    _ => (),
                }
                debug!("resetting {:?}", path);
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
            txn.write().touch_channel(&mut *channel.write(), None);
        } else {
            let _output_spinner = Spinner::new(OUTPUT_MESSAGE)?;
            for root in self.files.iter() {
                let root = std::fs::canonicalize(&root)?;
                let path = root.strip_prefix(&repo_path)?;
                use path_slash::PathExt;
                let path = path.to_slash_lossy();
                conflicts.extend(
                    libatomic::output::output_repository_no_pending(
                        &repo.working_copy,
                        &repo.changes,
                        &txn,
                        &channel,
                        &path,
                        true,
                        None,
                        std::thread::available_parallelism()?.get(),
                        0,
                    )?
                    .into_iter(),
                );
            }
        }
        super::print_conflicts(&conflicts)?;
        txn.commit()?;
        debug!("now = {:?}", now.elapsed());
        let locks = libatomic::TIMERS.lock().unwrap();
        info!(
            "retrieve: {:?}, graph: {:?}, output: {:?}",
            locks.alive_retrieve, locks.alive_graph, locks.alive_output,
        );

        writeln!(stderr, "Reset given paths to last recorded change")?;

        Ok(())
    }
}

fn changes_after<T: ChannelTxnT + DepsTxnT>(
    txn: &T,
    chan: &T::Channel,
    from: Option<u64>,
    changediff: &mut HashSet<NodeId>,
    inodes: &mut HashSet<Position<NodeId>>,
) -> Result<(), anyhow::Error> {
    let f = if let Some(f) = from {
        (f + 1).into()
    } else {
        0u64.into()
    };
    for x in libatomic::pristine::changeid_log(txn, chan, f)? {
        let (n, p) = x?;
        let n: u64 = (*n).into();
        debug!("{:?} {:?} {:?}", n, p, from);
        if changediff.insert(p.a) {
            for y in txn.iter_rev_touched_files(&p.a, None)? {
                let (uu, pos) = y?;
                debug_assert!(uu >= &p.a);
                if uu > &p.a {
                    break;
                }
                inodes.insert(*pos);
            }
        }
    }
    Ok(())
}

fn last_modified<T: ChannelTxnT>(txn: &T, channel: &T::Channel) -> std::time::SystemTime {
    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(txn.last_modified(channel))
}

fn has_unrecorded_changes(
    txn: ArcTxn<MutTxn<()>>,
    channel: ChannelRef<MutTxn<()>>,
    repo: &Repository,
) -> Result<bool, anyhow::Error> {
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
    debug!("actions = {:?}", rec.actions);
    Ok(!rec.actions.is_empty())
}
