use super::*;
use crate::HashMap;
use ::sanakirja::*;
use parking_lot::Mutex;
use std::collections::hash_map::Entry;
#[cfg(feature = "mmap")]
use std::path::Path;
use std::sync::Arc;

/// A Sanakirja pristine.
#[derive(Clone)]
pub struct Pristine {
    pub env: Arc<::sanakirja::Env>,
}

pub(crate) type P<K, V> = btree::page::Page<K, V>;
pub type Db<K, V> = btree::Db<K, V>;
pub(crate) type UP<K, V> = btree::page_unsized::Page<K, V>;
pub type UDb<K, V> = btree::Db_<K, V, UP<K, V>>;

#[derive(Debug, Error)]
pub enum SanakirjaError {
    #[error(transparent)]
    Sanakirja(#[from] ::sanakirja::Error),
    #[error("Pristine locked")]
    PristineLocked,
    #[error("Pristine corrupt")]
    PristineCorrupt,
    #[error(transparent)]
    Borrow(#[from] std::cell::BorrowError),
    #[error("Cannot dropped a borrowed channel: {:?}", c)]
    ChannelRc { c: String },
    #[error("Pristine version mismatch. Cloning over the network can fix this.")]
    Version,
}

impl std::convert::From<::sanakirja::CRCError> for SanakirjaError {
    fn from(_: ::sanakirja::CRCError) -> Self {
        SanakirjaError::PristineCorrupt
    }
}

impl std::convert::From<::sanakirja::CRCError> for TxnErr<SanakirjaError> {
    fn from(_: ::sanakirja::CRCError) -> Self {
        TxnErr(SanakirjaError::PristineCorrupt)
    }
}

impl std::convert::From<::sanakirja::Error> for TxnErr<SanakirjaError> {
    fn from(e: ::sanakirja::Error) -> Self {
        TxnErr(e.into())
    }
}

impl std::convert::From<::sanakirja::Error> for TreeErr<SanakirjaError> {
    fn from(e: ::sanakirja::Error) -> Self {
        TreeErr(e.into())
    }
}

impl std::convert::From<TxnErr<::sanakirja::Error>> for TxnErr<SanakirjaError> {
    fn from(e: TxnErr<::sanakirja::Error>) -> Self {
        TxnErr(e.0.into())
    }
}

impl std::convert::From<TreeErr<::sanakirja::Error>> for TreeErr<SanakirjaError> {
    fn from(e: TreeErr<::sanakirja::Error>) -> Self {
        TreeErr(e.0.into())
    }
}

impl Pristine {
    #[cfg(feature = "mmap")]
    pub fn new<P: AsRef<Path>>(name: P) -> Result<Self, SanakirjaError> {
        Self::new_with_size(name, 1 << 20)
    }
    #[cfg(feature = "mmap")]
    pub unsafe fn new_nolock<P: AsRef<Path>>(name: P) -> Result<Self, SanakirjaError> {
        Self::new_with_size_nolock(name, 1 << 20)
    }

    #[cfg(feature = "mmap")]
    pub fn new_with_size<P: AsRef<Path>>(name: P, size: u64) -> Result<Self, SanakirjaError> {
        let env = ::sanakirja::Env::new(name, size, 2);
        match env {
            Ok(env) => Ok(Pristine { env: Arc::new(env) }),
            Err(::sanakirja::Error::IO(e)) => {
                if let std::io::ErrorKind::WouldBlock = e.kind() {
                    Err(SanakirjaError::PristineLocked)
                } else {
                    Err(SanakirjaError::Sanakirja(::sanakirja::Error::IO(e)))
                }
            }
            Err(e) => Err(SanakirjaError::Sanakirja(e)),
        }
    }
    #[cfg(feature = "mmap")]
    pub unsafe fn new_with_size_nolock<P: AsRef<Path>>(
        name: P,
        size: u64,
    ) -> Result<Self, SanakirjaError> {
        Ok(Pristine {
            env: Arc::new(::sanakirja::Env::new_nolock(name, size, 2)?),
        })
    }
    pub fn new_anon() -> Result<Self, SanakirjaError> {
        Self::new_anon_with_size(1 << 20)
    }
    pub fn new_anon_with_size(size: u64) -> Result<Self, SanakirjaError> {
        Ok(Pristine {
            env: Arc::new(::sanakirja::Env::new_anon(size, 2)?),
        })
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
#[repr(usize)]
pub enum Root {
    Version,
    Tree,
    RevTree,
    Inodes,
    RevInodes,
    Internal,
    External,
    NodeTypes,
    RevDep,
    Channels,
    TouchedFiles,
    Dep,
    RevTouchedFiles,
    Partials,
    Remotes,
    // Attribution tables
    PatchAttribution,
    AuthorPatches,
    AIPatchMetadata,
    PatchDependenciesAttribution,
    AuthorStats,
    AuthorInfo,
    PatchDescriptions,
    // Consolidating tags tables
    TagsMetadata,
    TagAttributionSummaries,
}

// Semantic versioning encoded as u64: (major << 32) | (minor << 16) | patch
// Version 1.2.0 = (1 << 32) | (2 << 16) | 0
const VERSION_MAJOR: u64 = 1;
const VERSION_MINOR: u64 = 2;
const VERSION_PATCH: u64 = 0;
const VERSION: u64 = (VERSION_MAJOR << 32) | (VERSION_MINOR << 16) | VERSION_PATCH;

impl Pristine {
    pub fn txn_begin(&self) -> Result<Txn, SanakirjaError> {
        let txn = ::sanakirja::Env::txn_begin(self.env.clone())?;
        let db_version = txn.root(Root::Version as usize);
        debug!(
            "txn_begin: expected version = {} (v{}.{}.{}), database version = {}",
            VERSION, VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH, db_version
        );
        if db_version != VERSION {
            error!(
                "Pristine version mismatch: expected {} (v{}.{}.{}), found {}",
                VERSION, VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH, db_version
            );
            return Err(SanakirjaError::Version);
        }
        debug!("txn_begin");
        fn begin(txn: ::sanakirja::Txn<Arc<::sanakirja::Env>>) -> Option<Txn> {
            debug!("Loading root_db: Channels");
            let channels = txn.root_db(Root::Channels as usize)?;
            debug!("Loading root_db: External");
            let external = txn.root_db(Root::External as usize)?;
            debug!("Loading root_db: Internal");
            let internal = txn.root_db(Root::Internal as usize)?;
            debug!("Loading root_db: NodeTypes");
            let node_types = txn.root_db(Root::NodeTypes as usize)?;
            debug!("Loading root_db: Inodes");
            let inodes = txn.root_db(Root::Inodes as usize)?;
            debug!("Loading root_db: RevInodes");
            let revinodes = txn.root_db(Root::RevInodes as usize)?;
            debug!("Loading root_db: Tree");
            let tree = txn.root_db(Root::Tree as usize)?;
            debug!("Loading root_db: RevTree");
            let revtree = txn.root_db(Root::RevTree as usize)?;
            debug!("Loading root_db: RevDep");
            let revdep = txn.root_db(Root::RevDep as usize)?;
            debug!("Loading root_db: TouchedFiles");
            let touched_files = txn.root_db(Root::TouchedFiles as usize)?;
            debug!("Loading root_db: RevTouchedFiles");
            let rev_touched_files = txn.root_db(Root::RevTouchedFiles as usize)?;
            debug!("Loading root_db: Partials");
            let partials = txn.root_db(Root::Partials as usize)?;
            debug!("Loading root_db: Dep");
            let dep = txn.root_db(Root::Dep as usize)?;
            debug!("Loading root_db: Remotes");
            let remotes = txn.root_db(Root::Remotes as usize)?;
            debug!("Loading root_db: TagsMetadata");
            let tags_metadata = txn.root_db(Root::TagsMetadata as usize)?;
            debug!("Loading root_db: TagAttributionSummaries");
            let tag_attribution_summaries = txn.root_db(Root::TagAttributionSummaries as usize)?;
            debug!("All root_db tables loaded successfully");

            Some(Txn {
                channels,
                external,
                internal,
                node_types,
                inodes,
                revinodes,
                tree,
                revtree,
                revdep,
                touched_files,
                rev_touched_files,
                partials,
                dep,
                remotes,
                tags_metadata,
                tag_attribution_summaries,
                open_channels: Mutex::new(HashMap::default()),
                open_remotes: Mutex::new(HashMap::default()),
                txn,
                counter: 0,
                cur_channel: None,
            })
        }
        debug!("txn begin done");
        if let Some(txn) = begin(txn) {
            Ok(txn)
        } else {
            error!("Failed to load pristine database - one or more root_db tables are missing");
            Err(SanakirjaError::PristineCorrupt)
        }
    }

    pub fn arc_txn_begin(&self) -> Result<ArcTxn<MutTxn<()>>, SanakirjaError> {
        Ok(ArcTxn(Arc::new(RwLock::new(self.mut_txn_begin()?))))
    }

    pub fn mut_txn_begin(&self) -> Result<MutTxn<()>, SanakirjaError> {
        unsafe {
            let mut txn = ::sanakirja::Env::mut_txn_begin(self.env.clone()).unwrap();
            if let Some(version) = txn.root(Root::Version as usize) {
                debug!(
                    "mut_txn_begin: existing database version = {} (expected {})",
                    version, VERSION
                );
                if version != VERSION {
                    error!(
                        "Pristine version mismatch: expected {} (v{}.{}.{}), found {}",
                        VERSION, VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH, version
                    );
                    return Err(SanakirjaError::Version.into());
                }
            } else {
                debug!(
                    "mut_txn_begin: initializing new database with version {} (v{}.{}.{})",
                    VERSION, VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH
                );
                txn.set_root(Root::Version as usize, VERSION);
            }
            Ok(MutTxn {
                channels: if let Some(db) = txn.root_db(Root::Channels as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                external: if let Some(db) = txn.root_db(Root::External as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                internal: if let Some(db) = txn.root_db(Root::Internal as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                node_types: if let Some(db) = txn.root_db(Root::NodeTypes as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                inodes: if let Some(db) = txn.root_db(Root::Inodes as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                revinodes: if let Some(db) = txn.root_db(Root::RevInodes as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                tree: if let Some(db) = txn.root_db(Root::Tree as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                revtree: if let Some(db) = txn.root_db(Root::RevTree as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                revdep: if let Some(db) = txn.root_db(Root::RevDep as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                dep: if let Some(db) = txn.root_db(Root::Dep as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                touched_files: if let Some(db) = txn.root_db(Root::TouchedFiles as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                rev_touched_files: if let Some(db) = txn.root_db(Root::RevTouchedFiles as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                partials: if let Some(db) = txn.root_db(Root::Partials as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                remotes: if let Some(db) = txn.root_db(Root::Remotes as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                tags_metadata: if let Some(db) = txn.root_db(Root::TagsMetadata as usize) {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                tag_attribution_summaries: if let Some(db) =
                    txn.root_db(Root::TagAttributionSummaries as usize)
                {
                    db
                } else {
                    btree::create_db_(&mut txn)?
                },
                open_channels: Mutex::new(HashMap::default()),
                open_remotes: Mutex::new(HashMap::default()),
                txn,
                counter: 0,
                cur_channel: None,
            })
        }
    }
}

pub type Txn = GenericTxn<::sanakirja::Txn<Arc<::sanakirja::Env>>>;
pub type MutTxn<T> = GenericTxn<::sanakirja::MutTxn<Arc<::sanakirja::Env>, T>>;

/// A transaction, used both for mutable and immutable transactions,
/// depending on type parameter `T`.
///
/// In Sanakirja, both `sanakirja::Txn` and `sanakirja::MutTxn`
/// implement `sanakirja::Transaction`, explaining our implementation
/// of `TxnT` for `Txn<T>` for all `T: sanakirja::Transaction`. This
/// covers both mutable and immutable transactions in a single
/// implementation.
pub struct GenericTxn<T: ::sanakirja::LoadPage<Error = ::sanakirja::Error> + ::sanakirja::RootPage>
{
    #[doc(hidden)]
    pub txn: T,
    #[doc(hidden)]
    pub internal: UDb<SerializedHash, NodeId>,
    #[doc(hidden)]
    pub external: UDb<NodeId, SerializedHash>,
    pub(crate) node_types: Db<NodeId, u8>,
    pub inodes: Db<Inode, Position<NodeId>>,
    pub revinodes: Db<Position<NodeId>, Inode>,

    pub tree: UDb<PathId, Inode>,
    pub revtree: UDb<Inode, PathId>,

    revdep: Db<NodeId, NodeId>,
    dep: Db<NodeId, NodeId>,

    touched_files: Db<Position<NodeId>, NodeId>,
    rev_touched_files: Db<NodeId, Position<NodeId>>,

    partials: UDb<SmallStr, Position<NodeId>>,
    channels: UDb<SmallStr, SerializedChannel>,
    remotes: UDb<RemoteId, SerializedRemote>,

    // Consolidating tags tables (Increment 3 - Persistent Storage)
    // Uses UDb for variable-length serialized data storage
    pub(crate) tags_metadata: UDb<SerializedHash, TagBytes>,
    pub(crate) tag_attribution_summaries: UDb<SerializedHash, AttributionSummaryBytes>,

    pub(crate) open_channels: Mutex<HashMap<SmallString, ChannelRef<Self>>>,
    open_remotes: Mutex<HashMap<RemoteId, RemoteRef<Self>>>,
    counter: usize,
    cur_channel: Option<String>,
}

direct_repr!(SerializedPublicKey);

/// This is actually safe because the only non-Send fields are
/// `open_channels` and `open_remotes`, but we can't do anything with
/// a `ChannelRef` whose transaction has been moved to another thread.
unsafe impl<T: ::sanakirja::LoadPage<Error = ::sanakirja::Error> + ::sanakirja::RootPage> Send
    for GenericTxn<T>
{
}

impl Txn {
    pub fn check_database(&self, refs: &mut std::collections::BTreeMap<u64, usize>) {
        unsafe {
            use ::sanakirja::debug::Check;
            debug!("check: internal 0x{:x}", self.internal.db);
            self.internal.add_refs(&self.txn, refs).unwrap();
            debug!("check: external 0x{:x}", self.external.db);
            self.external.add_refs(&self.txn, refs).unwrap();
            debug!("check: inodes 0x{:x}", self.inodes.db);
            self.inodes.add_refs(&self.txn, refs).unwrap();
            debug!("check: revinodes 0x{:x}", self.revinodes.db);
            self.revinodes.add_refs(&self.txn, refs).unwrap();
            debug!("check: tree 0x{:x}", self.tree.db);
            self.tree.add_refs(&self.txn, refs).unwrap();
            debug!("check: revtree 0x{:x}", self.revtree.db);
            self.revtree.add_refs(&self.txn, refs).unwrap();
            debug!("check: revdep 0x{:x}", self.revdep.db);
            self.revdep.add_refs(&self.txn, refs).unwrap();
            debug!("check: dep 0x{:x}", self.dep.db);
            self.dep.add_refs(&self.txn, refs).unwrap();
            debug!("check: touched_files 0x{:x}", self.touched_files.db);
            self.touched_files.add_refs(&self.txn, refs).unwrap();
            debug!("check: rev_touched_files 0x{:x}", self.rev_touched_files.db);
            self.rev_touched_files.add_refs(&self.txn, refs).unwrap();
            debug!("check: partials 0x{:x}", self.partials.db);
            self.partials.add_refs(&self.txn, refs).unwrap();
            debug!("check: channels 0x{:x}", self.channels.db);
            self.channels.add_refs(&self.txn, refs).unwrap();
            for x in btree::iter(&self.txn, &self.channels, None).unwrap() {
                let (name, tup) = x.unwrap();
                debug!("check: channel name: {:?}", name.as_str());
                let graph: Db<Vertex<NodeId>, SerializedEdge> = Db::from_page(tup.graph.into());
                let changes: Db<NodeId, L64> = Db::from_page(tup.changes.into());
                let revchanges: UDb<L64, Pair<NodeId, SerializedMerkle>> =
                    UDb::from_page(tup.revchanges.into());
                let states: UDb<SerializedMerkle, L64> = UDb::from_page(tup.states.into());
                let tags: UDb<L64, TagBytes> = UDb::from_page(tup.tags.into());
                debug!("check: graph 0x{:x}", graph.db);
                graph.add_refs(&self.txn, refs).unwrap();
                debug!("check: changes 0x{:x}", changes.db);
                changes.add_refs(&self.txn, refs).unwrap();
                debug!("check: revchanges 0x{:x}", revchanges.db);
                revchanges.add_refs(&self.txn, refs).unwrap();
                debug!("check: states 0x{:x}", states.db);
                states.add_refs(&self.txn, refs).unwrap();
                debug!("check: tags 0x{:x}", tags.db);
                // TODO: Fix add_refs for unsized TagBytes
                // tags.add_refs(&self.txn, refs).unwrap();
            }
            debug!("check: remotes 0x{:x}", self.remotes.db);
            self.remotes.add_refs(&self.txn, refs).unwrap();
            for x in btree::iter(&self.txn, &self.remotes, None).unwrap() {
                let (name, tup) = x.unwrap();
                debug!("check: remote name: {:?}", name);
                let remote: UDb<L64, Pair<SerializedHash, SerializedMerkle>> =
                    UDb::from_page(tup.remote.into());

                let rev: UDb<SerializedHash, L64> = UDb::from_page(tup.rev.into());
                let states: UDb<SerializedMerkle, L64> = UDb::from_page(tup.states.into());
                let tags: UDb<L64, TagBytes> = UDb::from_page(tup.tags.into());
                debug!("check: remote 0x{:x}", remote.db);
                remote.add_refs(&self.txn, refs).unwrap();
                debug!("check: rev 0x{:x}", rev.db);
                rev.add_refs(&self.txn, refs).unwrap();
                debug!("check: states 0x{:x}", states.db);
                states.add_refs(&self.txn, refs).unwrap();
                debug!("check: tags 0x{:x}", tags.db);
                // TODO: Fix add_refs for unsized TagBytes
                // tags.add_refs(&self.txn, refs).unwrap();
            }
            ::sanakirja::debug::add_free_refs(&self.txn, refs).unwrap();
            ::sanakirja::debug::check_free(&self.txn, &refs);
        }
    }
}

impl<T: ::sanakirja::LoadPage<Error = ::sanakirja::Error> + ::sanakirja::RootPage> GraphTxnT
    for GenericTxn<T>
{
    type Graph = Channel;
    type GraphError = SanakirjaError;

    fn get_graph<'txn>(
        &'txn self,
        db: &Self::Graph,
        key: &Vertex<NodeId>,
        value: Option<&SerializedEdge>,
    ) -> Result<Option<&'txn SerializedEdge>, TxnErr<Self::GraphError>> {
        match ::sanakirja::btree::get(&self.txn, &db.graph, key, value) {
            Ok(Some((k, v))) if k == key => Ok(Some(v)),
            Ok(_) => Ok(None),
            Err(e) => {
                error!("{:?}", e);
                Err(TxnErr(SanakirjaError::PristineCorrupt))
            }
        }
    }

    fn get_external(
        &self,
        p: &NodeId,
    ) -> Result<Option<&SerializedHash>, TxnErr<Self::GraphError>> {
        debug!("get_external {:?}", p);
        if p.is_root() {
            Ok(Some(&HASH_NONE))
        } else {
            match btree::get(&self.txn, &self.external, p, None) {
                Ok(Some((k, v))) if k == p => Ok(Some(v)),
                Ok(_) => Ok(None),
                Err(e) => {
                    error!("{:?}", e);
                    Err(TxnErr(SanakirjaError::PristineCorrupt))
                }
            }
        }
    }

    fn get_internal(
        &self,
        p: &SerializedHash,
    ) -> Result<Option<&NodeId>, TxnErr<Self::GraphError>> {
        // Check if this is the "None" hash (Ed25519 base point)
        if *p == crate::pristine::hash::HASH_NONE {
            Ok(Some(&NodeId::ROOT))
        } else {
            match btree::get(&self.txn, &self.internal, &p, None) {
                Ok(Some((k, v))) if k == p => Ok(Some(v)),
                Ok(_) => Ok(None),
                Err(e) => {
                    error!("{:?}", e);
                    Err(TxnErr(SanakirjaError::PristineCorrupt))
                }
            }
        }
    }

    fn get_node_type(&self, id: &NodeId) -> Result<Option<NodeType>, TxnErr<Self::GraphError>> {
        match btree::get(&self.txn, &self.node_types, id, None) {
            Ok(Some((k, v))) if k == id => Ok(NodeType::from_u8(*v)),
            Ok(_) => Ok(None),
            Err(e) => {
                error!("Error getting node type for id={:?}: {:?}", id, e);
                Err(TxnErr(SanakirjaError::PristineCorrupt))
            }
        }
    }

    type Adj = Adj;

    fn init_adj(
        &self,
        g: &Self::Graph,
        key: Vertex<NodeId>,
        dest: Position<NodeId>,
        min_flag: EdgeFlags,
        max_flag: EdgeFlags,
    ) -> Result<Self::Adj, TxnErr<Self::GraphError>> {
        let edge = SerializedEdge::new(min_flag, dest.change, dest.pos, NodeId::ROOT);
        let mut cursor = btree::cursor::Cursor::new(&self.txn, &g.graph).map_err(TxnErr)?;
        cursor.set(&self.txn, &key, Some(&edge))?;
        Ok(Adj {
            cursor,
            key,
            min_flag,
            max_flag,
        })
    }

    fn next_adj<'a>(
        &'a self,
        _: &Self::Graph,
        a: &mut Self::Adj,
    ) -> Option<Result<&'a SerializedEdge, TxnErr<Self::GraphError>>> {
        next_adj(&self.txn, a).map(|x| x.map_err(|x| TxnErr(x.into())))
    }

    fn find_block(
        &self,
        graph: &Self::Graph,
        p: Position<NodeId>,
    ) -> Result<&Vertex<NodeId>, BlockError<Self::GraphError>> {
        Ok(find_block(&self.txn, &graph.graph, p)?)
    }

    fn find_block_end(
        &self,
        graph: &Self::Graph,
        p: Position<NodeId>,
    ) -> Result<&Vertex<NodeId>, BlockError<Self::GraphError>> {
        Ok(find_block_end(&self.txn, &graph.graph, p)?)
    }
}

impl std::convert::From<BlockError<::sanakirja::Error>> for BlockError<SanakirjaError> {
    fn from(e: BlockError<::sanakirja::Error>) -> Self {
        match e {
            BlockError::Txn(t) => BlockError::Txn(t.into()),
            BlockError::Block { block } => BlockError::Block { block },
        }
    }
}

#[doc(hidden)]
pub fn next_adj<'a, T: ::sanakirja::LoadPage>(
    txn: &'a T,
    a: &mut Adj,
) -> Option<Result<&'a SerializedEdge, T::Error>>
where
    T::Error: std::error::Error,
{
    loop {
        let x: Result<Option<(&Vertex<NodeId>, &SerializedEdge)>, _> = a.cursor.next(txn);
        match x {
            Ok(Some((v, e))) => {
                if *v == a.key {
                    if e.flag() >= a.min_flag {
                        if e.flag() <= a.max_flag {
                            return Some(Ok(e));
                        } else {
                            return None;
                        }
                    }
                } else if *v > a.key {
                    return None;
                }
            }
            Err(e) => return Some(Err(e.into())),
            Ok(None) => {
                return None;
            }
        }
    }
}

#[doc(hidden)]
pub fn find_block<'a, T: ::sanakirja::LoadPage>(
    txn: &'a T,
    graph: &::sanakirja::btree::Db<Vertex<NodeId>, SerializedEdge>,
    p: Position<NodeId>,
) -> Result<&'a Vertex<NodeId>, BlockError<T::Error>>
where
    T::Error: std::error::Error,
{
    if p.change.is_root() {
        return Ok(&Vertex::ROOT);
    }
    let key = Vertex {
        change: p.change,
        start: p.pos,
        end: p.pos,
    };
    let mut cursor =
        btree::cursor::Cursor::new(txn, &graph).map_err(|x| BlockError::Txn(x.into()))?;
    let mut k = if let Some((k, _)) = cursor
        .set(txn, &key, None)
        .map_err(|x| BlockError::Txn(x.into()))?
    {
        k
    } else if let Some((k, _)) = cursor.prev(txn).map_err(|x| BlockError::Txn(x.into()))? {
        k
    } else {
        debug!("find_block: BLOCK ERROR");
        return Err(BlockError::Block { block: p });
    };
    // The only guarantee here is that k is either the first key >=
    // `key`. We might need to rewind by one step if key is strictly
    // larger than the result (i.e. if `p` is in the middle of the
    // key).
    while k.change > p.change || (k.change == p.change && k.start > p.pos) {
        if let Some((k_, _)) = cursor.prev(txn).map_err(|x| BlockError::Txn(x.into()))? {
            k = k_
        } else {
            break;
        }
    }
    loop {
        if k.change == p.change && k.start <= p.pos {
            if k.end > p.pos || (k.start == k.end && k.end == p.pos) {
                return Ok(k);
            }
        } else if k.change > p.change {
            debug!("find_block: BLOCK ERROR");
            return Err(BlockError::Block { block: p });
        }
        if let Some((k_, _)) = cursor.next(txn).map_err(|x| BlockError::Txn(x.into()))? {
            k = k_
        } else {
            break;
        }
    }
    debug!("find_block: BLOCK ERROR");
    Err(BlockError::Block { block: p })
}

#[doc(hidden)]
pub fn find_block_end<'a, T: ::sanakirja::LoadPage>(
    txn: &'a T,
    graph: &::sanakirja::btree::Db<Vertex<NodeId>, SerializedEdge>,
    p: Position<NodeId>,
) -> Result<&'a Vertex<NodeId>, BlockError<T::Error>>
where
    T::Error: std::error::Error,
{
    if p.change.is_root() {
        return Ok(&Vertex::ROOT);
    }
    let key = Vertex {
        change: p.change,
        start: p.pos,
        end: p.pos,
    };
    let mut cursor =
        btree::cursor::Cursor::new(txn, graph).map_err(|x| BlockError::Txn(x.into()))?;
    debug!("key {:?}", key);
    let (mut k, v) = match cursor.set(txn, &key, None) {
        Ok(Some((k, v))) => (k, v),
        Ok(None) => {
            if let Some((k, v)) = cursor.prev(txn).map_err(|x| BlockError::Txn(x.into()))? {
                (k, v)
            } else {
                debug!("find_block_end, no prev");
                return Err(BlockError::Block { block: p });
            }
        }
        Err(e) => {
            debug!("find_block_end: BLOCK ERROR 0 {:?}", e);
            return Err(BlockError::Txn(e.into()));
        }
    };
    debug!("cursor {:?} {:?}", k, v);
    loop {
        debug!("find_block_end, loop, k = {:?}, p = {:?}", k, p);
        if k.change < p.change {
            break;
        } else if k.change == p.change {
            // Here we want to create an edge pointing between `p`
            // and its successor. If k.start == p.pos, the only
            // case where that's what we want is if k.start ==
            // k.end.
            if k.start == p.pos && k.end == p.pos {
                return Ok(k);
            } else if k.start < p.pos {
                break;
            }
        }
        if let Some((k_, _)) = cursor.prev(txn).map_err(|x| BlockError::Txn(x.into()))? {
            k = k_
        } else {
            break;
        }
    }
    // We also want k.end >= p.pos, so we just call next() until
    // we have that.
    debug!("find_block_end, {:?} {:?}", k, p);
    while k.change < p.change || (k.change == p.change && p.pos > k.end) {
        if let Some((k_, _)) = cursor.next(txn).map_err(|x| BlockError::Txn(x.into()))? {
            k = k_
        } else {
            break;
        }
    }
    debug!("find_block_end, {:?} {:?}", k, p);
    if k.change == p.change
        && ((k.start < p.pos && p.pos <= k.end) || (k.start == k.end && k.start == p.pos))
    {
        Ok(k)
    } else {
        debug!("find_block_end: BLOCK ERROR");
        Err(BlockError::Block { block: p })
    }
}

pub struct Adj {
    pub cursor: ::sanakirja::btree::cursor::Cursor<
        Vertex<NodeId>,
        SerializedEdge,
        P<Vertex<NodeId>, SerializedEdge>,
    >,
    pub key: Vertex<NodeId>,
    pub min_flag: EdgeFlags,
    pub max_flag: EdgeFlags,
}

impl<T: ::sanakirja::LoadPage<Error = ::sanakirja::Error> + ::sanakirja::RootPage> GraphIter
    for GenericTxn<T>
{
    type GraphCursor = ::sanakirja::btree::cursor::Cursor<
        Vertex<NodeId>,
        SerializedEdge,
        P<Vertex<NodeId>, SerializedEdge>,
    >;

    fn graph_cursor(
        &self,
        g: &Self::Graph,
        s: Option<&Vertex<NodeId>>,
    ) -> Result<Self::GraphCursor, TxnErr<Self::GraphError>> {
        let mut c = ::sanakirja::btree::cursor::Cursor::new(&self.txn, &g.graph)?;
        if let Some(s) = s {
            c.set(&self.txn, s, None)?;
        }
        Ok(c)
    }

    fn next_graph<'txn>(
        &'txn self,
        _: &Self::Graph,
        a: &mut Self::GraphCursor,
    ) -> Option<Result<(&'txn Vertex<NodeId>, &'txn SerializedEdge), TxnErr<Self::GraphError>>>
    {
        match a.next(&self.txn) {
            Ok(Some(x)) => Some(Ok(x)),
            Ok(None) => None,
            Err(e) => {
                error!("{:?}", e);
                Some(Err(TxnErr(SanakirjaError::PristineCorrupt)))
            }
        }
    }
}

// There is a choice here: the datastructure for `revchanges` is
// intuitively a list. Moreover, when removing a change, we must
// recompute the entire merkle tree after the removed change.
//
// This seems to indicate that a linked list could be an appropriate
// structure (a growable array is excluded because amortised
// complexity is not really acceptable here).
//
// However, we want to be able to answers queries such as "when was
// change X introduced?" without having to read the entire database.
//
// Additionally, even though `SerializedMerkle` has only one
// implementation, and is therefore sized in the current
// implementation, we can't exclude that other algorithms may be
// added, which means that the pages inside linked lists won't even be
// randomly-accessible arrays.
pub struct Channel {
    pub graph: Db<Vertex<NodeId>, SerializedEdge>,
    pub changes: Db<NodeId, L64>,
    pub revchanges: UDb<L64, Pair<NodeId, SerializedMerkle>>,
    pub states: UDb<SerializedMerkle, L64>,
    pub tags: UDb<L64, TagBytes>,
    pub apply_counter: ApplyTimestamp,
    pub name: SmallString,
    pub last_modified: u64,
    pub id: RemoteId,
}

impl<T: ::sanakirja::LoadPage<Error = ::sanakirja::Error> + ::sanakirja::RootPage> ChannelTxnT
    for GenericTxn<T>
{
    type Channel = Channel;

    fn graph<'a>(&self, c: &'a Self::Channel) -> &'a Channel {
        c
    }
    fn name<'a>(&self, c: &'a Self::Channel) -> &'a str {
        c.name.as_str()
    }
    fn id<'a>(&self, c: &'a Self::Channel) -> Option<&'a RemoteId> {
        Some(&c.id)
    }
    fn apply_counter(&self, channel: &Self::Channel) -> u64 {
        channel.apply_counter.into()
    }
    fn last_modified(&self, channel: &Self::Channel) -> u64 {
        channel.last_modified.into()
    }
    fn changes<'a>(&self, channel: &'a Self::Channel) -> &'a Self::Changeset {
        &channel.changes
    }
    fn rev_changes<'a>(&self, channel: &'a Self::Channel) -> &'a Self::RevChangeset {
        &channel.revchanges
    }
    fn tags<'a>(&self, channel: &'a Self::Channel) -> &'a Self::Tags {
        &channel.tags
    }

    type Changeset = Db<NodeId, L64>;
    type RevChangeset = UDb<L64, Pair<NodeId, SerializedMerkle>>;

    fn get_changeset(
        &self,
        channel: &Self::Changeset,
        c: &NodeId,
    ) -> Result<Option<&L64>, TxnErr<Self::GraphError>> {
        match btree::get(&self.txn, channel, c, None) {
            Ok(Some((k, x))) if k == c => Ok(Some(x)),
            Ok(x) => {
                debug!("get_changeset = {:?}", x);
                Ok(None)
            }
            Err(e) => {
                error!("{:?}", e);
                Err(TxnErr(SanakirjaError::PristineCorrupt))
            }
        }
    }
    fn get_revchangeset(
        &self,
        revchanges: &Self::RevChangeset,
        c: &L64,
    ) -> Result<Option<&Pair<NodeId, SerializedMerkle>>, TxnErr<Self::GraphError>> {
        match btree::get(&self.txn, revchanges, c, None) {
            Ok(Some((k, x))) if k == c => Ok(Some(x)),
            Ok(_) => Ok(None),
            Err(e) => {
                error!("{:?}", e);
                Err(TxnErr(SanakirjaError::PristineCorrupt))
            }
        }
    }

    type ChangesetCursor = ::sanakirja::btree::cursor::Cursor<NodeId, L64, P<NodeId, L64>>;

    fn cursor_changeset<'a>(
        &'a self,
        channel: &Self::Changeset,
        pos: Option<NodeId>,
    ) -> Result<Cursor<Self, &'a Self, Self::ChangesetCursor, NodeId, L64>, TxnErr<SanakirjaError>>
    {
        let mut cursor = btree::cursor::Cursor::new(&self.txn, &channel)?;
        if let Some(k) = pos {
            cursor.set(&self.txn, &k, None)?;
        }
        Ok(Cursor {
            cursor,
            txn: self,
            k: std::marker::PhantomData,
            v: std::marker::PhantomData,
            t: std::marker::PhantomData,
        })
    }

    type RevchangesetCursor = ::sanakirja::btree::cursor::Cursor<
        L64,
        Pair<NodeId, SerializedMerkle>,
        UP<L64, Pair<NodeId, SerializedMerkle>>,
    >;

    fn cursor_revchangeset_ref<'a, RT: std::ops::Deref<Target = Self>>(
        txn: RT,
        channel: &Self::RevChangeset,
        pos: Option<L64>,
    ) -> Result<
        Cursor<Self, RT, Self::RevchangesetCursor, L64, Pair<NodeId, SerializedMerkle>>,
        TxnErr<SanakirjaError>,
    > {
        let mut cursor = btree::cursor::Cursor::new(&txn.txn, channel)?;
        if let Some(k) = pos {
            cursor.set(&txn.txn, &k, None)?;
        }
        Ok(Cursor {
            cursor,
            txn,
            k: std::marker::PhantomData,
            v: std::marker::PhantomData,
            t: std::marker::PhantomData,
        })
    }

    fn rev_cursor_revchangeset<'a>(
        &'a self,
        channel: &Self::RevChangeset,
        pos: Option<L64>,
    ) -> Result<
        RevCursor<Self, &'a Self, Self::RevchangesetCursor, L64, Pair<NodeId, SerializedMerkle>>,
        TxnErr<SanakirjaError>,
    > {
        let mut cursor = btree::cursor::Cursor::new(&self.txn, channel)?;
        if let Some(ref pos) = pos {
            cursor.set(&self.txn, pos, None)?;
        } else {
            cursor.set_last(&self.txn)?;
        };
        Ok(RevCursor {
            cursor,
            txn: self,
            k: std::marker::PhantomData,
            v: std::marker::PhantomData,
            t: std::marker::PhantomData,
        })
    }

    fn cursor_revchangeset_next(
        &self,
        cursor: &mut Self::RevchangesetCursor,
    ) -> Result<Option<(&L64, &Pair<NodeId, SerializedMerkle>)>, TxnErr<SanakirjaError>> {
        if let Ok(x) = cursor.next(&self.txn) {
            Ok(x)
        } else {
            Err(TxnErr(SanakirjaError::PristineCorrupt))
        }
    }
    fn cursor_revchangeset_prev(
        &self,
        cursor: &mut Self::RevchangesetCursor,
    ) -> Result<Option<(&L64, &Pair<NodeId, SerializedMerkle>)>, TxnErr<SanakirjaError>> {
        if let Ok(x) = cursor.prev(&self.txn) {
            Ok(x)
        } else {
            Err(TxnErr(SanakirjaError::PristineCorrupt))
        }
    }

    fn cursor_changeset_next(
        &self,
        cursor: &mut Self::ChangesetCursor,
    ) -> Result<Option<(&NodeId, &L64)>, TxnErr<SanakirjaError>> {
        if let Ok(x) = cursor.next(&self.txn) {
            Ok(x)
        } else {
            Err(TxnErr(SanakirjaError::PristineCorrupt))
        }
    }
    fn cursor_changeset_prev(
        &self,
        cursor: &mut Self::ChangesetCursor,
    ) -> Result<Option<(&NodeId, &L64)>, TxnErr<SanakirjaError>> {
        if let Ok(x) = cursor.prev(&self.txn) {
            Ok(x)
        } else {
            Err(TxnErr(SanakirjaError::PristineCorrupt))
        }
    }

    type States = UDb<SerializedMerkle, L64>;
    fn states<'a>(&self, channel: &'a Self::Channel) -> &'a Self::States {
        &channel.states
    }
    fn channel_has_state(
        &self,
        channel: &Self::States,
        m: &SerializedMerkle,
    ) -> Result<Option<L64>, TxnErr<Self::GraphError>> {
        match btree::get(&self.txn, channel, m, None)? {
            Some((k, v)) if k == m => Ok(Some(*v)),
            _ => Ok(None),
        }
    }

    type Tags = UDb<L64, TagBytes>;

    fn is_tagged(&self, tags: &Self::Tags, t: u64) -> Result<bool, TxnErr<Self::GraphError>> {
        let t: L64 = t.into();
        match btree::get(&self.txn, tags, &t, None)? {
            Some((k, _)) => Ok(k == &t),
            _ => Ok(false),
        }
    }

    type TagsCursor = ::sanakirja::btree::cursor::Cursor<
        L64,
        TagBytes,
        ::sanakirja::btree::page_unsized::Page<L64, TagBytes>,
    >;
    fn cursor_tags<'txn>(
        &'txn self,
        channel: &Self::Tags,
        k: Option<L64>,
    ) -> Result<
        crate::pristine::Cursor<Self, &'txn Self, Self::TagsCursor, L64, TagBytes>,
        TxnErr<Self::GraphError>,
    > {
        let mut cursor = btree::cursor::Cursor::new(&self.txn, channel)
            .map_err(|e| TxnErr(SanakirjaError::from(e)))?;
        if let Some(k) = k {
            cursor
                .set(&self.txn, &k, None)
                .map_err(|e| TxnErr(SanakirjaError::from(e)))?;
        }
        Ok(Cursor {
            cursor,
            txn: self,
            k: std::marker::PhantomData,
            v: std::marker::PhantomData,
            t: std::marker::PhantomData,
        })
    }
    fn cursor_tags_next(
        &self,
        cursor: &mut Self::TagsCursor,
    ) -> Result<Option<(&L64, &TagBytes)>, TxnErr<Self::GraphError>> {
        cursor
            .next(&self.txn)
            .map_err(|e| TxnErr(SanakirjaError::from(e)))
    }

    fn cursor_tags_prev(
        &self,
        cursor: &mut Self::TagsCursor,
    ) -> Result<Option<(&L64, &TagBytes)>, TxnErr<Self::GraphError>> {
        cursor
            .prev(&self.txn)
            .map_err(|e| TxnErr(SanakirjaError::from(e)))
    }

    fn iter_tags(
        &self,
        channel: &Self::Tags,
        from: u64,
    ) -> Result<super::Cursor<Self, &Self, Self::TagsCursor, L64, TagBytes>, TxnErr<Self::GraphError>>
    {
        self.cursor_tags(channel, Some(from.into()))
    }

    fn rev_iter_tags(
        &self,
        channel: &Self::Tags,
        from: Option<u64>,
    ) -> Result<
        super::RevCursor<Self, &Self, Self::TagsCursor, L64, TagBytes>,
        TxnErr<Self::GraphError>,
    > {
        let mut cursor = btree::cursor::Cursor::new(&self.txn, channel)
            .map_err(|e| TxnErr(SanakirjaError::from(e)))?;
        if let Some(from) = from {
            cursor
                .set(&self.txn, &from.into(), None)
                .map_err(|e| TxnErr(SanakirjaError::from(e)))?;
        } else {
            cursor
                .set_last(&self.txn)
                .map_err(|e| TxnErr(SanakirjaError::from(e)))?;
        }
        Ok(RevCursor {
            cursor,
            txn: self,
            k: std::marker::PhantomData,
            v: std::marker::PhantomData,
            t: std::marker::PhantomData,
        })
    }
}

