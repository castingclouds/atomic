//! Output the pristine to the working copy, synchronising file
//! changes (file additions, deletions and renames) in the process.
use super::{collect_children, OutputError, OutputItem, PristineOutputError};
use crate::alive::retrieve;
use crate::alive::Redundant;
use crate::changestore::ChangeStore;
use crate::fs::create_new_inode;
use crate::pristine::*;
use crate::small_string::SmallString;
use crate::working_copy::WorkingCopy;
use crate::{alive, path, vertex_buffer};
use crate::{HashMap, HashSet};

use std::collections::{hash_map::Entry, BTreeSet};
use std::sync::Arc;

/// A structure representing a file with conflicts.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Conflict {
    Name {
        path: String,
        inodes: Vec<Position<NodeId>>,
        changes: Vec<Hash>,
    },
    ZombieFile {
        path: String,
        inode: [Position<NodeId>; 1],
        changes: Vec<Hash>,
    },
    MultipleNames {
        path: String,
        pos: [Position<NodeId>; 1],
        names: Vec<Vertex<NodeId>>,
        changes: Vec<Hash>,
    },
    Zombie {
        path: String,
        inode_vertex: [Position<NodeId>; 1],
        line: usize,
        changes: Vec<Hash>,
        id: usize,
    },
    Cyclic {
        path: String,
        inode_vertex: [Position<NodeId>; 1],
        line: usize,
        changes: Vec<Hash>,
        id: usize,
    },
    Order {
        path: String,
        inode_vertex: [Position<NodeId>; 1],
        line: usize,
        changes: Vec<Hash>,
        id: usize,
    },
}

impl Conflict {
    pub fn changes(&self) -> &[Hash] {
        match self {
            Conflict::Name { ref changes, .. } => changes,
            Conflict::ZombieFile { ref changes, .. } => changes,
            Conflict::MultipleNames { ref changes, .. } => changes,
            Conflict::Zombie { ref changes, .. } => changes,
            Conflict::Cyclic { ref changes, .. } => changes,
            Conflict::Order { ref changes, .. } => changes,
        }
    }

    pub fn inodes(&self) -> &[Position<NodeId>] {
        match self {
            Conflict::Name { ref inodes, .. } => inodes,
            Conflict::ZombieFile { ref inode, .. } => inode,
            Conflict::MultipleNames { ref pos, .. } => pos,
            Conflict::Zombie {
                ref inode_vertex, ..
            } => inode_vertex,
            Conflict::Cyclic {
                ref inode_vertex, ..
            } => inode_vertex,
            Conflict::Order {
                ref inode_vertex, ..
            } => inode_vertex,
        }
    }
}

/// Output updates the working copy after applying changes, including
/// the graph-file correspondence.
///
/// **WARNING:** This overwrites the working copy, cancelling any
/// unrecorded change.
pub fn output_repository_no_pending<
    T: ChannelMutTxnT + TreeMutTxnT<TreeError = T::GraphError> + Send + Sync + 'static,
    R: WorkingCopy + Send + Clone + Sync + 'static,
    P: ChangeStore + Send + Clone + 'static,
>(
    repo: &R,
    changes: &P,
    txn: &ArcTxn<T>,
    channel: &ChannelRef<T>,
    prefix: &str,
    output_name_conflicts: bool,
    if_modified_since: Option<std::time::SystemTime>,
    n_workers: usize,
    salt: u64,
) -> Result<BTreeSet<Conflict>, OutputError<P::Error, T, R::Error>>
where
    T::Channel: Send + Sync + 'static,
{
    debug!("output_repository_no_pending: {:?}", prefix);
    let (c, f) = output_repository(
        repo,
        changes,
        txn.clone(),
        channel.clone(),
        NodeId::ROOT,
        &mut crate::path::components(prefix),
        output_name_conflicts,
        if_modified_since,
        n_workers,
        salt,
    )?;

    del_redundant(txn.clone(), channel.clone(), &f)?;
    Ok(c)
}

