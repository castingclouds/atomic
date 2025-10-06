use clap::{Parser, ValueHint};
use libatomic::*;
use std::io::Write;
use std::path::PathBuf;

use atomic_repository::*;

#[derive(Parser, Debug)]
pub struct Dependents {
    /// Use the repository at PATH instead of the current directory
    #[clap(long = "repository", value_name = "PATH", value_hint = ValueHint::DirPath)]
    repo_path: Option<PathBuf>,
    /// The hash of the change to show, or an unambiguous prefix thereof
    #[clap(value_name = "HASH")]
    hash: Option<String>,
}

impl Dependents {
    pub fn run(self) -> Result<(), anyhow::Error> {
        let repo = Repository::find_root(self.repo_path.clone())?;
        let txn = repo.pristine.txn_begin()?;
        let channel_name = txn.current_channel().unwrap_or(libatomic::DEFAULT_CHANNEL);
        let channel = if let Some(channel) = txn.load_channel(&channel_name)? {
            channel
        } else {
            return Ok(());
        };
        let channelr = channel.read();

        let hash = if let Some(hash) = self.hash {
            let h = if let Some(h) = Hash::from_base32(hash.as_bytes()) {
                h
            } else {
                txn.hash_from_prefix(&hash)?.0
            };
            if txn.has_change(&channel, &h.into())?.is_none() {
                return Ok(());
            }
            h
        } else {
            return Ok(());
        };

        if hash.is_none() {
            eprintln!("Warning: listing dependents of the root change")
        }

        let mut ids = vec![(txn.get_internal(&hash.into())?.unwrap(), 0u64, false)];
        let mut seen = HashSet::new();
        let mut stdout = std::io::stdout();
        while let Some((id, n, v)) = ids.pop() {
            if v {
                let h: Hash = txn.get_external(&id)?.unwrap().into();
                writeln!(stdout, "{}", h.to_base32())?;
            } else if seen.insert(id) {
                ids.push((id, n, true));
                let l = ids.len();
                for t in txn.iter_revdep(&id).unwrap() {
                    let (id_, t) = t?;
                    if id_ > id {
                        break;
                    }
                    if let Some(n) = txn.get_changeset(txn.changes(&channelr), t)? {
                        ids.push((t, (*n).into(), false));
                    }
                }
                (&mut ids[l..]).sort_by(|a, b| a.1.cmp(&b.1));
            }
        }
        Ok(())
    }
}
