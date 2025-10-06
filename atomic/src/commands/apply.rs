use std::path::PathBuf;

use anyhow::bail;
use clap::{Parser, ValueHint};
use libatomic::attribution::{
    ApplyAttributionContext, ApplyIntegrationConfig, AuthorId, AuthorInfo,
};
use libatomic::changestore::ChangeStore;
use libatomic::{Base32, DepsTxnT, GraphTxnT, MutTxnTExt, TxnT};
use libatomic::{HashMap, HashSet};
use log::*;

use atomic_interaction::{Spinner, OUTPUT_MESSAGE};
use atomic_repository::Repository;

#[derive(Parser, Debug)]
pub struct Apply {
    /// Set the repository where this command should run. Defaults to the first ancestor of the current directory that contains a `.atomic` directory.
    #[clap(long = "repository", value_hint = ValueHint::DirPath)]
    repo_path: Option<PathBuf>,
    /// Apply change to this channel
    #[clap(long = "channel")]
    channel: Option<String>,
    /// Only apply the dependencies of the change, not the change itself. Only applicable for a single change.
    #[clap(long = "deps-only")]
    deps_only: bool,
    /// The change that need to be applied. If this value is missing, read the change in text format on the standard input.
    change: Vec<String>,
    /// Enable AI attribution tracking during apply
    #[clap(long = "with-attribution")]
    with_attribution: bool,
    /// Show attribution information during apply
    #[clap(long = "show-attribution")]
    show_attribution: bool,
}

