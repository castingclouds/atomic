use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::bail;
use libatomic::pristine::{Hash, Merkle, MutTxnT, NodeType, Position, TxnT};
use libatomic::*;
use log::debug;

use crate::Node;
use atomic_interaction::ProgressBar;

#[derive(Clone)]
pub struct Local {
    pub channel: String,
    pub root: std::path::PathBuf,
    pub changes_dir: std::path::PathBuf,
    pub pristine: Arc<libatomic::pristine::sanakirja::Pristine>,
    pub name: String,
}

pub fn get_state<T: TxnTExt>(
    txn: &T,
    channel: &libatomic::pristine::ChannelRef<T>,
    mid: Option<u64>,
) -> Result<Option<(u64, Merkle, Merkle)>, anyhow::Error> {
    if let Some(x) = txn.reverse_log(&*channel.read(), mid)?.next() {
        let (n, (_, m)) = x?;
        // Tags changed to TagBytes format - breaking change
        // Skip tag merkle for now, will be regenerated
        Ok(Some((n, m.into(), Merkle::zero())))
    } else {
        Ok(None)
    }
}

impl Local {
    pub fn get_state(
        &mut self,
        mid: Option<u64>,
    ) -> Result<Option<(u64, Merkle, Merkle)>, anyhow::Error> {
        let txn = self.pristine.txn_begin()?;
        let channel = txn.load_channel(&self.channel)?.unwrap();
        Ok(get_state(&txn, &channel, mid)?)
    }

    pub fn get_id(&self) -> Result<libatomic::pristine::RemoteId, anyhow::Error> {
        let txn = self.pristine.txn_begin()?;
        if let Some(channel) = txn.load_channel(&self.channel)? {
            Ok(*txn.id(&*channel.read()).unwrap())
        } else {
            Err(anyhow::anyhow!(
                "Channel {} does not exist in repository {}",
                self.channel,
                self.name
            ))
        }
    }

    pub fn download_changelist<
        A,
        F: FnMut(&mut A, u64, Hash, Merkle, bool) -> Result<(), anyhow::Error>,
    >(
        &mut self,
        f: F,
        a: &mut A,
        from: u64,
        paths: &[String],
    ) -> Result<HashSet<Position<Hash>>, anyhow::Error> {
        let remote_txn = self.pristine.txn_begin()?;
        let remote_channel = if let Some(channel) = remote_txn.load_channel(&self.channel)? {
            channel
        } else {
            debug!(
                "Local::download_changelist found no channel named {:?}",
                self.channel
            );
            bail!("No channel {} found for remote {}", self.name, self.channel)
        };
        self.download_changelist_(f, a, from, paths, &remote_txn, &remote_channel)
    }