/// Output updates the working copy after applying changes, including
/// the graph-file correspondence.
///
/// **WARNING:** This overwrites the working copy, cancelling any
/// unrecorded change.
pub fn output_repository_no_pending_<
    T: ChannelTxnT + TreeMutTxnT + Send + Sync + 'static,
    R: WorkingCopy + Send + Clone + Sync + 'static,
    P: ChangeStore + Send + Clone + 'static,
>(
    repo: &R,
    changes: &P,
    txn: &ArcTxn<T>,
    channel: &ChannelRef<T>,
    prefix: &str,
    output_name_conflicts: bool,
    if_modified_since: Option<std::time::SystemTime>,
    n_workers: usize,
    salt: u64,
) -> Result<BTreeSet<Conflict>, OutputError<P::Error, T, R::Error>>
where
    T::Channel: Send + Sync + 'static,
{
    debug!("output_repository_no_pending: {:?}", prefix);
    let (c, _) = output_repository(
        repo,
        changes,
        txn.clone(),
        channel.clone(),
        NodeId::ROOT,
        &mut crate::path::components(prefix),
        output_name_conflicts,
        if_modified_since,
        n_workers,
        salt,
    )?;
    Ok(c)
}

fn output_loop<
    T: TreeMutTxnT + ChannelTxnT + GraphTxnT,
    R: WorkingCopy + Clone + 'static,
    P: ChangeStore + Clone + Send,
>(
    repo: &R,
    changes: &P,
    txn: ArcTxn<T>,
    channel: ChannelRef<T>,
    work: Arc<crossbeam_deque::Injector<(OutputItem, Inode, String, Option<String>)>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    t: usize,
) -> Result<(Vec<Conflict>, Vec<Redundant>), OutputError<P::Error, T, R::Error>> {
    use crossbeam_deque::*;
    // let backoff = crossbeam_utils::Backoff::new();
    // let w: Worker<(OutputItem, String)> = Worker::new_fifo();
    let mut conflicts = Vec::new();
    let mut forward = Vec::new();
    loop {
        match work.steal() {
            Steal::Success((item, inode, path, tmp)) => {
                info!("Outputting {:?} (tmp {:?}), on thread {}", path, tmp, t);
                let path = tmp.as_deref().unwrap_or(&path);
                output_item::<_, _, R>(
                    txn.clone(),
                    channel.clone(),
                    changes,
                    &item,
                    &mut conflicts,
                    &repo,
                    inode,
                    path,
                    &mut forward,
                )?;
                debug!("setting permissions for {:?}", path);
                repo.set_permissions(path, item.meta.permissions())
                    .map_err(OutputError::WorkingCopy)?;
                debug!("output {:?}", path);
            }
            Steal::Retry => {}
            Steal::Empty => {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
            }
        }
    }
    Ok((conflicts, forward))
}

fn output_repository<
    'a,
    T: TreeMutTxnT + ChannelTxnT + GraphTxnT + Send + Sync + 'static,
    R: WorkingCopy + Clone + Send + Sync + 'static,
    P: ChangeStore + Send + Clone + 'static,
    I: Iterator<Item = &'a str>,