impl<T: ::sanakirja::LoadPage<Error = ::sanakirja::Error> + ::sanakirja::RootPage> DepsTxnT
    for GenericTxn<T>
{
    type DepsError = SanakirjaError;
    type Dep = Db<NodeId, NodeId>;
    type Revdep = Db<NodeId, NodeId>;

    sanakirja_table_get!(dep, NodeId, NodeId, DepsError);
    sanakirja_table_get!(revdep, NodeId, NodeId, DepsError);
    type DepCursor = ::sanakirja::btree::cursor::Cursor<NodeId, NodeId, P<NodeId, NodeId>>;
    sanakirja_cursor_ref!(dep, NodeId, NodeId);
    fn iter_dep_ref<RT: std::ops::Deref<Target = Self> + Clone>(
        txn: RT,
        p: &NodeId,
    ) -> Result<super::Cursor<Self, RT, Self::DepCursor, NodeId, NodeId>, TxnErr<Self::DepsError>>
    {
        Self::cursor_dep_ref(txn.clone(), &txn.dep, Some((p, None)))
    }

    sanakirja_table_get!(touched_files, Position<NodeId>, NodeId, DepsError);
    sanakirja_table_get!(rev_touched_files, NodeId, Position<NodeId>, DepsError);

    type Touched_files = Db<Position<NodeId>, NodeId>;

    type Rev_touched_files = Db<NodeId, Position<NodeId>>;

    type Touched_filesCursor =
        ::sanakirja::btree::cursor::Cursor<Position<NodeId>, NodeId, P<Position<NodeId>, NodeId>>;
    sanakirja_iter!(touched_files, Position<NodeId>, NodeId);

    type Rev_touched_filesCursor =
        ::sanakirja::btree::cursor::Cursor<NodeId, Position<NodeId>, P<NodeId, Position<NodeId>>>;
    sanakirja_iter!(rev_touched_files, NodeId, Position<NodeId>);
    fn iter_revdep(
        &self,
        k: &NodeId,
    ) -> Result<super::Cursor<Self, &Self, Self::DepCursor, NodeId, NodeId>, TxnErr<Self::DepsError>>
    {
        self.cursor_dep(&self.revdep, Some((k, None)))
    }

    fn iter_dep(
        &self,
        k: &NodeId,
    ) -> Result<super::Cursor<Self, &Self, Self::DepCursor, NodeId, NodeId>, TxnErr<Self::DepsError>>
    {
        self.cursor_dep(&self.dep, Some((k, None)))
    }

    fn iter_touched(
        &self,
        k: &Position<NodeId>,
    ) -> Result<
        super::Cursor<Self, &Self, Self::Touched_filesCursor, Position<NodeId>, NodeId>,
        TxnErr<Self::DepsError>,
    > {
        self.cursor_touched_files(&self.touched_files, Some((k, None)))
    }

    fn iter_rev_touched(
        &self,
        k: &NodeId,
    ) -> Result<
        super::Cursor<Self, &Self, Self::Rev_touched_filesCursor, NodeId, Position<NodeId>>,
        TxnErr<Self::DepsError>,
    > {
        self.cursor_rev_touched_files(&self.rev_touched_files, Some((k, None)))
    }
}

