use std::path::PathBuf;

use super::{make_changelist, parse_changelist};
use atomic_remote::Node;

use anyhow::{anyhow, bail};
use atomic_repository::Repository;
use clap::{Parser, ValueHint};
use libatomic::changestore::ChangeStore;
use libatomic::*;
use log::debug;

#[derive(Parser, Debug)]
pub struct Unrecord {
    /// Set the repository where this command should run. Defaults to the first ancestor of the current directory that contains a `.atomic` directory.
    #[clap(long = "repository", value_hint = ValueHint::DirPath)]
    repo_path: Option<PathBuf>,
    /// Unrecord changes from this channel instead of the current channel
    #[clap(long = "channel")]
    channel: Option<String>,
    /// Also undo the changes in the working copy (preserving unrecorded changes if there are any)
    #[clap(long = "reset")]
    reset: bool,
    /// Show N changes in a text editor if no <change-id>s were given.
    /// Defaults to the value
    /// of `unrecord_changes` in your global configuration.
    #[clap(long = "show-changes", value_name = "N", conflicts_with("change_id"))]
    show_changes: Option<usize>,
    /// The hash of a change (unambiguous prefixes are accepted)
    change_id: Vec<String>,
}

impl Unrecord {
    pub fn run(self) -> Result<(), anyhow::Error> {
        let mut repo = Repository::find_root(self.repo_path)?;
        debug!("{:?}", repo.config);
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
        let is_current_channel = cur == channel_name;
        let channel = if let Some(channel) = txn.read().load_channel(&channel_name)? {
            channel
        } else {
            bail!("No such channel: {:?}", channel_name);
        };
        let mut hashes = Vec::new();

        if self.change_id.is_empty() {
            // No change ids were given, present a list for choosing
            // The number can be set in the global config or passed as a command-line option
            let number_of_changes = if let Some(n) = self.show_changes {
                n
            } else if let Some(n) = repo.config.unrecord_changes {
                n
            } else {
                let (cfg, _) = atomic_config::Global::load()?;
                cfg.unrecord_changes.ok_or_else(|| {
                    anyhow!(
                        "Can't determine how many changes to show. \
                         Please set the `unrecord_changes` option in \
                         your global config or run `atomic unrecord` \
                         with the `--show-changes` option."
                    )
                })?
            };
            let txn = txn.read();
            let hashes_ = txn
                .reverse_log(&*channel.read(), None)?
                .map(|h| {
                    let (_, (hash, state)) = h.unwrap();
                    Node::change(hash.into(), state.into())
                })
                .take(number_of_changes)
                .collect::<Vec<_>>();
            let o = make_changelist(&repo.changes, &hashes_, "unrecord")?;
            for h in parse_changelist(&edit::edit_bytes(&o[..])?, &hashes_).iter() {
                if h.is_change() {
                    let h = h.hash;
                    hashes.push((h, *txn.get_internal(&h.into())?.unwrap()))
                }
            }
        } else {
            let txn = txn.read();
            for c in self.change_id.iter() {
                let (hash, cid) = txn.hash_from_prefix(c)?;
                hashes.push((hash, cid))
            }
        };
        let channel_ = channel.read();
        let mut changes: Vec<(Hash, NodeId, Option<u64>)> = Vec::new();
        {
            let txn = txn.read();
            for (hash, change_id) in hashes {
                let n = txn
                    .get_changeset(txn.changes(&channel_), &change_id)
                    .unwrap();
                if n.is_none() {
                    bail!("Change not in channel: {:?}", hash)
                }
                changes.push((hash, change_id, n.map(|&x| x.into())));
            }
        }
        debug!("changes: {:?}", changes);
        std::mem::drop(channel_);
        let pending_hash = if self.reset {
            super::pending(txn.clone(), &channel, &mut repo)?
        } else {
            None
        };
        changes.sort_by(|a, b| b.2.cmp(&a.2));
        for (hash, change_id, _) in changes {
            let channel_ = channel.read();
            let txn_ = txn.read();
            for p in txn_.iter_revdep(&change_id)? {
                let (p, d) = p?;
                if p < &change_id {
                    continue;
                } else if p > &change_id {
                    break;
                }
                if txn_.get_changeset(txn_.changes(&channel_), d)?.is_some() {
                    let dep: Hash = txn_.get_external(d)?.unwrap().into();
                    if Some(dep) == pending_hash {
                        bail!(
                            "Cannot unrecord change {} because unrecorded changes depend on it",
                            hash.to_base32()
                        );
                    } else {
                        bail!(
                            "Cannot unrecord change {} because {} depend on it",
                            hash.to_base32(),
                            dep.to_base32()
                        );
                    }
                }
            }
            std::mem::drop(channel_);
            std::mem::drop(txn_);
            txn.write().unrecord(&repo.changes, &channel, &hash, 0)?;
        }

        if self.reset && is_current_channel {
            libatomic::output::output_repository_no_pending(
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
        }
        if let Some(h) = pending_hash {
            txn.write().unrecord(&repo.changes, &channel, &h, 0)?;
            if !cfg!(feature = "keep-changes") {
                repo.changes.del_change(&h)?;
            }
        }
        txn.commit()?;
        Ok(())
    }
}
