use std::io::Write;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::bail;
use atomic_repository::Repository;
use clap::{Parser, ValueHint};
use libatomic::{ChannelTxnT, MutTxnT, TxnT};
use log::debug;

#[derive(Parser, Debug)]
pub struct Channel {
    /// Set the repository where this command should run. Defaults to the first ancestor of the current directory that contains a `.atomic` directory.
    #[clap(long = "repository", value_hint = ValueHint::DirPath)]
    repo_path: Option<PathBuf>,
    #[clap(subcommand)]
    subcmd: Option<SubCommand>,
}

#[derive(Parser, Debug)]
pub enum SubCommand {
    /// Delete a channel.
    /// The channel must not be the current channel.
    #[clap(name = "delete")]
    Delete { delete: String },
    /// Rename a channel.
    #[clap(name = "rename")]
    Rename { from: String, to: Option<String> },
    /// Switch to a channel.
    /// There must not be unrecorded changes in the working copy.
    #[clap(name = "switch")]
    Switch {
        to: Option<String>,
        #[clap(long = "force", short = 'f')]
        force: bool,
    },
    /// Create a new, empty channel.
    #[clap(name = "new")]
    New {
        name: String,
        #[clap(long = "empty")]
        empty: bool,
        #[clap(long = "force", short = 'f')]
        force: bool,
    },
}

impl Channel {
    pub fn run(self) -> Result<(), anyhow::Error> {
        let mut stdout = std::io::stdout();
        match self.subcmd {
            None => {
                let repo = Repository::find_root(self.repo_path)?;
                let txn = repo.pristine.txn_begin()?;
                let current = txn.current_channel().ok();
                for channel in txn.channels("")? {
                    let channel = channel.read();
                    let name = txn.name(&*channel);
                    if current == Some(name) {
                        writeln!(stdout, "* {}", name)?;
                    } else {
                        writeln!(stdout, "  {}", name)?;
                    }
                }
            }
            Some(SubCommand::Delete { ref delete }) => {
                let repo = Repository::find_root(self.repo_path)?;
                let mut txn = repo.pristine.mut_txn_begin()?;
                let current = txn.current_channel().ok();
                if Some(delete.as_str()) == current {
                    bail!("Cannot delete current channel")
                }
                if !txn.drop_channel(delete)? {
                    return Err(anyhow!("Channel {} not found", delete));
                }
                txn.commit()?;
            }
            Some(SubCommand::Switch { to, force }) => {
                (crate::commands::reset::Reset {
                    repo_path: self.repo_path,
                    channel: to,
                    dry_run: false,
                    files: Vec::new(),
                    force,
                })
                .switch()?;
            }
            Some(SubCommand::Rename { ref from, ref to }) => {
                let repo = Repository::find_root(self.repo_path)?;
                let mut txn = repo.pristine.mut_txn_begin()?;
                let current = txn.current_channel().ok();
                let (from, to) = if let Some(to) = to {
                    (from.as_str(), to.as_str())
                } else if let Some(current) = current {
                    (current, from.as_str())
                } else {
                    bail!("No current channel")
                };
                let mut channel = if let Some(channel) = txn.load_channel(from)? {
                    channel
                } else {
                    bail!("No such channel: {:?}", from)
                };
                txn.rename_channel(&mut channel, to)?;
                txn.set_current_channel(&to)?;
                txn.commit()?;
            }
            Some(SubCommand::New { name, empty, force }) => {
                if empty && !force {
                    bail!("If creating an empty channel is really what you want, please use -f.")
                }
                let repo = Repository::find_root(self.repo_path)?;
                let mut txn = repo.pristine.mut_txn_begin()?;
                if txn.load_channel(&name)?.is_some() {
                    bail!("Channel {} already exists", name)
                }
                let new = txn.open_or_create_channel(&name)?;
                if !empty {
                    // Safeguard: apply the root patch if we're creating a new channel.
                    let current = txn.current_channel()?;
                    let channel = if let Some(channel) = txn.load_channel(current)? {
                        channel
                    } else {
                        bail!("No such channel: {:?}", current)
                    };
                    let ch = channel.read();
                    use libatomic::{GraphTxnT, MutTxnTExt};
                    let h = if let Some(Ok((k, v))) =
                        libatomic::pristine::changeid_log(&txn, &ch, 0u64.into())?.next()
                    {
                        debug!("initial patch on current channel: {:?} {:?}", k, v);
                        Some(txn.get_external(&v.a)?.unwrap().into())
                    } else {
                        None
                    };
                    if let Some(h) = h {
                        let mut new = new.write();
                        txn.apply_node(
                            &repo.changes,
                            &mut new,
                            &h,
                            libatomic::pristine::NodeType::Change,
                        )?;
                    }
                }
                txn.commit()?;
            }
        }
        Ok(())
    }
}