impl<T: ::sanakirja::LoadPage<Error = ::sanakirja::Error> + ::sanakirja::RootPage> TreeTxnT
    for GenericTxn<T>
{
    type TreeError = SanakirjaError;
    type Inodes = Db<Inode, Position<NodeId>>;
    type Revinodes = Db<Position<NodeId>, Inode>;
    sanakirja_table_get!(inodes, Inode, Position<NodeId>, TreeError, TreeErr);
    sanakirja_table_get!(revinodes, Position<NodeId>, Inode, TreeError, TreeErr);
    sanakirja_cursor!(inodes, Inode, Position<NodeId>, TreeErr);
    // #[cfg(debug_assertions)]
    sanakirja_cursor!(revinodes, Position<NodeId>, Inode, TreeErr);

    type Tree = UDb<PathId, Inode>;
    sanakirja_table_get!(tree, PathId, Inode, TreeError, TreeErr);
    type TreeCursor = ::sanakirja::btree::cursor::Cursor<PathId, Inode, UP<PathId, Inode>>;
    sanakirja_iter!(tree, PathId, Inode, TreeErr);
    type RevtreeCursor = ::sanakirja::btree::cursor::Cursor<Inode, PathId, UP<Inode, PathId>>;
    sanakirja_iter!(revtree, Inode, PathId, TreeErr);

    type Revtree = UDb<Inode, PathId>;
    sanakirja_table_get!(revtree, Inode, PathId, TreeError, TreeErr);

    type Partials = UDb<SmallStr, Position<NodeId>>;
    type PartialsCursor = ::sanakirja::btree::cursor::Cursor<
        SmallStr,
        Position<NodeId>,
        UP<SmallStr, Position<NodeId>>,
    >;
    sanakirja_cursor!(partials, SmallStr, Position<NodeId>, TreeErr);
    type InodesCursor =
        ::sanakirja::btree::cursor::Cursor<Inode, Position<NodeId>, P<Inode, Position<NodeId>>>;
    fn iter_inodes(
        &self,
    ) -> Result<
        super::Cursor<Self, &Self, Self::InodesCursor, Inode, Position<NodeId>>,
        TreeErr<Self::TreeError>,
    > {
        self.cursor_inodes(&self.inodes, None)
    }

    // #[cfg(debug_assertions)]
    type RevinodesCursor =
        ::sanakirja::btree::cursor::Cursor<Position<NodeId>, Inode, P<Position<NodeId>, Inode>>;
    // #[cfg(debug_assertions)]
    fn iter_revinodes(
        &self,
    ) -> Result<
        super::Cursor<Self, &Self, Self::RevinodesCursor, Position<NodeId>, Inode>,
        TreeErr<SanakirjaError>,
    > {
        self.cursor_revinodes(&self.revinodes, None)
    }

    fn iter_partials<'txn>(
        &'txn self,
        k: &str,
    ) -> Result<
        super::Cursor<Self, &'txn Self, Self::PartialsCursor, SmallStr, Position<NodeId>>,
        TreeErr<SanakirjaError>,
    > {
        let k0 = SmallString::from_str(k);
        self.cursor_partials(&self.partials, Some((&k0, None)))
    }
}