>(
    repo: &R,
    changes: &P,
    txn: ArcTxn<T>,
    channel: ChannelRef<T>,
    pending_change_id: NodeId,
    prefix: &mut I,
    output_name_conflicts: bool,
    if_modified_after: Option<std::time::SystemTime>,
    n_workers: usize,
    salt: u64,
) -> Result<(BTreeSet<Conflict>, Vec<Redundant>), OutputError<P::Error, T, R::Error>>
where
    T::Channel: Send + Sync + 'static,
{
    let work = Arc::new(crossbeam_deque::Injector::new());
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut threads = Vec::new();
    for t in 0..n_workers - 1 {
        let repo = repo.clone();
        let work = work.clone();
        let stop = stop.clone();
        let txn = txn.clone();
        let channel = channel.clone();
        let changes = changes.clone();
        threads.push(std::thread::spawn(move || {
            output_loop(&repo, &changes, txn, channel, work, stop, t + 1)
        }))
    }

    let mut state = OutputState {
        done_vertices: HashMap::default(),
        actual_moves: Vec::new(),
        move_map: HashMap::new(),
        conflicts: BTreeSet::new(),

        output_name_conflicts,
        work: work.clone(),
        done_inodes: HashSet::new(),
        salt,
        if_modified_after,
        next_prefix_basename: prefix.next(),
        is_following_prefix: true,
        pending_change_id,
        redundant: Vec::new(),
    };

    let mut files = HashMap::default();
    let mut next_files = HashMap::default();
    state.kill_dead_files::<_, _, P>(repo, &txn, &channel)?;
    {
        let txn = txn.read();
        let channel = channel.read();
        collect_children(
            &*txn,
            &*changes,
            txn.graph(&*channel),
            Position::ROOT,
            Inode::ROOT,
            "",
            None,
            state.next_prefix_basename,
            &mut files,
        )?;
    }
    debug!("done collecting: {:?}", files);
    // Actual moves is used to avoid a situation where have two files
    // a and b, first rename a -> b, and then b -> c.
    while !files.is_empty() {
        debug!("files {:?}", files.len());
        next_files.clear();
        state.next_prefix_basename = prefix.next();
        for (a, mut b) in files.drain() {
            sort_conflicting_names(&txn, &channel, &mut b);
            state.output_name(repo, changes, &txn, &channel, &mut next_files, a, b)?;
        }
        std::mem::swap(&mut files, &mut next_files);
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let o = output_loop(repo, changes, txn.clone(), channel, work, stop, 0);
    for t in threads {
        let (a, b) = t.join().unwrap()?;
        for x in a.into_iter() {
            state.conflicts.insert(x);
        }
        for x in b.into_iter() {
            state.redundant.push(x);
        }
    }
    let (a, b) = o?;
    for x in a.into_iter() {
        state.conflicts.insert(x);
    }
    for x in b.into_iter() {
        state.redundant.push(x);
    }
    // Since we did a depth-first search of the output paths, we need
    // to move in reverse order of the search.
    for (a, b) in state.actual_moves.iter().rev() {
        debug!("actual move: {:?} {:?}", a, b);
        repo.rename(a, b).map_err(OutputError::WorkingCopy)?
    }

    let txn_ = txn.read();
    for (pos, (_, path, names)) in state.done_vertices {
        if !names.is_empty() {
            state.conflicts.insert(Conflict::MultipleNames {
                changes: names
                    .iter()
                    .map(|v| txn_.get_external(&v.change).unwrap().unwrap().into())
                    .collect(),
                pos: [pos],
                names,
                path,
            });
        }
    }

    Ok((state.conflicts, state.redundant))
}

fn sort_conflicting_names<T: ChannelTxnT + Send + Sync + 'static>(
    txn: &ArcTxn<T>,
    channel: &ChannelRef<T>,
    b: &mut [(Vertex<NodeId>, OutputItem)],
) {
    debug!("files: {:?}", b);
    let txn = txn.read();
    let channel = channel.read();
    b.sort_unstable_by(|u, v| {
        txn.get_changeset(txn.changes(&channel), &u.0.change)
            .unwrap()
            .cmp(
                &txn.get_changeset(txn.changes(&channel), &v.0.change)
                    .unwrap(),
            )
    });
}

struct OutputState<'a> {
    actual_moves: Vec<(String, String)>,
    move_map: HashMap<Inode, String>,
    output_name_conflicts: bool,
    done_vertices: HashMap<Position<NodeId>, (Vertex<NodeId>, String, Vec<Vertex<NodeId>>)>,

    conflicts: BTreeSet<Conflict>,

    work: Arc<crossbeam_deque::Injector<(OutputItem, Inode, String, Option<String>)>>,
    done_inodes: HashSet<Inode>,
    salt: u64,
    if_modified_after: Option<std::time::SystemTime>,
    next_prefix_basename: Option<&'a str>,
    is_following_prefix: bool,
    pending_change_id: NodeId,
    redundant: Vec<Redundant>,
}

