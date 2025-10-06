use anyhow::bail;
use clap::{Parser, ValueHint};
use libatomic::attribution::{
    sanakirja_impl::AttributionStore as SanakirjaAttributionStore, AttributionContext,
    AttributionDetector, SuggestionType,
};
use libatomic::pristine::*;
use libatomic::*;
use log::{debug, error, info, trace, warn};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use atomic_repository::*;

#[derive(Parser, Debug)]
pub struct Git {
    /// Process this path instead of the current directory, creating a atomic repository if necessary.
    #[clap(value_hint = ValueHint::DirPath)]
    pub git_path: Option<PathBuf>,
    /// Process this path instead of the current directory, creating a atomic repository if necessary.
    #[clap(value_hint = ValueHint::DirPath)]
    pub atomic_path: Option<PathBuf>,
    /// Time the import, and output values to this file.
    #[clap(long = "stats", hide = true, value_hint = ValueHint::FilePath)]
    stats: Option<PathBuf>,
    /// Check only the first n commits processed.
    #[clap(default_value = "0", hide = true)]
    check: usize,
}

struct OpenRepo {
    repo: Repository,
    stats: Option<std::fs::File>,
    n: usize,
    check: usize,
    current_commit: Option<git2::Oid>,
}

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
struct Oid(git2::Oid);

use ::sanakirja::{Storable, UnsizedStorable};

::sanakirja::direct_repr!(Oid);