impl<T: ::sanakirja::LoadPage<Error = ::sanakirja::Error> + ::sanakirja::RootPage> GenericTxn<T> {
    #[doc(hidden)]
    pub unsafe fn unsafe_load_channel(
        &self,
        name: SmallString,
    ) -> Result<Option<Channel>, TxnErr<SanakirjaError>> {
        debug!("unsafe load channel");
        match btree::get(&self.txn, &self.channels, &name, None)? {
            Some((name_, tup)) if name_ == name.as_ref() => {
                debug!("load_channel: {:?} {:?}", name, tup);
                Ok(Some(Channel {
                    graph: Db::from_page(tup.graph.into()),
                    changes: Db::from_page(tup.changes.into()),
                    revchanges: UDb::from_page(tup.revchanges.into()),
                    states: UDb::from_page(tup.states.into()),
                    tags: UDb::from_page(tup.tags.into()),
                    apply_counter: tup.apply_counter.into(),
                    last_modified: tup.last_modified.into(),
                    id: tup.id,
                    name,
                }))
            }
            _ => {
                debug!("unsafe_load_channel: not found");
                Ok(None)
            }
        }
    }
}

impl<T: ::sanakirja::LoadPage<Error = ::sanakirja::Error> + ::sanakirja::RootPage> TxnT
    for GenericTxn<T>
{
    fn hash_from_prefix(
        &self,
        s: &str,
    ) -> Result<(Hash, NodeId), super::HashPrefixError<Self::GraphError>> {
        let h: SerializedHash = if let Some(ref h) = Hash::from_prefix(s) {
            h.into()
        } else {
            return Err(super::HashPrefixError::Parse(s.to_string()));
        };
        let mut result = None;
        debug!("h = {:?}", h);
        for x in btree::iter(&self.txn, &self.internal, Some((&h, None)))
            .map_err(|e| super::HashPrefixError::Txn(e.into()))?
        {
            let (e, i) = x.map_err(|e| super::HashPrefixError::Txn(e.into()))?;
            debug!("{:?} {:?}", e, i);
            if e < &h {
                continue;
            } else {
                let e: Hash = e.into();
                let b32 = e.to_base32();
                debug!("{:?}", b32);
                let (b32, _) = b32.split_at(s.len().min(b32.len()));
                if b32 != s {
                    break;
                } else if result.is_none() {
                    result = Some((e, *i))
                } else {
                    return Err(super::HashPrefixError::Ambiguous(s.to_string()));
                }
            }
        }
        if let Some(result) = result {
            Ok(result)
        } else {
            Err(super::HashPrefixError::NotFound(s.to_string()))
        }
    }

    fn state_from_prefix(
        &self,
        channel: &Self::States,
        s: &str,
    ) -> Result<(Merkle, L64), super::HashPrefixError<Self::GraphError>> {
        let h: SerializedMerkle = if let Some(ref h) = Merkle::from_prefix(s) {
            h.into()
        } else {
            return Err(super::HashPrefixError::Parse(s.to_string()));
        };
        let mut result = None;
        debug!("h = {:?}", h);
        for x in btree::iter(&self.txn, &channel, Some((&h, None)))
            .map_err(|e| super::HashPrefixError::Txn(e.into()))?
        {
            let (e, i) = x.map_err(|e| super::HashPrefixError::Txn(e.into()))?;
            debug!("{:?} {:?}", e, i);
            if e < &h {
                continue;
            } else {
                let e: Merkle = e.into();
                let b32 = e.to_base32();
                debug!("{:?}", b32);
                let (b32, _) = b32.split_at(s.len().min(b32.len()));
                if b32 != s {
                    break;
                } else if result.is_none() {
                    result = Some((e, *i))
                } else {
                    return Err(super::HashPrefixError::Ambiguous(s.to_string()));
                }
            }
        }
        if let Some(result) = result {
            Ok(result)
        } else {
            Err(super::HashPrefixError::NotFound(s.to_string()))
        }
    }

    fn hash_from_prefix_remote<'txn>(
        &'txn self,
        remote: &RemoteRef<Self>,
        s: &str,
    ) -> Result<Hash, super::HashPrefixError<Self::GraphError>> {
        let remote = remote.db.lock();
        let h: SerializedHash = if let Some(h) = Hash::from_prefix(s) {
            (&h).into()
        } else {
            return Err(super::HashPrefixError::Parse(s.to_string()));
        };
        let mut result = None;
        debug!("h = {:?}", h);
        for x in btree::iter(&self.txn, &remote.rev, Some((&h, None)))
            .map_err(|e| super::HashPrefixError::Txn(e.into()))?
        {
            let (e, _) = x.map_err(|e| super::HashPrefixError::Txn(e.into()))?;
            debug!("{:?}", e);
            if e < &h {
                continue;
            } else {
                let e: Hash = e.into();
                let b32 = e.to_base32();
                debug!("{:?}", b32);
                let (b32, _) = b32.split_at(s.len().min(b32.len()));
                if b32 != s {
                    break;
                } else if result.is_none() {
                    result = Some(e)
                } else {
                    return Err(super::HashPrefixError::Ambiguous(s.to_string()));
                }
            }
        }
        if let Some(result) = result {
            Ok(result)
        } else {
            Err(super::HashPrefixError::NotFound(s.to_string()))
        }
    }

    fn load_channel(
        &self,
        name: &str,
    ) -> Result<Option<ChannelRef<Self>>, TxnErr<Self::GraphError>> {
        let name = SmallString::from_str(name);
        match self.open_channels.lock().entry(name.clone()) {
            Entry::Vacant(v) => {
                if let Some(c) = unsafe { self.unsafe_load_channel(name)? } {
                    Ok(Some(
                        v.insert(ChannelRef {
                            r: Arc::new(RwLock::new(c)),
                        })
                        .clone(),
                    ))
                } else {
                    Ok(None)
                }
            }
            Entry::Occupied(occ) => Ok(Some(occ.get().clone())),
        }
    }

    fn load_remote(
        &self,
        name: &RemoteId,
    ) -> Result<Option<RemoteRef<Self>>, TxnErr<Self::GraphError>> {
        unsafe {
            let name = name.to_owned();
            match self.open_remotes.lock().entry(name.clone()) {
                Entry::Vacant(v) => match btree::get(&self.txn, &self.remotes, &name, None)? {
                    Some((name_, remote)) if name == *name_ => {
                        debug!("load_remote: {:?} {:?}", name_, remote);
                        let r = Remote {
                            remote: UDb::from_page(remote.remote.into()),
                            rev: UDb::from_page(remote.rev.into()),
                            states: UDb::from_page(remote.states.into()),
                            id_rev: remote.id_rev.into(),
                            tags: UDb::from_page(remote.tags.into()),
                            path: remote.path.to_owned(),
                        };
                        for x in btree::iter(&self.txn, &r.remote, None).unwrap() {
                            debug!("remote -> {:?}", x);
                        }
                        for x in btree::iter(&self.txn, &r.rev, None).unwrap() {
                            debug!("rev -> {:?}", x);
                        }
                        for x in btree::iter(&self.txn, &r.states, None).unwrap() {
                            debug!("states -> {:?}", x);
                        }

                        for x in self.iter_remote(&r.remote, 0).unwrap() {
                            debug!("ITER {:?}", x);
                        }

                        let r = RemoteRef {
                            db: Arc::new(Mutex::new(r)),
                            id: name,
                        };
                        Ok(Some(v.insert(r).clone()))
                    }
                    _ => return Ok(None),
                },
                Entry::Occupied(occ) => Ok(Some(occ.get().clone())),
            }
        }
    }

    ///
    type Channels = UDb<SmallStr, SerializedChannel>;
    type ChannelsCursor = ::sanakirja::btree::cursor::Cursor<
        SmallStr,
        SerializedChannel,
        UP<SmallStr, SerializedChannel>,
    >;
    sanakirja_cursor!(channels, SmallStr, SerializedChannel);
    fn channels<'txn>(
        &'txn self,
        start: &str,
    ) -> Result<Vec<ChannelRef<Self>>, TxnErr<Self::GraphError>> {
        let name = SmallString::from_str(start);
        let mut cursor = btree::cursor::Cursor::new(&self.txn, &self.channels)?;
        cursor.set(&self.txn, &name, None)?;
        while let Ok(Some((name, _))) = self.cursor_channels_next(&mut cursor) {
            self.load_channel(name.as_str())?;
        }
        Ok(self
            .open_channels
            .lock()
            .iter()
            .map(|(_, x)| x.clone())
            .collect())
    }

    type Remotes = UDb<RemoteId, SerializedRemote>;
    type RemotesCursor = ::sanakirja::btree::cursor::Cursor<
        RemoteId,
        SerializedRemote,
        UP<RemoteId, SerializedRemote>,
    >;
    sanakirja_cursor!(remotes, RemoteId, SerializedRemote);
    fn iter_remotes<'txn>(
        &'txn self,
        start: &RemoteId,
    ) -> Result<RemotesIterator<'txn, Self>, TxnErr<Self::GraphError>> {
        let mut cursor = btree::cursor::Cursor::new(&self.txn, &self.remotes)?;
        cursor.set(&self.txn, start, None)?;
        Ok(RemotesIterator { cursor, txn: self })
    }

    type Remote = UDb<L64, Pair<SerializedHash, SerializedMerkle>>;
    type Revremote = UDb<SerializedHash, L64>;
    type Remotestates = UDb<SerializedMerkle, L64>;
    type Remotetags = UDb<L64, Pair<SerializedMerkle, SerializedMerkle>>;
    type RemoteCursor = ::sanakirja::btree::cursor::Cursor<
        L64,
        Pair<SerializedHash, SerializedMerkle>,
        UP<L64, Pair<SerializedHash, SerializedMerkle>>,
    >;
    sanakirja_cursor!(remote, L64, Pair<SerializedHash, SerializedMerkle>);
    sanakirja_rev_cursor!(remote, L64, Pair<SerializedHash, SerializedMerkle>);

    fn iter_remote<'txn>(
        &'txn self,
        remote: &Self::Remote,
        k: u64,
    ) -> Result<
        super::Cursor<
            Self,
            &'txn Self,
            Self::RemoteCursor,
            L64,
            Pair<SerializedHash, SerializedMerkle>,
        >,
        TxnErr<Self::GraphError>,
    > {
        self.cursor_remote(remote, Some((&k.into(), None)))
    }

    fn iter_rev_remote<'txn>(
        &'txn self,
        remote: &Self::Remote,
        k: Option<L64>,
    ) -> Result<
        super::RevCursor<
            Self,
            &'txn Self,
            Self::RemoteCursor,
            L64,
            Pair<SerializedHash, SerializedMerkle>,
        >,
        TxnErr<Self::GraphError>,
    > {
        self.rev_cursor_remote(remote, k.as_ref().map(|k| (k, None)))
    }

    fn get_remote(
        &mut self,
        name: RemoteId,
    ) -> Result<Option<RemoteRef<Self>>, TxnErr<Self::GraphError>> {
        unsafe {
            let name = name.to_owned();
            match self.open_remotes.lock().entry(name.clone()) {
                Entry::Vacant(v) => match btree::get(&self.txn, &self.remotes, &name, None)? {
                    Some((name_, remote)) if *name_ == name => {
                        let r = RemoteRef {
                            db: Arc::new(Mutex::new(Remote {
                                remote: UDb::from_page(remote.remote.into()),
                                rev: UDb::from_page(remote.rev.into()),
                                states: UDb::from_page(remote.states.into()),
                                id_rev: remote.id_rev.into(),
                                tags: UDb::from_page(remote.tags.into()),
                                path: remote.path.to_owned(),
                            })),
                            id: name,
                        };
                        v.insert(r);
                    }
                    _ => return Ok(None),
                },
                Entry::Occupied(_) => {}
            }
            Ok(self.open_remotes.lock().get(&name).cloned())
        }
    }

    fn last_remote(
        &self,
        remote: &Self::Remote,
    ) -> Result<Option<(u64, &Pair<SerializedHash, SerializedMerkle>)>, TxnErr<Self::GraphError>>
    {
        debug!("last_remote: {:?}", remote);
        if let Some(x) = btree::rev_iter(&self.txn, remote, None)?.next() {
            let (&k, v) = x?;
            Ok(Some((k.into(), v)))
        } else {
            Ok(None)
        }
    }

    fn last_remote_tag(
        &self,
        _remote: &Self::Tags,
    ) -> Result<Option<(u64, &SerializedMerkle, &SerializedMerkle)>, TxnErr<Self::GraphError>> {
        // Remote tags need full refactoring for new format - breaking change for MVP
        Ok(None)
    }

    fn get_remote_state(
        &self,
        remote: &Self::Remote,
        n: u64,
    ) -> Result<Option<(u64, &Pair<SerializedHash, SerializedMerkle>)>, TxnErr<Self::GraphError>>
    {
        let n = n.into();
        for x in btree::iter(&self.txn, remote, Some((&n, None)))? {
            let (&k, m) = x?;
            if k >= n {
                return Ok(Some((k.into(), m)));
            }
        }
        Ok(None)
    }

    fn get_remote_tag(
        &self,
        _remote: &Self::Tags,
        _n: u64,
    ) -> Result<Option<(u64, &Pair<SerializedMerkle, SerializedMerkle>)>, TxnErr<Self::GraphError>>
    {
        // Remote tags need full refactoring for new format - breaking change for MVP
        Ok(None)
    }

    fn remote_has_change(
        &self,
        remote: &RemoteRef<Self>,
        hash: &SerializedHash,
    ) -> Result<bool, TxnErr<Self::GraphError>> {
        match btree::get(&self.txn, &remote.db.lock().rev, hash, None)? {
            Some((k, _)) if k == hash => Ok(true),
            _ => Ok(false),
        }
    }
    fn remote_has_state(
        &self,
        remote: &RemoteRef<Self>,
        m: &SerializedMerkle,
    ) -> Result<Option<u64>, TxnErr<Self::GraphError>> {
        match btree::get(&self.txn, &remote.db.lock().states, m, None)? {
            Some((k, v)) if k == m => Ok(Some((*v).into())),
            _ => Ok(None),
        }
    }
    fn current_channel(&self) -> Result<&str, Self::GraphError> {
        if let Some(ref c) = self.cur_channel {
            Ok(c)
        } else {
            unsafe {
                let b = self.txn.root_page();
                let len = b[4096 - 256] as usize;
                if len == 0 {
                    Ok("main")
                } else {
                    let s = std::slice::from_raw_parts(b.as_ptr().add(4096 - 255), len);
                    Ok(std::str::from_utf8(s).unwrap_or("main"))
                }
            }
        }
    }
}