    pub fn download_changelist_<
        A,
        T: libatomic::ChannelTxnT + libatomic::TxnTExt + libatomic::DepsTxnT + libatomic::GraphTxnT,
        F: FnMut(&mut A, u64, Hash, Merkle, bool) -> Result<(), anyhow::Error>,
    >(
        &mut self,
        mut f: F,
        a: &mut A,
        from: u64,
        paths: &[String],
        remote_txn: &T,
        remote_channel: &ChannelRef<T>,
    ) -> Result<HashSet<Position<Hash>>, anyhow::Error> {
        let store = libatomic::changestore::filesystem::FileSystem::from_root(
            &self.root,
            atomic_repository::max_files()?,
        );
        let mut paths_ = HashSet::new();
        let mut result = HashSet::new();
        for s in paths {
            if let Ok((p, _ambiguous)) = remote_txn.follow_oldest_path(&store, &remote_channel, s) {
                debug!("p = {:?}", p);

                for p in std::iter::once(p).chain(
                    libatomic::fs::iter_graph_descendants(
                        remote_txn,
                        remote_txn.graph(&*remote_channel.read()),
                        p,
                    )?
                    .map(|x| x.unwrap()),
                ) {
                    paths_.insert(p);
                    result.insert(Position {
                        change: remote_txn.get_external(&p.change)?.unwrap().into(),
                        pos: p.pos,
                    });
                }
            }
        }
        debug!("paths_ = {:?}", paths_);
        debug!("from = {:?}", from);

        let rem = remote_channel.read();
        let tags: Vec<u64> = remote_txn
            .iter_tags(remote_txn.tags(&*rem), from)?
            .map(|k| (*k.unwrap().0).into())
            .collect();
        let mut tagsi = 0;

        if paths_.is_empty() {
            for x in remote_txn.log(&*rem, from)? {
                debug!("log {:?}", x);
                let (n, (h, m)) = x?;
                assert!(n >= from);
                debug!("put_remote {:?} {:?} {:?}", n, h, m);
                if tags.get(tagsi) == Some(&n) {
                    f(a, n, h.into(), m.into(), true)?;
                    tagsi += 1;
                } else {
                    f(a, n, h.into(), m.into(), false)?;
                }
            }
        } else {
            let mut hashes = HashMap::new();
            let mut stack = Vec::new();
            for x in remote_txn.log(&*rem, from)? {
                debug!("log {:?}", x);
                let (n, (h, m)) = x?;
                assert!(n >= from);
                let h_int = remote_txn.get_internal(h)?.unwrap();
                if paths_.is_empty()
                    || paths_.iter().any(|x| {
                        let y = remote_txn.get_touched_files(x, Some(h_int)).unwrap();
                        debug!("x {:?} {:?}", x, y);
                        y == Some(h_int)
                    })
                {
                    stack.push((*h_int, *m, n));
                }
            }

            while let Some((h_int, m, n)) = stack.pop() {
                if hashes.insert(h_int, (m, n)).is_some() {
                    continue;
                }
                for d in remote_txn.iter_dep(&h_int)? {
                    let (&h_int_, &d) = d?;
                    if h_int_ < h_int {
                        continue;
                    } else if h_int_ > h_int {
                        break;
                    }
                    let n = remote_txn
                        .get_changeset(remote_txn.changes(&*rem), &d)
                        .unwrap()
                        .unwrap();
                    let m = remote_txn
                        .get_revchangeset(remote_txn.rev_changes(&*rem), &n)
                        .unwrap()
                        .unwrap()
                        .b;
                    stack.push((d, m.into(), (*n).into()))
                }
            }

            let mut hashes: Vec<_> = hashes.into_iter().collect();
            hashes.sort_by_key(|(_, (_, n))| *n);
            for (h_int, (m, n)) in hashes {
                let h = remote_txn.get_external(&h_int)?.unwrap();
                debug!("put_remote {:?} {:?} {:?}", n, h, m);
                if tags.get(tagsi) == Some(&n) {
                    f(a, n, h.into(), m.into(), true)?;
                    tagsi += 1;
                } else {
                    f(a, n, h.into(), m.into(), false)?;
                }
            }
        }
        Ok(result)
    }

    pub fn upload_nodes(
        &mut self,
        progress_bar: ProgressBar,
        mut local: PathBuf,
        to_channel: Option<&str>,
        nodes: &[Node],
    ) -> Result<(), anyhow::Error> {
        let store = libatomic::changestore::filesystem::FileSystem::from_root(
            &self.root,
            atomic_repository::max_files()?,
        );
        let txn = self.pristine.arc_txn_begin()?;
        let channel = txn
            .write()
            .open_or_create_channel(to_channel.unwrap_or(&self.channel))?;
        for node in nodes {
            match node.node_type {
                NodeType::Change => {
                    libatomic::changestore::filesystem::push_filename(&mut local, &node.hash);
                    libatomic::changestore::filesystem::push_filename(
                        &mut self.changes_dir,
                        &node.hash,
                    );
                }
                NodeType::Tag => {
                    libatomic::changestore::filesystem::push_tag_filename(&mut local, &node.state);
                    libatomic::changestore::filesystem::push_tag_filename(
                        &mut self.changes_dir,
                        &node.state,
                    );
                }
            }
            std::fs::create_dir_all(&self.changes_dir.parent().unwrap())?;
            debug!("hard link {:?} {:?}", local, self.changes_dir);
            if std::fs::metadata(&self.changes_dir).is_err() {
                if std::fs::hard_link(&local, &self.changes_dir).is_err() {
                    std::fs::copy(&local, &self.changes_dir)?;
                }
            }
            debug!("hard link done");
            libatomic::changestore::filesystem::pop_filename(&mut local);
            libatomic::changestore::filesystem::pop_filename(&mut self.changes_dir);
        }
        let repo = libatomic::working_copy::filesystem::FileSystem::from_root(&self.root);
        upload_nodes(progress_bar, &store, &mut *txn.write(), &channel, nodes)?;
        libatomic::output::output_repository_no_pending(
            &repo,
            &store,
            &txn,
            &channel,
            "",
            true,
            None,
            std::thread::available_parallelism()?.get(),
            0,
        )?;
        txn.commit()?;
        Ok(())
    }