impl<'a> OutputState<'a> {
    fn kill_dead_files<
        T: TreeMutTxnT + ChannelTxnT + GraphTxnT + Send + Sync + 'static,
        R: WorkingCopy + Clone + Send + Sync + 'static,
        P: ChangeStore + Clone + 'static,
    >(
        &mut self,
        repo: &R,
        txn: &ArcTxn<T>,
        channel: &ChannelRef<T>,
    ) -> Result<(), OutputError<P::Error, T, R::Error>> {
        if self.next_prefix_basename.is_none() && self.is_following_prefix {
            let dead = {
                let txn_ = txn.read();
                let channel = channel.read();
                let graph = txn_.graph(&*channel);
                collect_dead_files::<_, R, P>(
                    &*txn_,
                    graph,
                    &self.move_map,
                    self.pending_change_id,
                    Inode::ROOT,
                )?
            };
            debug!("dead (line {}) = {:?}", line!(), dead);
            if !dead.is_empty() {
                let mut txn = txn.write();
                kill_dead_files::<T, R, P>(&mut *txn, &channel, &repo, &dead)?;
            }
            self.is_following_prefix = false;
        }
        Ok(())
    }

    fn make_inode(
        &mut self,
        a: &str,
        name_key: Vertex<NodeId>,
        output_item: &mut OutputItem,
        is_first_name: &mut Option<Position<NodeId>>,
        name_conflict: &mut Vec<Position<NodeId>>,
    ) -> MakeInode {
        let name_entry = match self.done_vertices.entry(output_item.pos) {
            Entry::Occupied(mut e) => {
                debug!(
                    "pos already visited: {:?} {:?} {:?} {:?}",
                    a,
                    output_item.pos,
                    e.get(),
                    name_key
                );
                let e = e.get_mut();
                if e.0 != name_key {
                    if e.2.is_empty() {
                        e.2.push(e.0)
                    }
                    // The same inode has more than one name.
                    e.2.push(name_key)
                }
                return MakeInode::AlreadyOutput;
            }
            Entry::Vacant(e) => {
                debug!("first visit {:?} {:?}", a, output_item.pos);
                e
            }
        };
        let name = if let Some(ref inode) = is_first_name {
            if name_conflict.is_empty() {
                name_conflict.push(*inode)
            }
            name_conflict.push(output_item.pos);

            // Multiple inodes share the same name.
            if self.output_name_conflicts {
                let name = make_conflicting_name(&a, name_key);
                name
            } else {
                return MakeInode::NameConflict;
            }
        } else {
            *is_first_name = Some(output_item.pos);
            a.to_string()
        };
        debug!("name = {:?} {:?}", name, name_key);
        let file_name = path::file_name(&name).unwrap();
        path::push(&mut output_item.path, file_name);
        name_entry.insert((name_key, output_item.path.clone(), Vec::new()));
        MakeInode::Ok(name)
    }