impl GraphMutTxnT for MutTxn<()> {
    fn put_graph(
        &mut self,
        graph: &mut Self::Graph,
        k: &Vertex<NodeId>,
        e: &SerializedEdge,
    ) -> Result<bool, TxnErr<Self::GraphError>> {
        Ok(btree::put(&mut self.txn, &mut graph.graph, k, e)?)
    }

    fn del_graph(
        &mut self,
        graph: &mut Self::Graph,
        k: &Vertex<NodeId>,
        e: Option<&SerializedEdge>,
    ) -> Result<bool, TxnErr<Self::GraphError>> {
        Ok(btree::del(&mut self.txn, &mut graph.graph, k, e)?)
    }

    fn debug(&mut self, graph: &mut Self::Graph, extra: &str) {
        ::sanakirja::debug::debug(
            &self.txn,
            &[&graph.graph],
            format!("debug{}{}", self.counter, extra),
            true,
        );
    }

    sanakirja_put_del!(internal, SerializedHash, NodeId, GraphError);
    sanakirja_put_del!(external, NodeId, SerializedHash, GraphError);

    fn put_node_type(
        &mut self,
        id: &NodeId,
        node_type: NodeType,
    ) -> Result<(), TxnErr<Self::GraphError>> {
        let type_u8 = node_type as u8;
        // Delete first to handle updates properly in copy-on-write B-trees
        btree::del(&mut self.txn, &mut self.node_types, id, None)?;
        btree::put(&mut self.txn, &mut self.node_types, id, &type_u8)?;
        Ok(())
    }

    fn del_node_type(&mut self, id: &NodeId) -> Result<bool, TxnErr<Self::GraphError>> {
        Ok(btree::del(&mut self.txn, &mut self.node_types, id, None)?)
    }