impl Git {
    pub fn run(self) -> Result<(), anyhow::Error> {
        let repo = if let Ok(repo) = Repository::find_root(self.atomic_path.clone()) {
            repo
        } else {
            Repository::init(self.atomic_path.clone(), None, None)?
        };
        let git_path = if let Some(git_path) = self.git_path.clone() {
            git_path
        } else {
            repo.path.clone()
        };
        let git = git2::Repository::open(&git_path)?;
        let st = git.statuses(None)?;
        let mut uncommitted = false;
        for i in 0..st.len() {
            if let Some(x) = st.get(i) {
                if x.path_bytes().starts_with(b".atomic") || x.path_bytes().starts_with(b".ignore")
                {
                    continue;
                }
                debug!("status = {:?}", x.status());
                if x.status() != git2::Status::CURRENT && x.status() != git2::Status::IGNORED {
                    eprintln!("Uncommitted file: {:?}", x.path().unwrap());
                    uncommitted = true;
                }
            }
        }
        if uncommitted {
            bail!("There were uncommitted files")
        }
        let head = git.head()?;
        info!("Loading Git history…");
        let oid = head.target().unwrap();
        let mut path_git = repo.path.join(libatomic::DOT_DIR);
        path_git.push("git");
        std::fs::create_dir_all(&path_git)?;
        let mut env_git = ::sanakirja::Env::new(&path_git.join("db"), 1 << 15, 2)?;
        let dag = Dag::dfs(&git, oid, &mut env_git)?;

        trace!(target: "dag", "{:?}", dag);
        debug!("Done");
        let mut pristine = repo.path.join(DOT_DIR);
        pristine.push(PRISTINE_DIR);
        std::fs::create_dir_all(&pristine)?;
        let mut repo = OpenRepo {
            repo,
            stats: self.stats.and_then(|f| std::fs::File::create(f).ok()),
            n: 0,
            check: self.check,
            current_commit: None,
        };
        import(&git, &mut env_git, &mut repo, &dag)?;

        let txn = repo.repo.pristine.arc_txn_begin()?;
        if let Some(oid) = repo.current_commit {
            let channel = txn.read().load_channel(&format!("{}", oid))?;
            if let Some(channel) = channel {
                libatomic::output::output_repository_no_pending(
                    &libatomic::working_copy::FileSystem::from_root(&repo.repo.path),
                    &repo.repo.changes,
                    &txn,
                    &channel,
                    "",
                    false,
                    None,
                    std::thread::available_parallelism()?.get(),
                    0,
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Dag {
    children: BTreeMap<git2::Oid, Vec<git2::Oid>>,
    parents: BTreeMap<git2::Oid, Vec<git2::Oid>>,
    root: Vec<(git2::Oid, Option<libatomic::Merkle>)>,
}

impl Dag {
    /// Load a Git repository in memory. The main reason this is
    /// needed is to compute the *backward* relations from a commit to
    /// its parents.
    fn dfs(
        git: &git2::Repository,
        oid: git2::Oid,
        env_git: &mut ::sanakirja::Env,
    ) -> Result<Self, anyhow::Error> {
        let mut stack = vec![git.find_commit(oid)?];
        let mut oids_set = BTreeSet::new();
        let mut dag = Dag {
            children: BTreeMap::new(),
            parents: BTreeMap::new(),
            root: Vec::new(),
        };
        oids_set.insert(oid.clone());
        let mut txn_git = ::sanakirja::Env::mut_txn_begin(env_git)?;
        let db: ::sanakirja::btree::UDb<Oid, libatomic::pristine::SerializedMerkle> = unsafe {
            if let Some(db) = txn_git.root(0) {
                ::sanakirja::btree::UDb::from_page(db)
            } else {
                ::sanakirja::btree::create_db_(&mut txn_git)?
            }
        };
        let mut state = BTreeMap::new();
        for x in ::sanakirja::btree::iter(&txn_git, &db, None)? {
            let (commit, merk) = x?;
            state.insert(commit, merk.clone());
        }
        debug!("state = {:?}", state);
        while let Some(commit) = stack.pop() {
            if let Some(state) = state.get(&Oid(commit.id())) {
                dag.root.push((commit.id(), Some(state.into())));
                continue;
            }
            let mut has_parents = false;
            for p in commit.parents() {
                trace!("parent {:?}", p);
                dag.children
                    .entry(p.id())
                    .or_insert(Vec::new())
                    .push(commit.id());
                dag.parents
                    .entry(commit.id())
                    .or_insert(Vec::new())
                    .push(p.id());
                if oids_set.insert(p.id()) {
                    stack.push(p);
                }
                has_parents = true
            }
            if !has_parents {
                dag.root.push((commit.id(), None))
            }
        }
        txn_git.set_root(0, db.db.into());
        ::sanakirja::Commit::commit(txn_git)?;
        Ok(dag)
    }

    fn collect_dead_parents<T: MutTxnTExt>(
        &self,
        oid: &git2::Oid,
        todo: &mut Todo,
        txn: &ArcTxn<T>,
    ) -> Result<(), anyhow::Error> {
        if let Some(parents) = self.parents.get(oid) {
            debug!("parents {:?}", parents);
            for p in parents {
                let rc = todo.refs.get_mut(p).unwrap();
                *rc -= 1;
                if *rc == 0 {
                    let p_name = format!("{}", p);
                    debug!("dropping channel {:?}", p_name);
                    let mut txn = txn.write();
                    txn.drop_channel(&p_name)?;
                }
            }
        }
        Ok(())
    }

    fn insert_children_in_todo(&self, oid: &git2::Oid, todo: &mut Todo) {
        if let Some(c) = self.children.get(&oid) {
            for child in c {
                debug!("child = {:?}", c);
                if todo.next_todo_set.insert(*child) {
                    todo.next_todo.push(*child);
                }
                *todo.refs.entry(*oid).or_insert(0) += 1;
            }
        } else {
            debug!("no children")
        }
    }
}

#[derive(Debug)]
struct Todo {
    todo: Vec<git2::Oid>,
    todo_set: BTreeSet<git2::Oid>,
    next_todo: Vec<git2::Oid>,
    next_todo_set: BTreeSet<git2::Oid>,
    // For each key k, number of items in the union of todo and
    // next_todo that have k as a parent. Moreover, all commits that
    // were imported are in this map.
    refs: BTreeMap<git2::Oid, usize>,
}

impl Todo {
    fn new() -> Self {
        Todo {
            todo: Vec::new(),
            todo_set: BTreeSet::new(),
            next_todo: Vec::new(),
            next_todo_set: BTreeSet::new(),
            refs: BTreeMap::new(),
        }
    }

    fn swap_next(&mut self, todo: Vec<git2::Oid>) {
        self.todo = todo;
        std::mem::swap(&mut self.todo, &mut self.next_todo);
        self.todo_set.clear();
        std::mem::swap(&mut self.todo_set, &mut self.next_todo_set);
    }

    fn insert_next(&mut self, oid: git2::Oid) {
        if self.next_todo_set.insert(oid) {
            self.next_todo.push(oid)
        }
    }

    fn is_empty(&self) -> bool {
        self.todo.is_empty()
    }

    fn all_processed(&self, parents: &[git2::Oid]) -> bool {
        parents.iter().all(|x| self.refs.contains_key(x))
    }
}

/// Import the entire Git DAG into atomic.
fn import(
    git: &git2::Repository,
    env_git: &mut ::sanakirja::Env,
    repo: &mut OpenRepo,
    dag: &Dag,
) -> Result<(), anyhow::Error> {
    let mut ws = libatomic::ApplyWorkspace::new();
    let mut todo = Todo::new();

    let txn = repo.repo.pristine.arc_txn_begin()?;
    for &(oid, merkle) in dag.root.iter() {
        if let Some(merkle) = merkle {
            let oid_ = format!("{}", oid);
            let channel = if let Some(c) = txn.read().load_channel(&oid_)? {
                c
            } else {
                bail!("Channel not found: {:?}", oid);
            };
            let (_, &p) = txn
                .read()
                .changeid_reverse_log(&*channel.read(), None)?
                .next()
                .unwrap()?;
            let merkle_: libatomic::Merkle = (&p.b).into();
            if merkle != merkle_ {
                bail!("atomic channel changed since last import. Please unrecord channel {} to state {}", oid_, merkle.to_base32())
            }
            if let Some(children) = dag.children.get(&oid) {
                *todo.refs.entry(oid).or_insert(0) += children.len();
                for c in children.iter() {
                    todo.insert_next(*c);
                }
            }
        } else {
            todo.insert_next(oid);
            if let Some(parents) = dag.parents.get(&oid) {
                for p in parents.iter() {
                    *todo.refs.entry(*p).or_insert(0) += 1;
                }
            }
        }
    }
    std::mem::drop(txn);
    todo.swap_next(Vec::new());

    while !todo.is_empty() {
        debug!("TODO: {:?}", todo);
        let mut todo_ = std::mem::replace(&mut todo.todo, Vec::new());
        {
            let mut draining = todo_.drain(..);
            let txn = repo.repo.pristine.arc_txn_begin()?;
            while let Some(oid) = draining.next() {
                let channel = if let Some(parents) = dag.parents.get(&oid) {
                    // If we don't have all the parents, continue.
                    if !todo.all_processed(&parents) {
                        todo.insert_next(oid);
                        continue;
                    }
                    let first_parent = parents.iter().next().unwrap();
                    let parent_name = format!("{}", first_parent);
                    let mut txn = txn.write();
                    let parent_channel = txn.load_channel(&parent_name)?.unwrap();

                    let name = format!("{}", oid);
                    let channel = txn.fork(&parent_channel, &name)?;

                    channel
                } else {
                    // Create a new channel for this commit.
                    let name = format!("{}", oid);
                    let mut txn = txn.write();
                    let channel = txn.open_or_create_channel(&name)?;
                    channel
                };

                let mut stats = Stats::new(oid);
                import_commit_parents(repo, dag, &txn, &channel, &oid, &mut ws, &mut stats)?;
                let state = import_commit(git, repo, &txn, &channel, &oid, &mut stats)?;
                save_state(env_git, &oid, state)?;
                dag.collect_dead_parents(&oid, &mut todo, &txn)?;
                dag.insert_children_in_todo(&oid, &mut todo);

                if let Some(ref mut f) = repo.stats {
                    stats.write(repo.n, &repo.repo.path, f)?
                }
                // Just add the remaining commits to the todo list,
                // because we prefer to move each channel as far as
                // possible before switching channels.
                while let Some(oid) = draining.next() {
                    todo.insert_next(oid)
                }
            }
            txn.commit()?;
        }
        todo.swap_next(todo_)
    }
    Ok(())
}

fn save_state(
    git: &mut ::sanakirja::Env,
    oid: &git2::Oid,
    state: libatomic::Merkle,
) -> Result<(), anyhow::Error> {
    use ::sanakirja::Commit;
    let mut txn = ::sanakirja::Env::mut_txn_begin(git)?;
    let mut db: ::sanakirja::btree::UDb<Oid, libatomic::pristine::SerializedMerkle> = unsafe {
        if let Some(db) = txn.root(0) {
            ::sanakirja::btree::UDb::from_page(db)
        } else {
            ::sanakirja::btree::create_db_(&mut txn)?
        }
    };
    ::sanakirja::btree::put(&mut txn, &mut db, &Oid(*oid), &state.into())?;
    txn.set_root(0, db.db.into());
    txn.commit()?;
    Ok(())
}

fn make_apply_plan<T: TxnTExt>(
    repo: &OpenRepo,
    txn: &ArcTxn<T>,
    channel: &ChannelRef<T>,
    dag: &Dag,
    oid: &git2::Oid,
) -> Result<(bool, Vec<(libatomic::Hash, u64)>), anyhow::Error> {
    let mut to_apply = Vec::new();
    let mut to_apply_set = BTreeSet::new();
    let mut needs_output = false;
    if let Some(parents) = dag.parents.get(&oid) {
        let txn = txn.read();
        for p in parents {
            // If one of the parents is not the repo's current commit,
            // then we're doing either a merge or a checkout of
            // another branch. If that is the case, we need to output
            // the entire repository to update the
            // tree/revtree/inodes/revinodes tables.
            if let Some(current_commit) = repo.current_commit {
                if current_commit != *p {
                    needs_output = true
                }
            }
            let p_name = format!("{}", p);
            let p_channel = txn.load_channel(&p_name)?.unwrap();
            for x in txn.log(&*p_channel.read(), 0)? {
                let (n, (h, _)) = x?;
                let h: libatomic::Hash = h.into();
                if txn.has_change(&channel, &h)?.is_none() {
                    if to_apply_set.insert(h) {
                        to_apply.push((h, n));
                    }
                }
            }
        }
    } else {
        needs_output = true
    }

    // Since we're pulling from multiple channels, the change numbers
    // are not necessarily in order (especially since we've
    // de-duplicated using `to_apply_set`.

    to_apply.sort_by(|a, b| a.1.cmp(&b.1));
    Ok((needs_output, to_apply))
}

/// Apply the changes corresponding to a commit's parents to `channel`.
fn import_commit_parents<T: TxnTExt + MutTxnTExt + GraphIter + Send + Sync + 'static>(
    repo: &mut OpenRepo,
    dag: &Dag,
    txn: &ArcTxn<T>,
    channel: &ChannelRef<T>,
    oid: &git2::Oid,
    ws: &mut libatomic::ApplyWorkspace,
    stats: &mut Stats,
) -> Result<(), anyhow::Error> {
    // Apply all the parent's logs to `channel`
    let (needs_output, to_apply) = make_apply_plan(repo, &txn, &channel, dag, oid)?;
    let parent_application_time = std::time::Instant::now();
    for h in to_apply.iter() {
        debug!("to_apply {:?}", h)
    }
    let mut txn_ = txn.write();
    for (h, _) in to_apply.iter() {
        let mut channel_ = channel.write();
        info!("applying {:?} to {:?}", h, txn_.name(&channel_));

        txn_.apply_node_ws(
            &repo.repo.changes,
            &mut channel_,
            h,
            libatomic::pristine::NodeType::Change,
            ws,
        )?;
        if repo.check > 0 && repo.n % repo.check == 0 {
            check_alive_debug(&repo.repo.changes, &*txn_, &channel_, line!())?;
        }
    }
    if repo.check > 0 && repo.n % repo.check == 0 {
        check_alive_debug(&repo.repo.changes, &*txn_, &*channel.read(), line!())?;
    }
    stats.parent_application_time = if to_apply.is_empty() {
        std::time::Duration::from_secs(0)
    } else {
        parent_application_time.elapsed()
    };
    std::mem::drop(txn_);
    stats.output_time = if !to_apply.is_empty() || needs_output {
        debug!("outputting");
        let output_time = std::time::Instant::now();
        libatomic::output::output_repository_no_pending(
            &libatomic::working_copy::sink(),
            &repo.repo.changes,
            &txn,
            &channel,
            "",
            false,
            None,
            std::thread::available_parallelism()?.get(),
            0,
        )?;
        let t = output_time.elapsed();
        if repo.check > 0 && repo.n % repo.check == 0 {
            let txn = txn.read();
            check_alive_debug(&repo.repo.changes, &*txn, &channel.read(), line!())?;
        }
        t
    } else {
        std::time::Duration::from_secs(0)
    };
    if repo.check > 0 && repo.n % repo.check == 0 {
        let txn = txn.read();
        check_tree_inodes(&*txn, txn.graph(&*channel.read()));
    }
    Ok(())
}

/// Reset to the Git commit specified by `child`, telling atomic which
/// files were moved in the reset.
fn git_reset<'a, T: TxnTExt + MutTxnTExt>(
    git: &'a git2::Repository,
    repo: &mut OpenRepo,

    txn: &ArcTxn<T>,
    channel: &ChannelRef<T>,

    child: &git2::Oid,

    stats: &mut Stats,
) -> Result<(git2::Object<'a>, BTreeMap<PathBuf, bool>, HashSet<String>), anyhow::Error> {
    // Reset the Git branch.

    debug!("resetting the git branch to {:?}", child);
    let reset_time = std::time::Instant::now();
    let object = git.find_object(*child, None)?;
    repo.current_commit = Some(*child);
    stats.reset_time = reset_time.elapsed();
    debug!("reset done");

    let mut prefixes = BTreeMap::new();
    let mut pref = HashSet::new();
    {
        let commit = object.as_commit().unwrap();
        let new_tree = commit.tree().unwrap();

        debug!("inspecting commit");
        let git_diff_time = std::time::Instant::now();
        let mut has_parents = false;
        for parent in commit.parents() {
            has_parents = true;
            let old_tree = parent.tree().unwrap();
            let mut diff = git
                .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)
                .unwrap();
            diff.find_similar(None).unwrap();
            let mut moves = Vec::new();
            let mut txn = txn.write();
            for delta in diff.deltas() {
                let old_path = delta.old_file().path().unwrap();
                let new_path = delta.new_file().path().unwrap();
                let is_dir = delta.new_file().mode() == git2::FileMode::Tree;
                match delta.status() {
                    git2::Delta::Renamed => {
                        info!(
                            "mv {:?} {:?}",
                            old_path.to_string_lossy(),
                            new_path.to_string_lossy()
                        );
                        if let Ok((vertex, _)) = txn.follow_oldest_path(
                            &repo.repo.changes,
                            &channel,
                            &old_path.to_string_lossy(),
                        ) {
                            if let Some(inode) = txn.get_revinodes(&vertex, None)? {
                                if let Some(old_path) =
                                    libatomic::fs::inode_filename(&*txn, *inode)?
                                {
                                    debug!(
                                        "moving {:?} ({:?}) from {:?} to {:?}",
                                        inode, vertex, old_path, new_path
                                    );
                                    let mut tmp_path = new_path.to_path_buf();
                                    tmp_path.pop();
                                    use rand::Rng;
                                    let s: String = rand::thread_rng()
                                        .sample_iter(&rand::distributions::Alphanumeric)
                                        .take(30)
                                        .map(|x| x as char)
                                        .collect();
                                    tmp_path.push(&s);
                                    if let Err(e) =
                                        txn.move_file(&old_path, &tmp_path.to_string_lossy(), 0)
                                    {
                                        error!("{}", e);
                                    } else {
                                        moves.push((tmp_path, new_path));
                                    }
                                }
                            }
                        }
                        let new_path_ = new_path.to_path_buf();
                        pref.insert(new_path.to_str().unwrap().to_string());
                        prefixes.insert(new_path_, is_dir);
                    }
                    git2::Delta::Deleted => {
                        let old_path = old_path.to_path_buf();
                        prefixes.insert(old_path, is_dir);
                    }
                    _ => {
                        if delta.new_file().mode() != git2::FileMode::Link {
                            debug!("delta old = {:?} new = {:?}", old_path, new_path);
                            let old_path_ = old_path.to_path_buf();
                            let new_path_ = new_path.to_path_buf();
                            prefixes.insert(old_path_, is_dir);
                            prefixes.insert(new_path_, is_dir);
                            pref.insert(old_path.to_str().unwrap().to_string());
                            pref.insert(new_path.to_str().unwrap().to_string());
                        }
                    }
                }
            }
            debug!("moves = {:?}", moves);
            for (a, b) in moves.drain(..) {
                if let Err(e) = txn.move_file(&a.to_string_lossy(), &b.to_string_lossy(), 0) {
                    error!("{}", e);
                }
            }
        }
        if !has_parents {
            use git2::{TreeWalkMode, TreeWalkResult};
            new_tree
                .walk(TreeWalkMode::PreOrder, |x, t| {
                    debug!("t = {:?} {:?}", x, t.name());
                    if let Some(n) = t.name() {
                        let mut m = Path::new(x).to_path_buf();
                        m.push(n);
                        prefixes.insert(m, t.kind() == Some(git2::ObjectType::Tree));
                    }
                    TreeWalkResult::Ok
                })
                .unwrap();
        }
        stats.git_diff_time = git_diff_time.elapsed();
        debug!("record prefixes {:?}", prefixes);
    }
    Ok((object, prefixes, pref))
}

#[derive(Clone)]
struct Commit<'a> {
    r: &'a git2::Repository,
    c: git2::Commit<'a>,
    pref: HashSet<String>,
}

impl<'a> libatomic::working_copy::WorkingCopyRead for Commit<'a> {
    type Error = git2::Error;