    fn output_name<
        T: TreeMutTxnT + ChannelTxnT + GraphTxnT + Send + Sync + 'static,
        R: WorkingCopy + Clone + Send + Sync + 'static,
        P: ChangeStore + Send + Clone + 'static,
    >(
        &mut self,
        repo: &R,
        changes: &P,
        txn: &ArcTxn<T>,
        channel: &ChannelRef<T>,
        next_files: &mut HashMap<String, Vec<(Vertex<NodeId>, OutputItem)>>,
        a: String,
        b: Vec<(Vertex<NodeId>, OutputItem)>,
    ) -> Result<(), OutputError<P::Error, T, R::Error>> {
        let mut is_first_name = None;
        let mut name_conflict = Vec::new();
        for (name_key, mut output_item) in b {
            debug!("name_key = {:?} {:?}", name_key, output_item);
            let name = match self.make_inode(
                &a,
                name_key,
                &mut output_item,
                &mut is_first_name,
                &mut name_conflict,
            ) {
                MakeInode::Ok(file_name) => file_name,
                MakeInode::AlreadyOutput => continue,
                MakeInode::NameConflict => break,
            };
            let output_item_inode = {
                let txn = txn.read();
                if let Some(inode) = txn.get_revinodes(&output_item.pos, None)? {
                    if !self.done_inodes.insert(*inode) {
                        debug!("inode already visited: {:?} {:?}", a, inode);
                        continue;
                    }
                    Some((*inode, *txn.get_inodes(inode, None)?.unwrap()))
                } else {
                    None
                }
            };

            let file_name = path::file_name(&name).unwrap();
            let mut tmp = output_item.tmp.take().map(|mut tmp| {
                path::push(&mut tmp, file_name);
                tmp
            });
            let path = std::mem::replace(&mut output_item.path, String::new());
            let inode = move_or_create::<T, R, P>(
                txn.clone(),
                &repo,
                &output_item,
                output_item_inode,
                &path,
                &mut tmp,
                &file_name,
                &mut self.actual_moves,
                &mut self.move_map,
                self.salt,
            )?;
            debug!("inode = {:?}", inode);
            self.kill_dead_files::<_, _, P>(repo, txn, channel)?;
            if output_item.meta.is_dir() {
                if !path.is_empty() {
                    let tmp_ = tmp.as_deref().unwrap_or(&path);
                    repo.create_dir_all(tmp_)
                        .map_err(OutputError::WorkingCopy)?;
                    repo.set_permissions(tmp_, output_item.meta.permissions())
                        .map_err(OutputError::WorkingCopy)?;
                }
                let txn = txn.read();
                let channel = channel.read();
                collect_children(
                    &*txn,
                    &*changes,
                    txn.graph(&*channel),
                    output_item.pos,
                    inode,
                    &path,
                    tmp.as_deref(),
                    self.next_prefix_basename,
                    next_files,
                )?;
                debug!("next_files {:?}", next_files);
            } else {
                if needs_output(repo, self.if_modified_after, &path) {
                    self.work
                        .push((output_item.clone(), inode, path.clone(), tmp.clone()));
                } else {
                    debug!("Not outputting {:?}", path)
                }
            }
            if let Some(id) = output_item.is_zombie.take() {
                self.conflicts.insert(Conflict::ZombieFile {
                    path: path.clone(),
                    changes: id,
                    inode: [output_item.pos],
                });
            }
        }
        if !name_conflict.is_empty() {
            let txn = txn.read();
            self.conflicts.insert(Conflict::Name {
                changes: name_conflict
                    .iter()
                    .map(|v| txn.get_external(&v.change).unwrap().unwrap().into())
                    .collect(),
                path: a.clone(),
                inodes: name_conflict,
            });
        }
        Ok(())
    }
}

enum MakeInode {
    AlreadyOutput,
    NameConflict,
    Ok(String),
}

fn make_conflicting_name(name: &str, name_key: Vertex<NodeId>) -> String {
    let parent = path::parent(name).unwrap();
    let basename = path::file_name(name).unwrap();
    let mut parent = parent.to_string();
    path::push(
        &mut parent,
        &format!("{}.{}", basename, &name_key.change.to_base32()),
    );
    parent
}

fn needs_output<R: WorkingCopy>(
    repo: &R,
    if_modified_after: Option<std::time::SystemTime>,
    path: &str,
) -> bool {
    if let Some(m) = if_modified_after {
        if let Ok(last) = repo.modified_time(path) {
            debug!("modified_after: {:?} {:?}", m, last);
            return last.duration_since(m).is_ok();
        }
    }
    true
}

use std::borrow::Cow;