    fn split_block(
        &mut self,
        graph: &mut Self::Graph,
        key: &Vertex<NodeId>,
        pos: ChangePosition,
        buf: &mut Vec<SerializedEdge>,
    ) -> Result<(), TxnErr<Self::GraphError>> {
        assert!(pos > key.start);
        assert!(pos < key.end);
        let mut cursor = btree::cursor::Cursor::new(&self.txn, &graph.graph)?;
        cursor.set(&self.txn, key, None)?;
        loop {
            match cursor.next(&self.txn) {
                Ok(Some((k, v))) => {
                    if k > key {
                        break;
                    } else if k < key {
                        continue;
                    }
                    buf.push(*v)
                }
                Ok(None) => break,
                Err(e) => {
                    error!("{:?}", e);
                    return Err(TxnErr(SanakirjaError::PristineCorrupt));
                }
            }
        }
        for chi in buf.drain(..) {
            assert!(chi.introduced_by() != NodeId::ROOT || chi.flag().contains(EdgeFlags::PSEUDO));
            if chi.flag().contains(EdgeFlags::PARENT | EdgeFlags::BLOCK) {
                put_graph_with_rev(
                    self,
                    graph,
                    chi.flag() - EdgeFlags::PARENT,
                    Vertex {
                        change: key.change,
                        start: key.start,
                        end: pos,
                    },
                    Vertex {
                        change: key.change,
                        start: pos,
                        end: key.end,
                    },
                    chi.introduced_by(),
                )?;
            }

            self.del_graph(graph, key, Some(&chi))?;
            self.put_graph(
                graph,
                &if chi.flag().contains(EdgeFlags::PARENT) {
                    Vertex {
                        change: key.change,
                        start: key.start,
                        end: pos,
                    }
                } else {
                    Vertex {
                        change: key.change,
                        start: pos,
                        end: key.end,
                    }
                },
                &chi,
            )?;
        }
        Ok(())
    }
}

impl ChannelMutTxnT for MutTxn<()> {
    fn graph_mut(c: &mut Self::Channel) -> &mut Self::Graph {
        c
    }
    fn touch_channel(&mut self, channel: &mut Self::Channel, t: Option<u64>) {
        use std::time::SystemTime;
        debug!("touch_channel: {:?}", t);
        if let Some(t) = t {
            channel.last_modified = t.into()
        } else if let Ok(duration) = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
            channel.last_modified = (duration.as_millis() as u64 / 1000) * 1000
        }
    }

    fn put_changes(
        &mut self,
        channel: &mut Self::Channel,
        p: NodeId,
        t: ApplyTimestamp,
        h: &Hash,
    ) -> Result<Option<Merkle>, TxnErr<Self::GraphError>> {
        debug!("put_changes {:?} {:?}", p, h);
        if let Some(m) = self.get_changeset(&channel.changes, &p)? {
            debug!("found m = {:?}, p = {:?}", m, p);
            Ok(None)
        } else {
            channel.apply_counter += 1;
            debug!("put_changes {:?} {:?}", t, p);
            let m = if let Some(x) = btree::rev_iter(&self.txn, &channel.revchanges, None)?.next() {
                let (a, b) = x?;
                let a: u64 = (*a).into();
                assert!(a < t);
                (&b.b).into()
            } else {
                Merkle::zero()
            };
            let m = m.next(h);
            assert!(self
                .get_revchangeset(&channel.revchanges, &t.into())?
                .is_none());
            assert!(btree::put(
                &mut self.txn,
                &mut channel.changes,
                &p,
                &t.into()
            )?);
            assert!(btree::put(
                &mut self.txn,
                &mut channel.revchanges,
                &t.into(),
                &Pair { a: p, b: m.into() }
            )?);
            assert!(btree::put(
                &mut self.txn,
                &mut channel.states,
                &m.into(),
                &t.into(),
            )?);
            Ok(Some(m.into()))
        }
    }

    fn del_changes(
        &mut self,
        channel: &mut Self::Channel,
        p: NodeId,
        t: ApplyTimestamp,
    ) -> Result<bool, TxnErr<Self::GraphError>> {
        let mut repl = Vec::new();
        let tl = t.into();
        for x in btree::iter(&self.txn, &channel.revchanges, Some((&tl, None)))? {
            let (t_, p) = x?;
            if *t_ >= tl {
                repl.push((*t_, p.a, p.b))
            }
        }
        let mut m = Merkle::zero();
        for x in btree::rev_iter(&self.txn, &channel.revchanges, Some((&tl, None)))? {
            let (t_, mm) = x?;
            if t_ < &tl {
                m = (&mm.b).into();
                break;
            }
        }
        for (t_, p, m0) in repl.iter() {
            debug!("del_changes {:?} {:?}", t_, p);
            btree::del(&mut self.txn, &mut channel.revchanges, t_, None)?;
            btree::del(&mut self.txn, &mut channel.states, m0, None)?;
            if *t_ > tl {
                m = m.next(self.get_external(p)?.unwrap());
                btree::put(
                    &mut self.txn,
                    &mut channel.revchanges,
                    t_,
                    &Pair { a: *p, b: m.into() },
                )?;
                btree::put(&mut self.txn, &mut channel.states, &m.into(), t_)?;
            }
        }
        btree::del(&mut self.txn, &mut channel.tags, &t.into(), None)?;
        Ok(btree::del(
            &mut self.txn,
            &mut channel.changes,
            &p,
            Some(&t.into()),
        )?)
    }

    fn tags_mut<'a>(&mut self, channel: &'a mut Self::Channel) -> &'a mut Self::Tags {
        &mut channel.tags
    }

    fn put_tags(
        &mut self,
        channel: &mut Self::Tags,
        n: u64,
        m: &Merkle,
    ) -> Result<(), TxnErr<Self::GraphError>> {
        debug!("put_tags timestamp={} merkle={}", n, m.to_base32());

        let n: L64 = n.into();

        // Check if already tagged
        if btree::get(&self.txn, channel, &n, None)?.is_some() {
            info!("put_tags: already tagged at timestamp {}", n);
            return Ok(());
        }

        // Try to look up the full tag from the global tags_metadata table
        let tag_hash: Hash = *m;
        info!(
            "put_tags: looking up tag metadata for tag_hash={}",
            tag_hash.to_base32()
        );
        let serialized = if let Ok(Some(tag_data)) = self.get_tag(&tag_hash) {
            // Use the existing tag data with all its consolidated_changes
            info!("put_tags: found existing tag metadata with consolidated changes");
            // Verify the state in the tag data
            if let Ok(existing_tag) = tag_data.to_tag() {
                info!(
                    "put_tags: existing tag has state={}, consolidated_changes={}",
                    existing_tag.state.to_base32(),
                    existing_tag.consolidated_changes.len()
                );
            }
            tag_data
        } else {
            info!("put_tags: no existing tag metadata found, creating minimal tag");
            info!("put_tags: minimal tag will have state={}", m.to_base32());
            // Fallback: create a minimal tag (for backward compatibility)
            let tag = Tag {
                tag_hash,
                change_file_hash: Some(tag_hash),
                state: m.clone(),
                channel: String::new(),
                consolidated_change_count: 0,
                dependency_count_before: 0,
                consolidated_changes: Vec::new(),
                previous_consolidation: None,
                consolidates_since: None,
                consolidation_timestamp: u64::from(n),
                version: None,
                message: None,
                created_by: None,
                metadata: std::collections::HashMap::new(),
            };
            info!(
                "put_tags: created minimal tag with state={}",
                tag.state.to_base32()
            );
            let serialized = SerializedTag::from_tag(&tag)
                .map_err(|_| TxnErr(SanakirjaError::PristineCorrupt))?;
            // Verify serialization round-trip
            if let Ok(verify_tag) = serialized.to_tag() {
                info!(
                    "put_tags: serialization round-trip check: state={}",
                    verify_tag.state.to_base32()
                );
            }
            serialized
        };

        // Store in the channel's tags table
        info!(
            "put_tags: storing tag in channel tags table at position {}",
            u64::from(n)
        );
        let wrapper = serialized.to_bytes_wrapper();
        btree::put(&mut self.txn, channel, &n, &*wrapper)?;
        info!(
            "put_tags: successfully stored tag at position {}",
            u64::from(n)
        );
        Ok(())
    }

    fn del_tags(
        &mut self,
        channel: &mut Self::Tags,
        t: u64,
    ) -> Result<(), TxnErr<Self::GraphError>> {
        btree::del(&mut self.txn, channel, &t.into(), None)?;
        Ok(())
    }
}

impl DepsMutTxnT for MutTxn<()> {
    sanakirja_put_del!(dep, NodeId, NodeId, DepsError);
    sanakirja_put_del!(revdep, NodeId, NodeId, DepsError);
    sanakirja_put_del!(touched_files, Position<NodeId>, NodeId, DepsError);
    sanakirja_put_del!(rev_touched_files, NodeId, Position<NodeId>, DepsError);
}

// Consolidating Tag Trait Implementations (Increment 3 - Persistent Storage)
// Uses Sanakirja btree operations with UnsizedStorable byte wrappers

impl<T: ::sanakirja::LoadPage<Error = ::sanakirja::Error> + ::sanakirja::RootPage> TagMetadataTxnT
    for GenericTxn<T>
{
    type TagError = SanakirjaError;

    fn get_tag(&self, hash: &Hash) -> Result<Option<SerializedTag>, TxnErr<Self::TagError>> {
        let h: SerializedHash = hash.into();
        if let Some((_, bytes)) = btree::get(&self.txn, &self.tags_metadata, &h, None)? {
            Ok(Some(SerializedTag::from_bytes_wrapper(bytes)))
        } else {
            Ok(None)
        }
    }

    fn get_tag_attribution_summary(
        &self,
        hash: &Hash,
    ) -> Result<Option<SerializedTagAttributionSummary>, TxnErr<Self::TagError>> {
        let h: SerializedHash = hash.into();
        if let Some((_, bytes)) = btree::get(&self.txn, &self.tag_attribution_summaries, &h, None)?
        {
            Ok(Some(SerializedTagAttributionSummary::from_bytes_wrapper(
                bytes,
            )))
        } else {
            Ok(None)
        }
    }

    fn has_tag(&self, hash: &Hash) -> Result<bool, TxnErr<Self::TagError>> {
        let h: SerializedHash = hash.into();
        Ok(btree::get(&self.txn, &self.tags_metadata, &h, None)?.is_some())
    }
}

impl TagMetadataMutTxnT for MutTxn<()> {
    fn put_tag(&mut self, hash: &Hash, tag: &SerializedTag) -> Result<(), TxnErr<Self::TagError>> {
        let h: SerializedHash = hash.into();
        let wrapper = tag.to_bytes_wrapper();
        btree::put(&mut self.txn, &mut self.tags_metadata, &h, &*wrapper)?;
        Ok(())
    }

    fn put_tag_attribution_summary(
        &mut self,
        hash: &Hash,
        summary: &SerializedTagAttributionSummary,
    ) -> Result<(), TxnErr<Self::TagError>> {
        let h: SerializedHash = hash.into();
        let wrapper = summary.to_bytes_wrapper();
        btree::put(
            &mut self.txn,
            &mut self.tag_attribution_summaries,
            &h,
            &*wrapper,
        )?;
        Ok(())
    }

    fn del_tag(&mut self, hash: &Hash) -> Result<bool, TxnErr<Self::TagError>> {
        let h: SerializedHash = hash.into();
        Ok(btree::del(
            &mut self.txn,
            &mut self.tags_metadata,
            &h,
            None,
        )?)
    }

    fn del_tag_attribution_summary(&mut self, hash: &Hash) -> Result<bool, TxnErr<Self::TagError>> {
        let h: SerializedHash = hash.into();
        Ok(btree::del(
            &mut self.txn,
            &mut self.tag_attribution_summaries,
            &h,
            None,
        )?)
    }
}

impl TreeMutTxnT for MutTxn<()> {
    sanakirja_put_del!(inodes, Inode, Position<NodeId>, TreeError, TreeErr);
    sanakirja_put_del!(revinodes, Position<NodeId>, Inode, TreeError, TreeErr);

    sanakirja_put_del!(tree, PathId, Inode, TreeError, TreeErr);
    sanakirja_put_del!(revtree, Inode, PathId, TreeError, TreeErr);

    fn put_partials(
        &mut self,
        k: &str,
        e: Position<NodeId>,
    ) -> Result<bool, TreeErr<Self::TreeError>> {
        let k = SmallString::from_str(k);
        btree::put(&mut self.txn, &mut self.partials, &k, &e).map_err(|e| TreeErr(e.into()))
    }

    fn del_partials(
        &mut self,
        k: &str,
        e: Option<Position<NodeId>>,
    ) -> Result<bool, TreeErr<Self::TreeError>> {
        let k = SmallString::from_str(k);
        btree::del(&mut self.txn, &mut self.partials, &k, e.as_ref()).map_err(|e| TreeErr(e.into()))
    }
}

