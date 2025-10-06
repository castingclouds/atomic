use clap::{Parser, ValueHint};
use libatomic::{Base32, ChannelTxnT, MutTxnT, MutTxnTExt, TxnT, TxnTExt};
use log::debug;
use std::path::PathBuf;

use atomic_repository::Repository;

#[derive(Parser, Debug)]
pub struct Fork {
    /// Set the repository where this command should run. Defaults to the first ancestor of the current directory that contains a `.atomic` directory.
    #[clap(long = "repository", value_hint = ValueHint::DirPath)]
    repo_path: Option<PathBuf>,
    /// Make the new channel from this state instead of the current channel
    #[clap(long = "state", conflicts_with = "change", conflicts_with = "channel")]
    state: Option<String>,
    /// Make the new channel from this channel instead of the current channel
    #[clap(long = "channel", conflicts_with = "change", conflicts_with = "state")]
    channel: Option<String>,
    /// Apply this change after creating the channel
    #[clap(long = "change", conflicts_with = "channel", conflicts_with = "state")]
    change: Option<String>,
    /// The name of the new channel
    to: String,
}

impl Fork {
    pub fn run(self) -> Result<(), anyhow::Error> {
        let repo = Repository::find_root(self.repo_path)?;
        debug!("{:?}", repo.config);
        let mut txn = repo.pristine.mut_txn_begin()?;
        if let Some(ref ch) = self.change {
            let (hash, _) = txn.hash_from_prefix(ch)?;
            let channel = txn.open_or_create_channel(&self.to)?;
            let mut channel = channel.write();
            txn.apply_node_rec(
                &repo.changes,
                &mut channel,
                &hash,
                libatomic::pristine::NodeType::Change,
            )?
        } else {
            let mut fork = if let Some(ref channel_name) = self.channel {
                if let Some(channel) = txn.load_channel(channel_name)? {
                    txn.fork(&channel, &self.to)?
                } else {
                    anyhow::bail!("Channel not found: {:?}", channel_name);
                }
            } else {
                let cur = txn
                    .current_channel()
                    .unwrap_or(libatomic::DEFAULT_CHANNEL)
                    .to_string();
                if let Some(channel) = txn.load_channel(&cur)? {
                    txn.fork(&channel, &self.to)?
                } else {
                    anyhow::bail!("Channel not found: {:?}", cur);
                }
            };

            if let Some(ref state) = self.state {
                if let Some(state) = libatomic::Merkle::from_base32(state.as_bytes()) {
                    let ch = fork.write();
                    if let Some(n) = txn.channel_has_state(&ch.states, &state.into())? {
                        let n: u64 = n.into();

                        let mut v = Vec::new();
                        for l in txn.reverse_log(&ch, None)? {
                            let (n_, h) = l?;
                            if n_ > n {
                                v.push(h.0.into())
                            } else {
                                break;
                            }
                        }
                        std::mem::drop(ch);
                        for h in v {
                            txn.unrecord(&repo.changes, &mut fork, &h, 0)?;
                        }
                    }
                }
            }
        }
        txn.commit()?;
        Ok(())
    }
}