fn move_or_create<T: GraphTxnT + TreeMutTxnT, R: WorkingCopy, C: ChangeStore>(
    txn: ArcTxn<T>,
    repo: &R,
    output_item: &OutputItem,
    output_item_inode: Option<(Inode, Position<NodeId>)>,
    path: &str,
    tmp: &mut Option<String>,
    file_name: &str,
    actual_moves: &mut Vec<(String, String)>,
    move_map: &mut HashMap<Inode, String>,
    salt: u64,
) -> Result<Inode, OutputError<C::Error, T, R::Error>> {
    let file_id = OwnedPathId {
        parent_inode: output_item.parent,
        basename: SmallString::from_str(&file_name),
    };
    debug!("move_or_create {:?}", file_id);

    if let Some((inode, _)) = output_item_inode {
        // If the file already exists, find its
        // current name and rename it if that name
        // is different.
        let txn_ = txn.read();
        if let Some(ref current_name) = inode_filename(&*txn_, inode, move_map)? {
            let actual_path = if let Some(tmp) = tmp.take() {
                Cow::Owned(tmp)
            } else {
                Cow::Borrowed(path)
            };
            debug!(
                "current_name = {:?}, path = {:?}, actual_path = {:?}",
                current_name, path, actual_path
            );
            if current_name.as_str() != &actual_path {
                std::mem::drop(txn_);
                let mut txn_ = txn.write();
                let parent = txn_.get_revtree(&inode, None)?.unwrap().to_owned();
                debug!("parent = {:?}, inode = {:?}", parent, inode);
                del_tree_with_rev(&mut *txn_, &parent, &inode)?;

                let s = {
                    let mut c = [0u8; 16];
                    unsafe { *(c.as_mut_ptr() as *mut Position<NodeId>) = output_item.pos }
                    // Use SHA-512 instead of blake3
                    use sha2::{Digest, Sha512};
                    let mut hasher = Sha512::new();
                    hasher.update(&c);
                    let hash = hasher.finalize();
                    BASE32.encode(&hash[..32])
                };

                if let Some(ref mut tmp) = tmp {
                    // The parent was already moved
                    debug!("tmp {:?}", tmp);
                    crate::path::pop(tmp);
                    crate::path::push(tmp, &s);
                }

                let mut tmp_ = actual_path.to_string();
                crate::path::pop(&mut tmp_);
                crate::path::push(&mut tmp_, &s);

                debug!("rename {:?} {:?}", current_name, tmp_);
                repo.rename(&current_name, &tmp_)
                    .map_err(OutputError::WorkingCopy)?;

                move_map.insert(inode, tmp_.to_string());
                actual_moves.push((tmp_.to_string(), actual_path.to_string()));

                *tmp = Some(tmp_);

                // If the new location is overwriting an existing one,
                // actually overwrite.
                if let Some(&inode) = txn_.get_tree(&file_id, None)? {
                    crate::fs::rec_delete(&mut *txn_, &file_id, inode, true)
                        .map_err(PristineOutputError::Fs)?;
                }
                put_inodes_with_rev(&mut *txn_, &inode, &output_item.pos)?;
                put_tree_with_rev(&mut *txn_, &file_id, &inode)?;
                // The directory marker is necessarily already there,
                // since the path is in the tree.
                if output_item.meta.is_dir() {
                    let path_id = OwnedPathId {
                        parent_inode: inode,
                        basename: SmallString::new(),
                    };
                    assert_eq!(txn_.get_tree(&path_id, None).unwrap(), Some(&inode))
                }
            } else {
                if let Cow::Owned(tmp_) = actual_path {
                    *tmp = Some(tmp_)
                }
            }
        } else {
            debug!("no current name, inserting {:?} {:?}", file_id, inode);
            std::mem::drop(txn_);
            let mut txn_ = txn.write();
            if let Some(&inode) = txn_.get_tree(&file_id, None)? {
                crate::fs::rec_delete(&mut *txn_, &file_id, inode, true)
                    .map_err(PristineOutputError::Fs)?;
            }
            put_inodes_with_rev(&mut *txn_, &inode, &output_item.pos)?;
            put_tree_with_rev(&mut *txn_, &file_id, &inode)?;
            if output_item.meta.is_dir() {
                let path_id = OwnedPathId {
                    parent_inode: inode,
                    basename: SmallString::new(),
                };
                txn_.put_tree(&path_id, &inode)?;
            }
        }
        Ok(inode)
    } else {
        let mut txn_ = txn.write();
        if let Some(&inode) = txn_.get_tree(&file_id, None)? {
            crate::fs::rec_delete(&mut *txn_, &file_id, inode, true)
                .map_err(PristineOutputError::Fs)?;
        }
        let inode = create_new_inode(&mut *txn_, &file_id, salt)?;
        debug!(
            "created new inode {:?} {:?} {:?}",
            inode, output_item.pos, file_id
        );
        put_inodes_with_rev(&mut *txn_, &inode, &output_item.pos)?;
        put_tree_with_rev(&mut *txn_, &file_id, &inode)?;
        if output_item.meta.is_dir() {
            let path_id = OwnedPathId {
                parent_inode: inode,
                basename: SmallString::new(),
            };
            txn_.put_tree(&path_id, &inode)?;
        }
        Ok(inode)
    }
}