impl MutTxnT for MutTxn<()> {
    fn put_remote(
        &mut self,
        remote: &mut RemoteRef<Self>,
        k: u64,
        v: (Hash, Merkle),
    ) -> Result<bool, TxnErr<Self::GraphError>> {
        let mut remote = remote.db.lock();
        let h = (&v.0).into();
        let m: SerializedMerkle = (&v.1).into();
        btree::put(
            &mut self.txn,
            &mut remote.remote,
            &k.into(),
            &Pair { a: h, b: m.clone() },
        )?;
        debug!("remote.remote after put: {:?}", remote.remote);
        btree::put(&mut self.txn, &mut remote.states, &m, &k.into())?;

        // Phase 2: Register node type if available
        // Query node type from database to ensure it's tracked
        if let Ok(Some(internal)) = self.get_internal(&h) {
            if let Ok(Some(node_type)) = self.get_node_type(internal) {
                debug!(
                    "Remote entry {} has node type {:?}",
                    v.0.to_base32(),
                    node_type
                );
            } else {
                debug!(
                    "Remote entry {} missing node type in database",
                    v.0.to_base32()
                );
            }
        }

        // if v.2 {
        //     self.put_tags(&mut remote.tags, k, &v.1)?;
        // }
        Ok(btree::put(&mut self.txn, &mut remote.rev, &h, &k.into())?)
    }

    fn del_remote(
        &mut self,
        remote: &mut RemoteRef<Self>,
        k: u64,
    ) -> Result<bool, TxnErr<Self::GraphError>> {
        let mut remote = remote.db.lock();
        let k = k.into();
        match btree::get(&self.txn, &remote.remote, &k, None)? {
            Some((k0, p)) if k0 == &k => {
                debug!("del_remote {:?} {:?}", k0, p);
                let p = p.clone();
                btree::del(&mut self.txn, &mut remote.rev, &p.a, None)?;
                btree::del(&mut self.txn, &mut remote.states, &p.b, None)?;
                Ok(btree::del(
                    &mut self.txn,
                    &mut remote.remote,
                    &k.into(),
                    None,
                )?)
            }
            x => {
                debug!("not found, {:?}", x);
                Ok(false)
            }
        }
    }

    fn open_or_create_channel(&mut self, name: &str) -> Result<ChannelRef<Self>, Self::GraphError> {
        unsafe {
            let name = crate::small_string::SmallString::from_str(name);
            let mut commit = None;
            let result = match self.open_channels.lock().entry(name.clone()) {
                Entry::Vacant(v) => {
                    let r = match btree::get(&self.txn, &self.channels, &name, None)? {
                        Some((name_, b)) if name_ == name.as_ref() => ChannelRef {
                            r: Arc::new(RwLock::new(Channel {
                                graph: Db::from_page(b.graph.into()),
                                changes: Db::from_page(b.changes.into()),
                                revchanges: UDb::from_page(b.revchanges.into()),
                                states: UDb::from_page(b.states.into()),
                                tags: UDb::from_page(b.tags.into()),
                                apply_counter: b.apply_counter.into(),
                                last_modified: b.last_modified.into(),
                                id: b.id,
                                name: name.clone(),
                            })),
                        },
                        _ => {
                            let br = ChannelRef {
                                r: Arc::new(RwLock::new(Channel {
                                    graph: btree::create_db_(&mut self.txn)?,
                                    changes: btree::create_db_(&mut self.txn)?,
                                    revchanges: btree::create_db_(&mut self.txn)?,
                                    states: btree::create_db_(&mut self.txn)?,
                                    tags: btree::create_db_(&mut self.txn)?,
                                    id: {
                                        let mut rng = rand::thread_rng();
                                        use rand::Rng;
                                        let mut x = RemoteId([0; 16]);
                                        for x in x.0.iter_mut() {
                                            *x = rng.gen()
                                        }
                                        x
                                    },
                                    apply_counter: 0,
                                    last_modified: 0,
                                    name: name.clone(),
                                })),
                            };
                            commit = Some(br.clone());
                            br
                        }
                    };
                    v.insert(r).clone()
                }
                Entry::Occupied(occ) => occ.get().clone(),
            };
            if let Some(commit) = commit {
                self.put_channel(commit)?;
            }
            Ok(result)
        }
    }

    fn fork(
        &mut self,
        channel: &ChannelRef<Self>,
        new_name: &str,
    ) -> Result<ChannelRef<Self>, ForkError<Self::GraphError>> {
        let channel = channel.r.read();
        let name = SmallString::from_str(new_name);
        match btree::get(&self.txn, &self.channels, &name, None)
            .map_err(|e| ForkError::Txn(e.into()))?
        {
            Some((name_, _)) if name_ == name.as_ref() => {
                Err(super::ForkError::ChannelNameExists(new_name.to_string()))
            }
            _ => {
                let br = ChannelRef {
                    r: Arc::new(RwLock::new(Channel {
                        graph: btree::fork_db(&mut self.txn, &channel.graph)
                            .map_err(|e| ForkError::Txn(e.into()))?,
                        changes: btree::fork_db(&mut self.txn, &channel.changes)
                            .map_err(|e| ForkError::Txn(e.into()))?,
                        revchanges: btree::fork_db(&mut self.txn, &channel.revchanges)
                            .map_err(|e| ForkError::Txn(e.into()))?,
                        states: btree::fork_db(&mut self.txn, &channel.states)
                            .map_err(|e| ForkError::Txn(e.into()))?,
                        tags: btree::fork_db(&mut self.txn, &channel.tags)
                            .map_err(|e| ForkError::Txn(e.into()))?,
                        name: name.clone(),
                        apply_counter: channel.apply_counter,
                        last_modified: channel.last_modified,
                        id: {
                            let mut rng = rand::thread_rng();
                            use rand::Rng;
                            let mut x = RemoteId([0; 16]);
                            for x in x.0.iter_mut() {
                                *x = rng.gen()
                            }
                            x
                        },
                    })),
                };
                self.open_channels.lock().insert(name, br.clone());
                Ok(br)
            }
        }
    }

    fn rename_channel(
        &mut self,
        channel: &mut ChannelRef<Self>,
        new_name: &str,
    ) -> Result<(), ForkError<Self::GraphError>> {
        let name = SmallString::from_str(new_name);
        match btree::get(&self.txn, &self.channels, &name, None)
            .map_err(|e| ForkError::Txn(e.into()))?
        {
            Some((name_, _)) if name_ == name.as_ref() => {
                Err(super::ForkError::ChannelNameExists(new_name.to_string()))
            }
            _ => {
                btree::del(
                    &mut self.txn,
                    &mut self.channels,
                    &channel.r.read().name,
                    None,
                )
                .map_err(|e| ForkError::Txn(e.into()))?;
                std::mem::drop(
                    self.open_channels
                        .lock()
                        .remove(&channel.r.read().name)
                        .unwrap(),
                );
                channel.r.write().name = name.clone();
                self.open_channels.lock().insert(name, channel.clone());
                Ok(())
            }
        }
    }