impl Apply {
    pub fn run(self) -> Result<(), anyhow::Error> {
        let repo = Repository::find_root(self.repo_path)?;

        // Initialize attribution context if requested
        let mut attribution_context = if self.with_attribution || self.show_attribution {
            let config = ApplyIntegrationConfig {
                enabled: true,
                auto_detect_ai: true,
                validate_chains: true,
                default_author: AuthorInfo {
                    id: AuthorId::new(0),
                    name: "Unknown User".to_string(),
                    email: "unknown@localhost".to_string(),
                    is_ai: false,
                },
            };

            // Create attribution context with database persistence
            Some(ApplyAttributionContext::with_database(
                config,
                repo.pristine.clone(),
            )?)
        } else {
            None
        };

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
        let is_current_channel = channel_name == cur;
        let channel = if let Some(channel) = txn.read().load_channel(&channel_name)? {
            channel
        } else {
            bail!("Channel {:?} not found", channel_name)
        };

        let mut hashes = Vec::new();
        if self.change.is_empty() {
            let mut change = std::io::BufReader::new(std::io::stdin());
            let mut change = libatomic::change::Change::read(&mut change, &mut HashMap::default())?;
            hashes.push(
                repo.changes
                    .save_change(&mut change, |_, _| Ok::<_, anyhow::Error>(()))?,
            )
        }

        use libatomic::MutTxnT;
        use rand::Rng;
        // Forked channel before the apply, in order to check whether
        // we are overwriting a path.
        let forked = if is_current_channel {
            let forked_s: String = rand::thread_rng()
                .sample_iter(&rand::distributions::Alphanumeric)
                .take(20)
                .map(char::from)
                .collect();
            let forked = txn.write().fork(&channel, &forked_s)?;
            Some((forked_s, forked))
        } else {
            None
        };
        for ch in self.change.iter() {
            hashes.push(if let Ok(h) = txn.read().hash_from_prefix(ch) {
                h.0
            } else {
                let change = libatomic::change::Change::deserialize(&ch, None);
                match change {
                    Ok(mut change) => repo
                        .changes
                        .save_change(&mut change, |_, _| Ok::<_, anyhow::Error>(()))?,
                    Err(libatomic::change::ChangeError::Io(e)) => {
                        if let std::io::ErrorKind::NotFound = e.kind() {
                            let mut changes = repo.changes_dir.clone();
                            super::find_hash(&mut changes, &ch)?
                        } else {
                            return Err(e.into());
                        }
                    }
                    Err(e) => return Err(e.into()),
                }
            })
        }
        if self.deps_only {
            if hashes.len() > 1 {
                bail!("--deps-only is only applicable to a single change")
            }
            let mut channel = channel.write();
            txn.write()
                .apply_deps_rec(&repo.changes, &mut channel, hashes.last().unwrap())?;
        } else {
            let mut channel = channel.write();
            let mut txn = txn.write();
            for hash in hashes.iter() {
                // Pre-apply attribution hook
                if let Some(ref mut ctx) = attribution_context {
                    if let Ok(change) = repo.changes.get_change(hash) {
                        if let Ok(Some(attributed_patch)) = ctx.pre_apply_hook(&change, hash) {
                            if self.show_attribution {
                                println!("Applying change with attribution:");
                                println!("  Hash: {}", hash.to_base32());
                                println!(
                                    "  Author: {} ({})",
                                    attributed_patch.author.name, attributed_patch.author.email
                                );
                                println!("  AI-Assisted: {}", attributed_patch.ai_assisted);
                                if let Some(ref ai_meta) = attributed_patch.ai_metadata {
                                    println!("  AI Provider: {}", ai_meta.provider);
                                    println!("  AI Model: {}", ai_meta.model);
                                }
                                println!();
                            }
                        }
                    }
                }

                let _result = txn.apply_node_rec(
                    &repo.changes,
                    &mut channel,
                    hash,
                    libatomic::pristine::NodeType::Change,
                )?;
                let apply_result = (0u64, libatomic::Merkle::zero());

                // Post-apply attribution hook
                if let Some(ref mut ctx) = attribution_context {
                    use libatomic::pristine::Base32;
                    let patch_id = libatomic::attribution::PatchId::from(
                        libatomic::pristine::NodeId::from_base32(hash.to_base32().as_bytes())
                            .unwrap_or(libatomic::pristine::NodeId::ROOT),
                    );
                    let _ = ctx.post_apply_hook(&patch_id, &apply_result);
                }
            }
        }

        let mut touched = HashSet::default();
        let txn_ = txn.read();
        for d in hashes.iter() {
            if let Some(int) = txn_.get_internal(&d.into())? {
                debug!("int = {:?}", int);
                for inode in txn_.iter_rev_touched(int)? {
                    debug!("{:?}", inode);
                    let (int_, inode) = inode?;
                    if int_ < int {
                        continue;
                    } else if int_ > int {
                        break;
                    }
                    touched.insert(*inode);
                }
            }
        }
        std::mem::drop(txn_);

        if let Some((_, ref forked)) = forked {
            let mut touched_files = Vec::with_capacity(touched.len());
            let txn_ = txn.read();
            for i in touched {
                if let Some((path, _)) =
                    libatomic::fs::find_path(&repo.changes, &*txn_, &*forked.read(), false, i)?
                {
                    if !path.is_empty() {
                        touched_files.push(path);
                        continue;
                    }
                }
                touched_files.clear();
                break;
            }
            debug!("touched files {:?}", touched_files);
            std::mem::drop(txn_);
            let _output_spinner = Spinner::new(OUTPUT_MESSAGE)?;

            {
                let mut state = libatomic::RecordBuilder::new();
                if touched_files.is_empty() {
                    state.record(
                        txn.clone(),
                        libatomic::Algorithm::default(),
                        false,
                        &libatomic::DEFAULT_SEPARATOR,
                        forked.clone(),
                        &repo.working_copy,
                        &repo.changes,
                        "",
                        std::thread::available_parallelism()?.get(),
                    )?
                } else {
                    use canonical_path::CanonicalPathBuf;
                    fill_relative_prefixes(&mut touched_files)?;
                    repo.working_copy.record_prefixes(
                        txn.clone(),
                        libatomic::Algorithm::default(),
                        forked.clone(),
                        &repo.changes,
                        &mut state,
                        CanonicalPathBuf::canonicalize(&repo.path)?,
                        &touched_files,
                        false,
                        std::thread::available_parallelism()?.get(),
                        0,
                    )?;
                }
                let rec = state.finish();
                if !rec.actions.is_empty() {
                    debug!("actions {:#?}", rec.actions);
                    bail!("Applying this patch would delete unrecorded changes, aborting")
                }
            }

            let mut conflicts = Vec::new();
            for path in touched_files.iter() {
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
            if !touched_files.is_empty() {
                conflicts.extend(
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
                    )?
                    .into_iter(),
                );
            }
            super::print_conflicts(&conflicts)?;
        }
        if let Some((forked_s, forked)) = forked {
            std::mem::drop(forked);
            txn.write().drop_channel(&forked_s)?;
        }

        // Show attribution statistics if context was used
        if let Some(ref ctx) = attribution_context {
            if self.show_attribution {
                let stats = ctx.get_attribution_stats();
                println!("\n=== Apply Attribution Summary ===");
                println!("Total patches applied: {}", stats.total_patches);
                println!("AI-assisted patches: {}", stats.ai_assisted_patches);
                println!("Human patches: {}", stats.human_patches);
                if stats.ai_assisted_patches > 0 {
                    println!(
                        "Average AI confidence: {:.1}%",
                        stats.average_ai_confidence * 100.0
                    );
                }
            }
        }

        txn.commit()?;
        Ok(())
    }
}

fn fill_relative_prefixes(prefixes: &mut [String]) -> Result<Vec<PathBuf>, anyhow::Error> {
    let cwd = std::env::current_dir()?;
    let mut pref = Vec::new();
    for p in prefixes.iter_mut() {
        if std::path::Path::new(p).is_relative() {
            pref.push(cwd.join(&p));
        }
    }
    Ok(pref)
}