    fn file_metadata(&self, file: &str) -> Result<InodeMetadata, Self::Error> {
        debug!("metadata {:?}", file);
        let entry = self.c.tree()?.get_path(Path::new(file))?;
        let is_dir = entry.kind() == Some(git2::ObjectType::Tree);
        if is_dir {
            Ok(InodeMetadata::new(0o100, true))
        } else {
            let permissions = entry.filemode();
            debug!(
                "permissions = {:o} {:o} {:?}",
                permissions,
                permissions & 0o100,
                is_dir
            );
            Ok(InodeMetadata::new(permissions as usize & 0o100, false))
        }
    }

    fn read_file(&self, file: &str, buffer: &mut Vec<u8>) -> Result<(), Self::Error> {
        debug!("read file {:?}", file);
        let entry = self.c.tree()?.get_path(Path::new(file))?;
        if let Ok(b) = entry.to_object(self.r)?.peel_to_blob() {
            buffer.extend(b.content());
        }
        debug!("entry {:?}", entry.kind());
        Ok(())
    }

    fn modified_time(&self, x: &str) -> Result<std::time::SystemTime, Self::Error> {
        if self.pref.contains(x) {
            Ok(std::time::SystemTime::now())
        } else {
            Ok(std::time::SystemTime::UNIX_EPOCH)
        }
    }
}

/// Reset to the Git commit specified as `child`, and record the
/// corresponding change in atomic.
fn import_commit<T: TxnTExt + MutTxnTExt + GraphIter + Send + Sync + 'static>(
    git: &git2::Repository,
    repo: &mut OpenRepo,
    txn: &ArcTxn<T>,
    channel: &ChannelRef<T>,
    child: &git2::Oid,
    stats: &mut Stats,
) -> Result<libatomic::Merkle, anyhow::Error> {
    let (object, prefixes, prefstr) = git_reset(git, repo, &txn, &channel, child, stats)?;
    debug!("prefixes = {:?}", prefixes);
    let mut txn_ = txn.write();
    let mut prefixes_ = BTreeMap::new();
    for (mut p, is_dir) in prefixes {
        use path_slash::PathExt;
        loop {
            debug!("p = {:?}", p);
            if prefixes_.contains_key(&p) {
                break;
            }
            let p_ = p.to_slash_lossy();
            debug!("adding prefix {:?}", p_);
            let (tracked, pos) = libatomic::fs::get_vertex(&*txn_, &p_)?;
            if !tracked {
                debug!("not tracked");
                if is_dir {
                    txn_.add_dir(&p_, 0).map(|_| ()).unwrap_or(());
                } else {
                    txn_.add_file(&p_, 0).map(|_| ()).unwrap_or(());
                }
            }
            debug!("pos = {:?}", pos);
            if pos.is_none() || !is_dir {
                if !p.pop() {
                    prefixes_.insert(PathBuf::new(), true);
                    break;
                }
            } else {
                prefixes_.insert(p, is_dir);
                break;
            }
        }
    }
    let commit = object.as_commit().unwrap();
    let signature = commit.author();
    // Record+Apply
    debug!("recording on channel {:?}", txn_.name(&channel.read()));

    if let Some(msg) = commit.message() {
        info!("Importing commit {:?}: {}", child, msg);
    } else {
        info!("Importing commit {:?} (no message)", child);
    }
    std::mem::drop(txn_);
    let msg = commit.message().unwrap();
    let mut msg_lines = msg.lines();
    let mut message = String::new();
    if let Some(m) = msg_lines.next() {
        message.push_str(m)
    }
    let mut description = String::new();
    for m in msg_lines {
        if !description.is_empty() {
            description.push('\n')
        }
        description.push_str(m);
    }
    let mut author = BTreeMap::new();
    author.insert("name".to_string(), signature.name().unwrap().to_string());
    author.insert("email".to_string(), signature.email().unwrap().to_string());

    // Create header before record_apply to use for attribution detection
    let header = libatomic::change::ChangeHeader {
        message: message.clone(),
        authors: vec![libatomic::change::Author(author.clone())],
        description: if description.is_empty() {
            None
        } else {
            Some(description.clone())
        },
        timestamp: chrono::DateTime::from_naive_utc_and_offset(
            chrono::NaiveDateTime::from_timestamp_opt(signature.when().seconds(), 0)
                .expect("seconds and nanos are within bounds"),
            chrono::Utc,
        ),
    };
    let rec = record_apply(
        &txn,
        &channel,
        // &repo.repo.working_copy
        &Commit {
            r: git,
            c: git.find_commit(*child)?,
            pref: prefstr,
        },
        &repo.repo.changes,
        &prefixes_,
        header.clone(),
        stats,
    );

    // Check if this might be an AI-assisted commit and store attribution
    if let Ok((_, Some(hash), _)) = rec {
        // Detect AI assistance from git commit message and description
        let attribution_detector = AttributionDetector::new(Default::default());
        let combined_text = format!(
            "{} {}",
            header.message,
            header.description.as_deref().unwrap_or("")
        )
        .to_lowercase();

        // Look for AI indicators in git commit messages
        let ai_indicators = [
            "ai-assisted",
            "ai-generated",
            "copilot",
            "claude",
            "gpt",
            "chatgpt",
            "ai:",
            "assistant:",
            "auto-generated",
            "github copilot",
            "codewhisperer",
            "tabnine",
            "kite",
            "intellicode",
            "machine learning",
            "automated",
        ];

        let is_ai_assisted = ai_indicators
            .iter()
            .any(|indicator| combined_text.contains(indicator));

        if is_ai_assisted {
            // Create attribution store for database persistence
            let attribution_store = SanakirjaAttributionStore::new(repo.repo.pristine.clone());

            // Initialize attribution tables if they don't exist
            if let Err(e) = attribution_store.initialize_tables() {
                warn!(
                    "Failed to initialize attribution tables during git import: {}",
                    e
                );
            } else {
                // Create attribution context for AI-assisted git commit
                let attribution_context = libatomic::attribution::AttributionContext {
                    is_ai_assisted: true,
                    ai_provider: Some("auto-detected".to_string()),
                    ai_model: Some("unknown".to_string()),
                    ai_suggestion_type: Some(SuggestionType::Complete),
                    ai_confidence: Some(0.6), // Lower confidence for auto-detection
                    ai_token_count: None,
                    ai_review_time: None,
                    model_temperature: None,
                    model_max_tokens: None,
                    model_top_p: None,
                    model_frequency_penalty: None,
                    model_presence_penalty: None,
                };

                let change_id = libatomic::attribution::PatchId::from(
                    libatomic::NodeId::from_base32(hash.to_base32().as_bytes())
                        .unwrap_or(libatomic::NodeId::ROOT),
                );

                let attributed_patch = attribution_detector.create_attributed_patch(
                    &attribution_context,
                    change_id,
                    &header.message,
                );

                // Store the attributed patch in the database
                match attribution_store.put_attribution(&attributed_patch) {
                    Ok(()) => {
                        debug!(
                            "Successfully stored AI attribution for git import: hash={}, message='{}'",
                            hash.to_base32(),
                            &header.message
                        );
                    }
                    Err(e) => {
                        warn!("Failed to store attribution during git import: {}", e);
                    }
                }
            }
        }
    }
    {
        let mut txn = txn.write();
        let name = txn.name(&channel.read()).to_string();
        txn.set_current_channel(&name)?;
    }
    let txn = txn.read();
    let (n_actions, hash, state) = match rec {
        Ok(x) => x,
        Err(libatomic::LocalApplyError::ChangeAlreadyOnChannel { hash }) => {
            error!("change already on channel: {:?}", hash);
            return Ok(txn.current_state(&channel.read())?);
        }
        Err(e) => return Err(e.into()),
    };

    if repo.check > 0 && repo.n % repo.check == 0 {
        check_alive_debug(&repo.repo.changes, &*txn, &channel.read(), line!())?;
    }

    stats.n_actions = n_actions;
    stats.hash = hash;

    if repo.check > 0 && repo.n % repo.check == 0 {
        check_tree_inodes(&*txn, txn.graph(&channel.read()));
    }
    repo.n += 1;
    Ok(state)
}