fn output_item<T: ChannelTxnT + TreeTxnT, P: ChangeStore, W: WorkingCopy>(
    txn: ArcTxn<T>,
    channel: ChannelRef<T>,
    changes: &P,
    output_item: &OutputItem,
    conflicts: &mut Vec<Conflict>,
    repo: &W,
    inode: Inode,
    path: &str,
    forward: &mut Vec<Redundant>,
) -> Result<(), OutputError<P::Error, T, W::Error>> {
    if !repo.is_writable(path).map_err(OutputError::WorkingCopy)? {
        return Ok(());
    }
    let mut l = {
        debug!("write");
        let txn = txn.write();
        debug!("/write");
        let channel = channel.read();
        retrieve(&*txn, txn.graph(&*channel), output_item.pos, false)?
    };
    let w = repo
        .write_file(&path, inode)
        .map_err(OutputError::WorkingCopy)?;
    debug!("vertex_buffer");
    let mut f = vertex_buffer::ConflictsWriter::new(w, &path, output_item.pos, conflicts);
    debug!("outputting graph");
    alive::output_graph(changes, &txn, &channel, &mut f, &mut l, forward)
        .map_err(PristineOutputError::from)?;
    use std::io::Write;
    f.w.flush().unwrap_or(());
    Ok(())
}

fn del_redundant<T: ChannelMutTxnT + GraphMutTxnT>(
    txn: ArcTxn<T>,
    channel: ChannelRef<T>,
    forward: &[Redundant],
) -> Result<(), TxnErr<T::GraphError>> {
    if forward.is_empty() {
        return Ok(());
    }
    let mut txn = txn.write();
    let mut channel = channel.write();
    for ve in forward.iter() {
        // Unwrap ok since `edge` is in the channel.
        let dest = *txn.find_block(txn.graph(&*channel), ve.e.dest()).unwrap();
        debug!("deleting forward edge {:?} {:?} {:?}", ve.v, dest, ve.e);
        del_graph_with_rev(
            &mut *txn,
            T::graph_mut(&mut *channel),
            ve.e.flag(),
            ve.v,
            dest,
            ve.e.introduced_by(),
        )?;
    }
    Ok(())
}