    pub async fn download_nodes(
        &mut self,
        progress_bar: ProgressBar,
        nodes: &mut tokio::sync::mpsc::UnboundedReceiver<Node>,
        send: &mut tokio::sync::mpsc::Sender<(Node, bool)>,
        mut path: &mut PathBuf,
    ) -> Result<(), anyhow::Error> {
        while let Some(node) = nodes.recv().await {
            match node.node_type {
                NodeType::Change => {
                    libatomic::changestore::filesystem::push_filename(
                        &mut self.changes_dir,
                        &node.hash,
                    );
                    libatomic::changestore::filesystem::push_filename(&mut path, &node.hash);
                }
                NodeType::Tag => {
                    libatomic::changestore::filesystem::push_tag_filename(
                        &mut self.changes_dir,
                        &node.state,
                    );
                    libatomic::changestore::filesystem::push_tag_filename(&mut path, &node.state);
                }
            }
            progress_bar.inc(1);

            if std::fs::metadata(&path).is_ok() {
                debug!("metadata {:?} ok", path);
                libatomic::changestore::filesystem::pop_filename(&mut self.changes_dir);
                libatomic::changestore::filesystem::pop_filename(&mut path);
                send.send((node, true)).await?;
                continue;
            }
            std::fs::create_dir_all(&path.parent().unwrap())?;
            if std::fs::hard_link(&self.changes_dir, &path).is_err() {
                std::fs::copy(&self.changes_dir, &path)?;
            }
            libatomic::changestore::filesystem::pop_filename(&mut self.changes_dir);
            libatomic::changestore::filesystem::pop_filename(&mut path);
            send.send((node, true)).await?;
        }
        Ok(())
    }

    pub async fn update_identities(
        &mut self,
        _rev: Option<u64>,
        mut path: PathBuf,
    ) -> Result<u64, anyhow::Error> {
        let mut other_path = self.root.join(DOT_DIR);
        other_path.push("identities");
        let r = if let Ok(r) = std::fs::read_dir(&other_path) {
            r
        } else {
            return Ok(0);
        };
        std::fs::create_dir_all(&path)?;
        for id in r {
            let id = id?;
            let m = id.metadata()?;
            let p = id.path();
            path.push(p.file_name().unwrap());
            if let Ok(ml) = std::fs::metadata(&path) {
                if ml.modified()? < m.modified()? {
                    std::fs::remove_file(&path)?;
                } else {
                    path.pop();
                    continue;
                }
            }
            if std::fs::hard_link(&p, &path).is_err() {
                std::fs::copy(&p, &path)?;
            }
            debug!("hard link done");
            path.pop();
        }
        Ok(0)
    }
}

pub fn upload_nodes<T: MutTxnTExt + 'static, C: libatomic::changestore::ChangeStore>(
    progress_bar: ProgressBar,
    store: &C,
    txn: &mut T,
    channel: &libatomic::pristine::ChannelRef<T>,
    nodes: &[Node],
) -> Result<(), anyhow::Error> {
    let mut ws = libatomic::ApplyWorkspace::new();
    let mut channel = channel.write();
    for node in nodes {
        // Use unified apply for both changes and tags
        txn.apply_node_ws(store, &mut *channel, &node.hash, node.node_type, &mut ws)?;
        progress_bar.inc(1);
    }
    Ok(())
}