fn record_apply<
    T: TxnT + TxnTExt + MutTxnTExt,
    C: libatomic::changestore::ChangeStore + Clone,
    W: libatomic::working_copy::WorkingCopyRead + Clone,
>(
    txn: &ArcTxn<T>,
    channel: &ChannelRef<T>,
    working_copy: &W,
    changes: &C,
    prefixes: &BTreeMap<PathBuf, bool>,
    header: libatomic::change::ChangeHeader,
    stats: &mut Stats,
) -> Result<(usize, Option<libatomic::Hash>, libatomic::Merkle), libatomic::LocalApplyError<T>>
where
    W::Error: 'static,
{
    debug!("record_apply {:?}", prefixes);
    let record_time = std::time::Instant::now();
    let mut state = libatomic::RecordBuilder::new();
    let mut last = None;
    for (p, _) in prefixes.iter() {
        if let Some(last) = last {
            if p.starts_with(&last) {
                continue;
            }
        }
        state
            .record_single_thread(
                txn.clone(),
                libatomic::Algorithm::default(),
                false,
                &libatomic::DEFAULT_SEPARATOR,
                channel.clone(),
                working_copy,
                changes,
                p.to_str().unwrap(),
            )
            .unwrap();
        last = Some(p);
    }
    if prefixes.is_empty() {
        state
            .record_single_thread(
                txn.clone(),
                libatomic::Algorithm::default(),
                false,
                &libatomic::DEFAULT_SEPARATOR,
                channel.clone(),
                working_copy,
                changes,
                "",
            )
            .unwrap();
    }
    let rec = state.finish();
    let mut txn = txn.write();
    if rec.actions.is_empty() {
        return Ok((0, None, txn.current_state(&channel.read()).map_err(TxnErr)?));
    }
    let actions: Vec<_> = rec
        .actions
        .into_iter()
        .map(|rec| rec.globalize(&*txn).unwrap())
        .collect();
    let n = actions.len();
    let (dependencies, extra_known) =
        libatomic::change::dependencies(&*txn, &channel.read(), actions.iter())?;
    let mut change = libatomic::change::LocalChange::make_change(
        &*txn,
        &channel,
        actions,
        std::mem::replace(&mut *rec.contents.lock(), Vec::new()),
        header,
        Vec::new(),
    )?;
    change.dependencies = dependencies;
    change.extra_known = extra_known;
    debug!("saving change");
    let hash = changes
        .save_change(&mut change, |_, _| Ok::<_, anyhow::Error>(()))
        .unwrap();
    stats.record_time = record_time.elapsed();
    debug!("saved");
    let apply_time = std::time::Instant::now();
    let (_, m) = txn.apply_local_change(&channel, &change, &hash, &rec.updatables)?;
    stats.apply_time = apply_time.elapsed();
    Ok((n, Some(hash), m))
}