    fn drop_channel(&mut self, name0: &str) -> Result<bool, Self::GraphError> {
        unsafe {
            debug!(target: "drop_channel", "drop channel {:?}", name0);
            let name = SmallString::from_str(name0);
            let channel = if let Some(channel) = self.open_channels.lock().remove(&name) {
                let channel = Arc::try_unwrap(channel.r)
                    .map_err(|_| SanakirjaError::ChannelRc {
                        c: name0.to_string(),
                    })?
                    .into_inner();
                Some((
                    channel.graph,
                    channel.changes,
                    channel.revchanges,
                    channel.states,
                    channel.tags,
                ))
            } else if let Some((name_, chan)) = btree::get(&self.txn, &self.channels, &name, None)?
            {
                if name_ == name.as_ref() {
                    Some((
                        Db::from_page(chan.graph.into()),
                        Db::from_page(chan.changes.into()),
                        UDb::from_page(chan.revchanges.into()),
                        UDb::from_page(chan.states.into()),
                        UDb::from_page(chan.tags.into()),
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            btree::del(&mut self.txn, &mut self.channels, &name, None)?;
            if let Some((a, b, c, d, e)) = channel {
                let mut unused_changes = Vec::new();
                'outer: for x in btree::rev_iter(&self.txn, &c, None)? {
                    let (_, p) = x?;
                    debug!(target: "drop_channel", "testing unused change: {:?}", p);
                    for chan in self.channels("").map_err(|e| e.0)? {
                        debug!(target: "drop_channel", "channel: {:?}", name);
                        let chan = chan.read();
                        assert_ne!(chan.name.as_str(), name0);
                        if self
                            .channel_has_state(&chan.states, &p.b)
                            .map_err(|e| e.0)?
                            .is_some()
                        {
                            // This other channel is in the same state as
                            // our dropped channel is, so all subsequent
                            // patches are in use.
                            break 'outer;
                        }
                        if self
                            .get_changeset(&chan.changes, &p.a)
                            .map_err(|e| e.0)?
                            .is_some()
                        {
                            // This channel has a patch, move on.
                            continue 'outer;
                        }
                    }

                    debug!(target: "drop_channel", "actually unused: {:?}", p);
                    unused_changes.push(p.a);
                }
                let mut deps = Vec::new();
                for ch in unused_changes.iter() {
                    for x in btree::iter(&self.txn, &self.dep, Some((ch, None)))? {
                        let (k, v) = x?;
                        if k > ch {
                            break;
                        }
                        deps.push((*k, *v));
                    }
                    for (k, v) in deps.drain(..) {
                        debug!(target: "drop_channel", "deleting from revdep: {:?} {:?}", k, v);
                        btree::del(&mut self.txn, &mut self.dep, &k, Some(&v))?;
                        btree::del(&mut self.txn, &mut self.revdep, &v, Some(&k))?;
                    }
                }
                btree::drop(&mut self.txn, a)?;
                btree::drop(&mut self.txn, b)?;
                btree::drop(&mut self.txn, c)?;
                btree::drop(&mut self.txn, d)?;
                btree::drop(&mut self.txn, e)?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }

    fn open_or_create_remote(
        &mut self,
        id: RemoteId,
        path: &str,
    ) -> Result<RemoteRef<Self>, Self::GraphError> {
        unsafe {
            let mut commit = None;
            match self.open_remotes.lock().entry(id) {
                Entry::Vacant(v) => {
                    let r = match btree::get(&self.txn, &self.remotes, &id, None)? {
                        Some((name_, remote)) if *name_ == id => RemoteRef {
                            db: Arc::new(Mutex::new(Remote {
                                remote: UDb::from_page(remote.remote.into()),
                                rev: UDb::from_page(remote.rev.into()),
                                states: UDb::from_page(remote.states.into()),
                                id_rev: remote.id_rev.into(),
                                tags: UDb::from_page(remote.tags.into()),
                                path: SmallString::from_str(path),
                            })),
                            id,
                        },
                        _ => {
                            let br = RemoteRef {
                                db: Arc::new(Mutex::new(Remote {
                                    remote: btree::create_db_(&mut self.txn)?,
                                    rev: btree::create_db_(&mut self.txn)?,
                                    states: btree::create_db_(&mut self.txn)?,
                                    id_rev: 0u64.into(),
                                    tags: btree::create_db_(&mut self.txn)?,
                                    path: SmallString::from_str(path),
                                })),
                                id,
                            };
                            commit = Some(br.clone());
                            br
                        }
                    };
                    v.insert(r);
                }
                Entry::Occupied(_) => {}
            }
            if let Some(commit) = commit {
                self.put_remotes(commit)?;
            }
            Ok(self.open_remotes.lock().get(&id).unwrap().clone())
        }
    }

    fn drop_remote(&mut self, remote: RemoteRef<Self>) -> Result<bool, Self::GraphError> {
        let r = self.open_remotes.lock().remove(&remote.id).unwrap();
        std::mem::drop(remote);
        assert_eq!(Arc::strong_count(&r.db), 1);
        Ok(btree::del(&mut self.txn, &mut self.remotes, &r.id, None)?)
    }

    fn drop_named_remote(&mut self, id: RemoteId) -> Result<bool, Self::GraphError> {
        if let Some(r) = self.open_remotes.lock().remove(&id) {
            assert_eq!(Arc::strong_count(&r.db), 1);
        }
        Ok(btree::del(&mut self.txn, &mut self.remotes, &id, None)?)
    }

    fn commit(mut self) -> Result<(), Self::GraphError> {
        use std::ops::DerefMut;
        {
            let open_channels =
                std::mem::replace(self.open_channels.lock().deref_mut(), HashMap::default());
            for (name, channel) in open_channels {
                debug!("commit_channel {:?}", name);
                self.commit_channel(channel)?
            }
        }
        {
            let open_remotes =
                std::mem::replace(self.open_remotes.lock().deref_mut(), HashMap::default());
            for (name, remote) in open_remotes {
                debug!("commit remote {:?}", name);
                self.commit_remote(remote)?
            }
        }
        if let Some(ref cur) = self.cur_channel {
            unsafe {
                assert!(cur.len() < 256);
                let b = self.txn.root_page_mut();
                b[4096 - 256] = cur.len() as u8;
                std::ptr::copy(cur.as_ptr(), b.as_mut_ptr().add(4096 - 255), cur.len())
            }
        }
        // No need to set `Root::Version`, it is set at init.
        debug!(
            "{:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x}",
            self.tree.db,
            self.revtree.db,
            self.inodes.db,
            self.revinodes.db,
            self.internal.db,
            self.external.db,
            self.node_types.db,
            self.revdep.db,
            self.channels.db,
            self.remotes.db,
            self.touched_files.db,
            self.dep.db,
            self.rev_touched_files.db,
            self.partials.db,
            self.tags_metadata.db,
            self.tag_attribution_summaries.db,
        );
        self.txn
            .set_root(Root::Tree as usize, u64::from(self.tree.db).into());
        self.txn
            .set_root(Root::RevTree as usize, u64::from(self.revtree.db).into());
        self.txn
            .set_root(Root::Inodes as usize, u64::from(self.inodes.db).into());
        self.txn
            .set_root(Root::RevInodes as usize, self.revinodes.db.into());
        self.txn
            .set_root(Root::Internal as usize, self.internal.db.into());
        self.txn
            .set_root(Root::External as usize, self.external.db.into());
        self.txn
            .set_root(Root::NodeTypes as usize, self.node_types.db.into());
        self.txn
            .set_root(Root::RevDep as usize, self.revdep.db.into());
        self.txn
            .set_root(Root::Channels as usize, self.channels.db.into());
        self.txn
            .set_root(Root::Remotes as usize, self.remotes.db.into());
        self.txn
            .set_root(Root::TouchedFiles as usize, self.touched_files.db.into());
        self.txn.set_root(Root::Dep as usize, self.dep.db.into());
        self.txn.set_root(
            Root::RevTouchedFiles as usize,
            self.rev_touched_files.db.into(),
        );
        self.txn
            .set_root(Root::Partials as usize, self.partials.db.into());
        self.txn
            .set_root(Root::TagsMetadata as usize, self.tags_metadata.db.into());
        self.txn.set_root(
            Root::TagAttributionSummaries as usize,
            self.tag_attribution_summaries.db.into(),
        );
        self.txn.commit()?;
        Ok(())
    }

    fn set_current_channel(&mut self, cur: &str) -> Result<(), Self::GraphError> {
        self.cur_channel = Some(cur.to_string());
        Ok(())
    }
}

impl Txn {
    pub fn load_const_channel(&self, name: &str) -> Result<Option<Channel>, SanakirjaError> {
        unsafe {
            let name = SmallString::from_str(name);
            match btree::get(&self.txn, &self.channels, &name, None)? {
                Some((name_, c)) if name.as_ref() == name_ => {
                    debug!("load_const_channel = {:?} {:?}", name_, c);
                    Ok(Some(Channel {
                        graph: Db::from_page(c.graph.into()),
                        changes: Db::from_page(c.changes.into()),
                        revchanges: UDb::from_page(c.revchanges.into()),
                        states: UDb::from_page(c.states.into()),
                        tags: UDb::from_page(c.tags.into()),
                        apply_counter: c.apply_counter.into(),
                        last_modified: c.last_modified.into(),
                        id: c.id,
                        name,
                    }))
                }
                _ => Ok(None),
            }
        }
    }
}

impl<T> MutTxn<T> {
    fn put_channel(&mut self, channel: ChannelRef<Self>) -> Result<(), SanakirjaError> {
        debug!("Commit_channel.");
        let channel = channel.r.read();
        debug!("Commit_channel, dbs_channels = {:?}", self.channels);
        btree::del(&mut self.txn, &mut self.channels, &channel.name, None)?;
        debug!(
            "channels: {:x} {:x} {:x} {:x} {:x}",
            channel.graph.db,
            channel.changes.db,
            channel.revchanges.db,
            channel.states.db,
            channel.tags.db,
        );
        let sc = SerializedChannel {
            graph: u64::from(channel.graph.db).into(),
            changes: u64::from(channel.changes.db).into(),
            revchanges: u64::from(channel.revchanges.db).into(),
            states: u64::from(channel.states.db).into(),
            tags: u64::from(channel.tags.db).into(),
            apply_counter: channel.apply_counter.into(),
            last_modified: channel.last_modified.into(),
            id: channel.id,
        };
        btree::put(&mut self.txn, &mut self.channels, &channel.name, &sc)?;
        debug!("Commit_channel, self.channels = {:?}", self.channels);
        Ok(())
    }

    fn commit_channel(&mut self, channel: ChannelRef<Self>) -> Result<(), SanakirjaError> {
        std::mem::drop(self.open_channels.lock().remove(&channel.r.read().name));
        self.put_channel(channel)
    }

    fn put_remotes(&mut self, remote: RemoteRef<Self>) -> Result<(), SanakirjaError> {
        btree::del(&mut self.txn, &mut self.remotes, &remote.id, None)?;
        debug!("Commit_remote, dbs_remotes = {:?}", self.remotes);
        let r = remote.db.lock();
        let rr = OwnedSerializedRemote {
            _remote: u64::from(r.remote.db).into(),
            _rev: u64::from(r.rev.db).into(),
            _states: u64::from(r.states.db).into(),
            _id_rev: r.id_rev.into(),
            _tags: u64::from(r.tags.db).into(),
            _path: r.path.clone(),
        };
        debug!("put {:?}", rr);
        btree::put(&mut self.txn, &mut self.remotes, &remote.id, &rr)?;
        debug!("Commit_remote, self.dbs.remotes = {:?}", self.remotes);
        Ok(())
    }

    fn commit_remote(&mut self, remote: RemoteRef<Self>) -> Result<(), SanakirjaError> {
        std::mem::drop(self.open_remotes.lock().remove(&remote.id));
        // assert_eq!(Rc::strong_count(&remote.db), 1);
        self.put_remotes(remote)
    }
}

direct_repr!(NodeId);
impl ::sanakirja::debug::Check for NodeId {}

direct_repr!(Vertex<NodeId>);
impl ::sanakirja::debug::Check for Vertex<NodeId> {}

direct_repr!(Position<NodeId>);
impl ::sanakirja::debug::Check for Position<NodeId> {}

direct_repr!(SerializedEdge);
impl ::sanakirja::debug::Check for SerializedEdge {}

impl ::sanakirja::debug::Check for PathId {}
impl Storable for PathId {
    fn compare<T>(&self, _: &T, x: &Self) -> std::cmp::Ordering {
        self.cmp(x)
    }
    type PageReferences = std::iter::Empty<u64>;
    fn page_references(&self) -> Self::PageReferences {
        std::iter::empty()
    }
}
impl UnsizedStorable for PathId {
    const ALIGN: usize = 8;
    fn size(&self) -> usize {
        9 + self.basename.len()
    }
    unsafe fn onpage_size(p: *const u8) -> usize {
        let len = *(p.add(8)) as usize;
        9 + len
    }
    unsafe fn from_raw_ptr<'a, T>(_: &T, p: *const u8) -> &'a Self {
        path_id_from_raw_ptr(p)
    }
    unsafe fn write_to_page_alloc<T: AllocPage>(&self, t: &mut T, p: *mut u8) {
        *(p as *mut u64) = (self.parent_inode.0).0;
        self.basename.write_to_page_alloc(t, p.add(8))
    }
}

unsafe fn path_id_from_raw_ptr<'a>(p: *const u8) -> &'a PathId {
    let len = *(p.add(8)) as usize;
    std::mem::transmute(std::slice::from_raw_parts(p, 1 + len as usize))
}

#[test]
fn pathid_repr() {
    let o = OwnedPathId {
        parent_inode: Inode::ROOT,
        basename: SmallString::from_str("blablabla"),
    };
    let mut x = vec![0u8; 200];

    unsafe {
        o.write_to_page(x.as_mut_ptr());
        let p = path_id_from_raw_ptr(x.as_ptr());
        assert_eq!(p.basename.as_str(), "blablabla");
        assert_eq!(p.parent_inode, Inode::ROOT);
    }
}

direct_repr!(Inode);
impl ::sanakirja::debug::Check for Inode {}
direct_repr!(SerializedMerkle);
impl ::sanakirja::debug::Check for SerializedMerkle {}
// SerializedHash is now a type alias for SerializedMerkle, so no need for duplicate impl

impl<A: ::sanakirja::debug::Check, B: ::sanakirja::debug::Check> ::sanakirja::debug::Check
    for Pair<A, B>
{
    fn add_refs<T: LoadPage>(
        &self,
        txn: &T,
        pages: &mut std::collections::BTreeMap<u64, usize>,
    ) -> Result<(), T::Error>
    where
        T::Error: std::fmt::Debug,
    {
        self.a.add_refs(txn, pages)?;
        self.b.add_refs(txn, pages)
    }
}
impl<A: Storable, B: Storable> Storable for Pair<A, B> {
    type PageReferences = core::iter::Chain<A::PageReferences, B::PageReferences>;
    fn page_references(&self) -> Self::PageReferences {
        self.a.page_references().chain(self.b.page_references())
    }
    fn compare<T: LoadPage>(&self, t: &T, b: &Self) -> core::cmp::Ordering {
        match self.a.compare(t, &b.a) {
            core::cmp::Ordering::Equal => self.b.compare(t, &b.b),
            ord => ord,
        }
    }
}

impl<A: Ord + UnsizedStorable, B: Ord + UnsizedStorable> UnsizedStorable for Pair<A, B> {
    const ALIGN: usize = std::mem::align_of::<(A, B)>();

    fn size(&self) -> usize {
        let a = self.a.size();
        let b_off = (a + (B::ALIGN - 1)) & !(B::ALIGN - 1);
        (b_off + self.b.size() + (Self::ALIGN - 1)) & !(Self::ALIGN - 1)
    }
    unsafe fn onpage_size(p: *const u8) -> usize {
        let a = A::onpage_size(p);
        let b_off = (a + (B::ALIGN - 1)) & !(B::ALIGN - 1);
        let b_size = B::onpage_size(p.add(b_off));
        (b_off + b_size + (Self::ALIGN - 1)) & !(Self::ALIGN - 1)
    }
    unsafe fn from_raw_ptr<'a, T>(_: &T, p: *const u8) -> &'a Self {
        &*(p as *const Self)
    }
    unsafe fn write_to_page_alloc<T: sanakirja::AllocPage>(&self, t: &mut T, p: *mut u8) {
        self.a.write_to_page_alloc(t, p);
        let off = (self.a.size() + (B::ALIGN - 1)) & !(B::ALIGN - 1);
        self.b.write_to_page_alloc(t, p.add(off));
    }
}

impl ::sanakirja::debug::Check for SerializedRemote {}
impl Storable for SerializedRemote {
    type PageReferences = std::iter::Empty<u64>;
    fn page_references(&self) -> Self::PageReferences {
        std::iter::empty()
    }
    fn compare<T: LoadPage>(&self, _t: &T, b: &Self) -> core::cmp::Ordering {
        self.cmp(b)
    }
}

const REMOTE_LEN: usize = 40;

impl UnsizedStorable for SerializedRemote {
    const ALIGN: usize = 8;

    fn size(&self) -> usize {
        REMOTE_LEN + 1 + self.path.len()
    }
    unsafe fn onpage_size(p: *const u8) -> usize {
        REMOTE_LEN + 1 + (*p.add(REMOTE_LEN)) as usize
    }
    unsafe fn from_raw_ptr<'a, T>(_: &T, p: *const u8) -> &'a Self {
        let len = *p.add(REMOTE_LEN) as usize;
        let m: &SerializedRemote =
            std::mem::transmute(std::slice::from_raw_parts(p, 1 + len as usize));
        m
    }
    unsafe fn write_to_page_alloc<T: sanakirja::AllocPage>(&self, _: &mut T, p: *mut u8) {
        std::ptr::copy(
            &self.remote as *const L64 as *const u8,
            p,
            REMOTE_LEN + 1 + self.path.len(),
        );
        debug!(
            "write_to_page: {:?}",
            std::slice::from_raw_parts(p, REMOTE_LEN + 1 + self.path.len())
        );
    }
}

#[derive(Debug)]
#[repr(C)]
struct OwnedSerializedRemote {
    _remote: L64,
    _rev: L64,
    _states: L64,
    _id_rev: L64,
    _tags: L64,
    _path: SmallString,
}

impl std::ops::Deref for OwnedSerializedRemote {
    type Target = SerializedRemote;
    fn deref(&self) -> &Self::Target {
        let len = REMOTE_LEN + 1 + self._path.len() as usize;
        unsafe {
            std::mem::transmute(std::slice::from_raw_parts(
                self as *const Self as *const u8,
                len,
            ))
        }
    }
}

direct_repr!(SerializedChannel);
impl ::sanakirja::debug::Check for SerializedChannel {}

direct_repr!(RemoteId);
impl ::sanakirja::debug::Check for RemoteId {}