fn is_alive_or_zombie<T: GraphTxnT>(
    txn: &T,
    channel: &T::Graph,
    a: &Vertex<NodeId>,
) -> Result<bool, TxnErr<T::GraphError>> {
    if a.is_root() {
        return Ok(true);
    }
    for e in iter_adjacent(
        txn,
        channel,
        *a,
        EdgeFlags::PARENT,
        EdgeFlags::all() - EdgeFlags::DELETED,
    )? {
        let e = e?;
        let zf = EdgeFlags::pseudof();
        if (e.flag() & zf != EdgeFlags::PSEUDO)
            && (e.flag().contains(EdgeFlags::BLOCK) || a.is_empty())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn collect_dead_files<T: TreeTxnT + GraphTxnT, W: WorkingCopy + Clone, C: ChangeStore>(
    txn: &T,
    channel: &T::Graph,
    move_map: &HashMap<Inode, String>,
    pending_change_id: NodeId,
    inode: Inode,
) -> Result<HashMap<OwnedPathId, (Inode, Option<String>)>, OutputError<C::Error, T, W::Error>> {
    let mut inodes = vec![(inode, false)];
    let mut next_inodes = Vec::new();
    let mut dead = HashMap::default();
    while !inodes.is_empty() {
        for (inode, parent_is_dead) in inodes.drain(..) {
            for x in txn.iter_tree(
                &OwnedPathId {
                    parent_inode: inode,
                    basename: SmallString::new(),
                },
                None,
            )? {
                let (id, inode_) = x?;
                assert!(id.parent_inode >= inode);
                if id.parent_inode > inode {
                    break;
                }
                let is_dead = parent_is_dead
                    || (!id.basename.is_empty() && {
                        if let Some(vertex) = txn.get_inodes(&inode_, None)? {
                            vertex.change != pending_change_id
                                && !is_alive_or_zombie(txn, channel, &vertex.inode_vertex())?
                        } else {
                            true
                        }
                    });
                if is_dead {
                    dead.insert(
                        id.to_owned(),
                        (*inode_, inode_filename(txn, *inode_, move_map)?),
                    );
                }
                if *inode_ != inode {
                    next_inodes.push((*inode_, is_dead))
                }
            }
        }
        std::mem::swap(&mut inodes, &mut next_inodes)
    }
    Ok(dead)
}

fn kill_dead_files<T: ChannelTxnT + TreeMutTxnT, W: WorkingCopy + Clone, C: ChangeStore>(
    txn: &mut T,
    channel: &ChannelRef<T>,
    repo: &W,
    dead: &HashMap<OwnedPathId, (Inode, Option<String>)>,
) -> Result<(), OutputError<C::Error, T, W::Error>> {
    let channel = channel.read();
    // In order to avoid killing a directory before killing the files
    // inside, sort the longest paths first.
    let mut dead: Vec<_> = dead.iter().collect();
    dead.sort_by(|a, b| {
        let cmp = b.1 .1.cmp(&a.1 .1);
        use std::cmp::Ordering;
        if let Ordering::Equal = cmp {
            b.cmp(&a)
        } else {
            cmp
        }
    });
    for (fileid, (inode, ref name)) in dead.iter() {
        debug!("killing {:?} {:?} {:?}", fileid, inode, name);
        del_tree_with_rev(txn, &fileid, inode)?;
        // In case this is a directory, we also need to delete the marker:
        let file_id_ = OwnedPathId {
            parent_inode: *inode,
            basename: SmallString::new(),
        };
        txn.del_tree(&file_id_, Some(&inode))?;

        if let Some(&vertex) = txn.get_inodes(inode, None)? {
            debug!("kill_dead_files {:?} {:?}", inode, vertex);
            del_inodes_with_rev(txn, inode, &vertex)?;
            if txn
                .get_graph(txn.graph(&*channel), &vertex.inode_vertex(), None)
                .map_err(|x| OutputError::Pristine(x.into()))?
                .is_some()
            {
                if let Some(name) = name {
                    repo.remove_path(&name, false)
                        .map_err(OutputError::WorkingCopy)?
                }
            }
        }
    }
    Ok(())
}

fn inode_filename<T: TreeTxnT>(
    txn: &T,
    inode: Inode,
    tmp: &HashMap<Inode, String>,
) -> Result<Option<String>, TreeErr<T::TreeError>> {
    debug!("inode_filename {:?}", inode);
    let mut components = Vec::new();
    let mut current = inode;
    loop {
        if let Some(tmp) = tmp.get(&current) {
            components.push(SmallString::from_str(tmp));
            break;
        }
        match txn.get_revtree(&current, None)? {
            Some(v) => {
                components.push(v.basename.to_owned());
                current = v.parent_inode;
                if current == Inode::ROOT {
                    break;
                }
            }
            None => {
                debug!("filename_of_inode: not in tree");
                return Ok(None);
            }
        }
    }

    let mut path = String::new();
    for c in components.iter().rev() {
        if !path.is_empty() {
            path.push('/')
        }
        path.push_str(c.as_str());
    }
    debug!("inode_filename = {:?}", path);
    Ok(Some(path))
}