struct Stats {
    child: git2::Oid,
    n_changes: usize,
    parent_application_time: std::time::Duration,
    output_time: std::time::Duration,
    reset_time: std::time::Duration,
    git_diff_time: std::time::Duration,
    record_time: std::time::Duration,
    apply_time: std::time::Duration,
    n_actions: usize,
    n_files: usize,
    n_dirs: usize,
    total_size: u64,
    changes_size: u64,
    pristine_size: u64,
    hash: Option<libatomic::Hash>,
}

impl Stats {
    fn new(child: git2::Oid) -> Self {
        let z = std::time::Duration::from_secs(0);
        Stats {
            child,
            n_changes: 0,
            parent_application_time: z,
            output_time: z,
            reset_time: z,
            git_diff_time: z,
            record_time: z,
            apply_time: z,
            n_actions: 0,
            n_files: 0,
            n_dirs: 0,
            total_size: 0,
            changes_size: 0,
            pristine_size: 0,
            hash: None,
        }
    }

    fn write(
        &mut self,
        n: usize,
        repo_path: &Path,
        f: &mut std::fs::File,
    ) -> Result<(), anyhow::Error> {
        // Count files.
        let mut walk = ignore::WalkBuilder::new(&repo_path);
        walk.add_ignore(DOT_DIR).unwrap();
        for f in walk.build() {
            let meta = f?.metadata()?;
            if meta.is_dir() {
                self.n_dirs += 1
            } else {
                self.n_files += 1;
                self.total_size += meta.len();
            }
        }

        let dot_dir = repo_path.join(DOT_DIR);
        let pristine_dir = dot_dir.join(PRISTINE_DIR);
        let changes_dir = dot_dir.join(CHANGES_DIR);
        if let Ok(walk) = std::fs::read_dir(&pristine_dir) {
            for f in walk {
                let meta = f?.metadata()?;
                self.pristine_size += meta.len();
            }
        }
        if let Ok(walk) = std::fs::read_dir(&changes_dir) {
            for f in walk {
                let meta = f?.metadata()?;
                self.changes_size += meta.len();
                self.n_changes += 1
            }
        }
        let timers = libatomic::get_timers();
        writeln!(
            f, "{}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}",
            self.child,
            n,
            self.parent_application_time.as_secs_f64(),
            timers.alive_output.as_secs_f64(),
            timers.alive_retrieve.as_secs_f64(),
            timers.alive_graph.as_secs_f64(),
            timers.alive_contents.as_secs_f64(),
            timers.alive_write.as_secs_f64(),
            timers.apply.as_secs_f64(),
            timers.record.as_secs_f64(),
            timers.repair_context.as_secs_f64(),
            timers.check_cyclic_paths.as_secs_f64(),
            timers.find_alive.as_secs_f64(),
            self.output_time.as_secs_f64(),
            self.reset_time.as_secs_f64(),
            self.git_diff_time.as_secs_f64(),
            self.record_time.as_secs_f64(),
            self.apply_time.as_secs_f64(),
            self.n_actions,
            self.n_files,
            self.n_dirs,
            self.total_size,
            self.changes_size,
            self.pristine_size,
            if let Some(ref h) = self.hash { h.to_base32() } else { String::new() },
        )?;
        libatomic::reset_timers();
        Ok(())
    }
}
